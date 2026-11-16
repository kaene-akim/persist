//! Small helpers used across the crate.

use std::path::{Component, Path, PathBuf};

/// Normalise a relative path expressed with `/` separators, rejecting anything
/// that could escape the repository root (`..`, absolute paths, drive prefixes).
///
/// This is the single choke point used when reading untrusted manifests, so that
/// a malicious or corrupted manifest can never cause a restore to write outside
/// of the destination directory.
pub fn sanitize_rel_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    // Reject absolute POSIX and Windows style paths and NUL bytes outright.
    if path.starts_with('/') || path.starts_with('\\') || path.contains('\0') {
        return None;
    }
    let mut out: Vec<&str> = Vec::new();
    for raw in path.split(['/', '\\']) {
        let part = raw.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        // Reject Windows drive / alternate data stream markers.
        if part.contains(':') {
            return None;
        }
        out.push(part);
    }
    if out.is_empty() {
        return None;
    }
    Some(out.join("/"))
}

/// Join a sanitised relative path onto a base directory, guaranteeing the result
/// stays inside `base`.
pub fn safe_join(base: &Path, rel: &str) -> Option<PathBuf> {
    let clean = sanitize_rel_path(rel)?;
    let joined = base.join(clean.split('/').collect::<PathBuf>());
    // Defence in depth: ensure no component escaped.
    if joined
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return None;
    }
    Some(joined)
}

/// Convert a filesystem path into a `/`-separated relative path string.
pub fn to_rel_string(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(p) => Some(p.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Return a short (12 character) display form of a hex digest.
pub fn short_hash(hash: &str) -> String {
    hash.chars().take(12).collect()
}

/// Human readable byte size, e.g. `1.5 MiB`.
pub fn human_size(bytes: u64) -> String {
    humansize::format_size(bytes, humansize::BINARY)
}

/// Current unix timestamp in seconds.
pub fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Generate a sortable snapshot id: `2026-11-15T23-40-00Z-<6 hex>`.
pub fn new_snapshot_id() -> String {
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ");
    let rand: u32 = rand::random();
    format!("{ts}-{:06x}", rand & 0xff_ffff)
}

/// Generate a random opaque identifier (uuid v4 simple form).
pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_accepts_normal_paths() {
        assert_eq!(sanitize_rel_path("a/b/c.txt").as_deref(), Some("a/b/c.txt"));
        assert_eq!(sanitize_rel_path("./a//b").as_deref(), Some("a/b"));
    }

    #[test]
    fn sanitize_rejects_traversal() {
        assert!(sanitize_rel_path("../etc/passwd").is_none());
        assert!(sanitize_rel_path("a/../../b").is_none());
        assert!(sanitize_rel_path("/etc/passwd").is_none());
        assert!(sanitize_rel_path("C:/windows").is_none());
        assert!(sanitize_rel_path("").is_none());
    }

    #[test]
    fn safe_join_stays_inside() {
        let base = Path::new("/tmp/dest");
        assert_eq!(safe_join(base, "a/b").unwrap(), PathBuf::from("/tmp/dest/a/b"));
        assert!(safe_join(base, "../../etc/passwd").is_none());
    }
}
