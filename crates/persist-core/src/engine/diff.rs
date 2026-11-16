//! Snapshot comparison.
//!
//! Comparison is purely content based: two file entries are considered equal
//! when their SHA-256 hashes match, regardless of timestamps. Symlinks compare
//! by target, directories by identity.

use crate::model::{ChangeKind, ChangeSet, EntryKind, FileChange, FileEntry, RootState, Snapshot};
use std::collections::BTreeMap;

type EntryMap = BTreeMap<(String, String), FileEntry>;

fn index_roots(roots: &[RootState]) -> (EntryMap, BTreeMap<String, String>) {
    let mut map = EntryMap::new();
    let mut names = BTreeMap::new();
    for root in roots {
        names.insert(root.root_id.clone(), root.name.clone());
        for entry in &root.entries {
            map.insert((root.root_id.clone(), entry.path.clone()), entry.clone());
        }
    }
    (map, names)
}

fn entries_equal(a: &FileEntry, b: &FileEntry) -> bool {
    if a.kind != b.kind {
        return false;
    }
    match a.kind {
        EntryKind::File => a.hash == b.hash,
        EntryKind::Symlink => a.link_target == b.link_target,
        EntryKind::Directory => true,
    }
}

/// Compare two sets of root states, producing a change set.
///
/// `old` may be empty for an initial backup (everything is "added").
pub fn diff_roots(old: &[RootState], new: &[RootState]) -> ChangeSet {
    let (old_map, old_names) = index_roots(old);
    let (new_map, new_names) = index_roots(new);

    let mut changes = Vec::new();

    for (key, new_entry) in &new_map {
        let (root_id, path) = key;
        let root_name = new_names
            .get(root_id)
            .or_else(|| old_names.get(root_id))
            .cloned()
            .unwrap_or_else(|| root_id.clone());
        match old_map.get(key) {
            None => changes.push(FileChange {
                root_id: root_id.clone(),
                root_name,
                path: path.clone(),
                kind: ChangeKind::Added,
                old: None,
                new: Some(new_entry.clone()),
            }),
            Some(old_entry) => {
                let kind = if entries_equal(old_entry, new_entry) {
                    ChangeKind::Unchanged
                } else {
                    ChangeKind::Modified
                };
                changes.push(FileChange {
                    root_id: root_id.clone(),
                    root_name,
                    path: path.clone(),
                    kind,
                    old: Some(old_entry.clone()),
                    new: Some(new_entry.clone()),
                });
            }
        }
    }

    for (key, old_entry) in &old_map {
        if new_map.contains_key(key) {
            continue;
        }
        let (root_id, path) = key;
        let root_name = old_names
            .get(root_id)
            .cloned()
            .unwrap_or_else(|| root_id.clone());
        changes.push(FileChange {
            root_id: root_id.clone(),
            root_name,
            path: path.clone(),
            kind: ChangeKind::Deleted,
            old: Some(old_entry.clone()),
            new: None,
        });
    }

    changes.sort_by(|a, b| {
        (&a.root_id, &a.path, format!("{:?}", a.kind))
            .cmp(&(&b.root_id, &b.path, format!("{:?}", b.kind)))
    });
    ChangeSet::from_changes(changes)
}

/// Compare two snapshots.
pub fn diff_snapshots(old: Option<&Snapshot>, new: &Snapshot) -> ChangeSet {
    let empty: Vec<RootState> = Vec::new();
    let old_roots = old.map(|s| s.roots.as_slice()).unwrap_or(&empty);
    diff_roots(old_roots, &new.roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hashing;

    fn file(path: &str, content: &str) -> FileEntry {
        FileEntry {
            path: path.into(),
            kind: EntryKind::File,
            size: content.len() as u64,
            hash: Some(hashing::hash_bytes(content.as_bytes())),
            link_target: None,
            mode: 0o644,
            mtime: 0,
        }
    }

    fn root(entries: Vec<FileEntry>) -> RootState {
        RootState {
            root_id: "r".into(),
            name: "r".into(),
            local_path: "/tmp/r".into(),
            remote_path: "r".into(),
            entries,
            exclusions: vec![],
        }
    }

    #[test]
    fn detects_all_change_kinds() {
        let old = vec![root(vec![
            file("same.txt", "a"),
            file("mod.txt", "old"),
            file("gone.txt", "x"),
        ])];
        let new = vec![root(vec![
            file("same.txt", "a"),
            file("mod.txt", "new"),
            file("new.txt", "y"),
        ])];
        let cs = diff_roots(&old, &new);
        assert_eq!(cs.added, 1);
        assert_eq!(cs.modified, 1);
        assert_eq!(cs.deleted, 1);
        assert_eq!(cs.unchanged, 1);
    }

    #[test]
    fn rename_is_add_plus_delete() {
        let old = vec![root(vec![file("a.txt", "same")])];
        let new = vec![root(vec![file("b.txt", "same")])];
        let cs = diff_roots(&old, &new);
        assert_eq!(cs.added, 1);
        assert_eq!(cs.deleted, 1);
        // The content hash is unchanged, so no new object would be uploaded.
    }

    #[test]
    fn initial_backup_all_added() {
        let new = vec![root(vec![file("a", "1"), file("b", "2")])];
        let cs = diff_roots(&[], &new);
        assert_eq!(cs.added, 2);
        assert_eq!(cs.unchanged, 0);
    }
}
