//! Repository layout: the mapping from logical concepts to storage paths.
//!
//! Keeping this in one place makes the format easy to document, version and
//! evolve.

/// Repository metadata file.
pub const REPO_META: &str = "repository.json";
/// Pointer to the head snapshot.
pub const HEAD_REF: &str = "refs/head.json";
/// Prefix for snapshot manifests.
pub const SNAPSHOTS_DIR: &str = "snapshots";
/// Prefix for content objects.
pub const OBJECTS_DIR: &str = "objects";
/// Prefix for the sharded object index.
pub const INDEX_DIR: &str = "index/objects";

/// Path of a snapshot manifest.
pub fn snapshot_path(id: &str) -> String {
    format!("{SNAPSHOTS_DIR}/{id}.json")
}

/// Path of a content object, sharded by the first two hex characters so that no
/// single directory holds an unbounded number of children (a real limitation of
/// the Drive API).
pub fn object_path(hash: &str) -> String {
    let shard = hash.get(0..2).unwrap_or("00");
    format!("{OBJECTS_DIR}/{shard}/{hash}")
}

/// The shard key for a hash.
pub fn shard_of(hash: &str) -> String {
    hash.get(0..2).unwrap_or("00").to_string()
}

/// Path of an object index shard.
pub fn index_shard_path(shard: &str) -> String {
    format!("{INDEX_DIR}/{shard}.json")
}

/// Extract a snapshot id from a manifest path, if it matches.
pub fn snapshot_id_from_path(path: &str) -> Option<String> {
    let name = path.strip_prefix(&format!("{SNAPSHOTS_DIR}/"))?;
    name.strip_suffix(".json").map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_paths_are_sharded() {
        let h = "abcdef0123456789";
        assert_eq!(object_path(h), "objects/ab/abcdef0123456789");
        assert_eq!(shard_of(h), "ab");
        assert_eq!(index_shard_path("ab"), "index/objects/ab.json");
    }

    #[test]
    fn snapshot_id_roundtrip() {
        let id = "2026-11-15T00-00-00Z-abc123";
        assert_eq!(snapshot_id_from_path(&snapshot_path(id)).as_deref(), Some(id));
    }
}
