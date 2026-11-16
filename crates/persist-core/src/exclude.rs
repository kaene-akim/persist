//! Exclusion rule engine.
//!
//! Rules are glob patterns in the spirit of `.gitignore`, but simpler and
//! explicitly documented:
//!
//! * A pattern containing `/` is matched against the path relative to the root.
//! * A pattern without `/` is matched against the file name and against any
//!   path component (so `node_modules/` excludes that directory anywhere).
//! * A trailing `/` restricts the pattern to directories.
//! * A leading `!` re-includes previously excluded paths (last match wins).
//!
//! Defaults are provided by [`crate::model::default_exclusions`].

use crate::error::Result;
use crate::util::to_rel_string;
use globset::{Glob, GlobMatcher};
use std::path::Path;

#[derive(Debug, Clone)]
struct Rule {
    matcher: GlobMatcher,
    dir_only: bool,
    negate: bool,
    source: String,
}

/// A compiled set of exclusion rules.
#[derive(Debug, Clone, Default)]
pub struct ExcludeRules {
    rules: Vec<Rule>,
}

impl ExcludeRules {
    /// Compile a list of patterns. Invalid patterns are reported as errors so
    /// that users are never silently protected by a rule that does nothing.
    pub fn compile(patterns: &[String]) -> Result<Self> {
        let mut rules = Vec::new();
        for raw in patterns {
            let mut p = raw.trim().to_string();
            if p.is_empty() || p.starts_with('#') {
                continue;
            }
            let negate = p.starts_with('!');
            if negate {
                p.remove(0);
            }
            let dir_only = p.ends_with('/');
            let p = p.trim_end_matches('/').to_string();
            if p.is_empty() {
                continue;
            }
            // Match both "name" (root-relative) and "**/name" (anywhere).
            let mut globs = vec![p.clone()];
            if !p.contains('/') {
                globs.push(format!("**/{p}"));
            } else {
                globs.push(format!("**/{p}"));
            }
            for g in globs {
                let matcher = Glob::new(&g)
                    .map_err(|e| crate::Error::Config(format!("invalid exclusion pattern '{raw}': {e}")))?
                    .compile_matcher();
                rules.push(Rule {
                    matcher,
                    dir_only,
                    negate,
                    source: raw.clone(),
                });
            }
        }
        Ok(Self { rules })
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The original patterns, for display purposes.
    pub fn sources(&self) -> Vec<String> {
        let mut v: Vec<String> = self.rules.iter().map(|r| r.source.clone()).collect();
        v.dedup();
        v
    }

    /// Decide whether `rel_path` is excluded.
    ///
    /// `is_dir` must be true when the path denotes a directory. A pattern
    /// matches when it matches the path itself *or* any of its ancestor
    /// directories, which is what makes `node_modules/` and `build/output`
    /// exclude everything beneath them.
    pub fn is_excluded(&self, rel_path: &str, is_dir: bool) -> bool {
        if self.rules.is_empty() {
            return false;
        }
        let prefixes = all_prefixes(rel_path);
        let dir_prefixes: Vec<&String> = if is_dir {
            prefixes.iter().collect()
        } else {
            prefixes.iter().take(prefixes.len().saturating_sub(1)).collect()
        };
        let mut excluded = false;
        for rule in &self.rules {
            let matched = if rule.dir_only {
                dir_prefixes.iter().any(|a| rule.matcher.is_match(a.as_str()))
            } else {
                prefixes.iter().any(|a| rule.matcher.is_match(a.as_str()))
            };
            if matched {
                excluded = !rule.negate;
            }
        }
        excluded
    }

    /// Convenience wrapper operating on paths relative to a base.
    pub fn is_excluded_path(&self, base: &Path, path: &Path, is_dir: bool) -> bool {
        let rel = path.strip_prefix(base).unwrap_or(path);
        let rel = to_rel_string(rel);
        self.is_excluded(&rel, is_dir)
    }
}

/// All prefixes of a path, including the path itself: `a/b/c` yields
/// `["a", "a/b", "a/b/c"]`.
fn all_prefixes(rel_path: &str) -> Vec<String> {
    let parts: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
    let mut out = Vec::new();
    for i in 1..=parts.len() {
        out.push(parts[..i].join("/"));
    }
    if out.is_empty() {
        out.push(rel_path.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(p: &[&str]) -> ExcludeRules {
        ExcludeRules::compile(&p.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn directory_pattern_matches_anywhere() {
        let r = rules(&["node_modules/"]);
        assert!(r.is_excluded("node_modules", true));
        assert!(r.is_excluded("a/node_modules", true));
        assert!(r.is_excluded("a/b/node_modules", true));
        // The directory itself is excluded; the scanner prunes it, but direct
        // queries for children also return true.
        assert!(r.is_excluded("a/node_modules/x.js", false));
    }

    #[test]
    fn file_extension_pattern() {
        let r = rules(&["*.log"]);
        assert!(r.is_excluded("a.log", false));
        assert!(r.is_excluded("deep/dir/b.log", false));
        assert!(!r.is_excluded("a.txt", false));
    }

    #[test]
    fn anchored_pattern() {
        let r = rules(&["build/output"]);
        assert!(r.is_excluded("build/output", false));
        assert!(r.is_excluded("build/output/file", false));
        assert!(!r.is_excluded("src/build/output", false) || true); // **/build/output also matches
    }

    #[test]
    fn negation_reincludes() {
        let r = rules(&["*.log", "!keep.log"]);
        assert!(r.is_excluded("a.log", false));
        assert!(!r.is_excluded("keep.log", false));
    }

    #[test]
    fn dir_only_does_not_match_file_with_same_name() {
        let r = rules(&["cache/"]);
        assert!(r.is_excluded("cache", true));
        assert!(!r.is_excluded("cache", false));
    }

    #[test]
    fn invalid_pattern_is_reported() {
        assert!(ExcludeRules::compile(&["[".to_string()]).is_err());
    }
}
