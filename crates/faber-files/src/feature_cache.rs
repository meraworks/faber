//! Persistent feature cache in the project folder.

use faber_ports::{FeatureCache, Result};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

/// Format marker. Another marker means another format, and the entry is ignored.
const MAGIC: &[u8; 4] = b"FBC1";
/// Marker, SHA-256 of the payload, payload length.
const HEADER: usize = 4 + 32 + 8;
/// Largest total size of the cache folder, in bytes. A write above it removes the oldest entries.
pub const LIMIT: u64 = 512 * 1024 * 1024;
/// Percentage of the limit that stays after pruning.
const KEEP_PERCENT: u64 = 80;

pub struct FileFeatureCache {
    root: PathBuf,
    read_only: bool,
    limit: u64,
    /// Entries that this instance wrote. Pruning never removes them.
    written: Mutex<HashSet<PathBuf>>,
    /// Bytes of all entries. The folder is listed at open and when pruning.
    total: Mutex<u64>,
    #[cfg(test)]
    listings: AtomicUsize,
}

impl FileFeatureCache {
    pub fn new(root: &Path) -> Self {
        Self::with_limit(root, LIMIT)
    }

    pub fn read_only(root: &Path) -> Self {
        Self::open(root, LIMIT, true)
    }

    pub fn with_limit(root: &Path, limit: u64) -> Self {
        Self::open(root, limit, false)
    }

    fn open(root: &Path, limit: u64, read_only: bool) -> Self {
        let cache = Self {
            read_only,
            root: root.into(),
            limit,
            written: Mutex::new(HashSet::new()),
            total: Mutex::new(0),
            #[cfg(test)]
            listings: AtomicUsize::new(0),
        };
        if read_only {
            return cache;
        }
        let total: u64 = cache.scan().iter().map(|file| file.1).sum();
        if let Ok(mut current) = cache.total.lock() {
            *current = total;
        }
        cache
    }

    fn folder(&self) -> PathBuf {
        self.root.join(".faber").join("cache")
    }

    /// Lists the entries with their modified time and size.
    fn scan(&self) -> Vec<(SystemTime, u64, PathBuf)> {
        #[cfg(test)]
        self.listings.fetch_add(1, Ordering::Relaxed);
        let Ok(entries) = std::fs::read_dir(self.folder()) else {
            return Vec::new();
        };
        entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension()? != "bin" {
                    return None;
                }
                let metadata = entry.metadata().ok()?;
                Some((metadata.modified().ok()?, metadata.len(), path))
            })
            .collect()
    }

    /// Removes the oldest entries, not written by this instance, until the total is at most 80 % of the limit.
    fn prune(&self) {
        let mut files = self.scan();
        let mut total: u64 = files.iter().map(|file| file.1).sum();
        if total <= self.limit {
            self.set_total(total);
            return;
        }
        let target = (u128::from(self.limit) * u128::from(KEEP_PERCENT) / 100) as u64;
        let Ok(written) = self.written.lock() else {
            return;
        };
        files.sort();
        for (_, size, path) in files {
            if total <= target {
                break;
            }
            if written.contains(&path) {
                continue;
            }
            // A failed delete keeps the entry and its size in the total.
            if std::fs::remove_file(&path).is_ok() {
                total -= size;
            }
        }
        self.set_total(total);
    }

    fn set_total(&self, value: u64) {
        if let Ok(mut total) = self.total.lock() {
            *total = value;
        }
    }

    /// Number of folder listings made by this instance. Used by the tests.
    #[cfg(test)]
    fn folder_listings(&self) -> usize {
        self.listings.load(Ordering::Relaxed)
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        let valid =
            !key.is_empty() && key.len() <= 128 && key.bytes().all(|c| c.is_ascii_alphanumeric());
        valid.then(|| {
            self.root
                .join(".faber")
                .join("cache")
                .join(format!("{key}.bin"))
        })
    }
}

impl FeatureCache for FileFeatureCache {
    fn writable(&self) -> bool {
        !self.read_only
    }

    fn get(&self, key: &str) -> Option<Vec<u8>> {
        let bytes = std::fs::read(self.path(key)?).ok()?;
        if bytes.len() < HEADER || &bytes[..4] != MAGIC {
            return None;
        }
        let digest = &bytes[4..36];
        let length = u64::from_le_bytes(bytes[36..HEADER].try_into().ok()?);
        let payload = &bytes[HEADER..];
        if payload.len() as u64 != length || Sha256::digest(payload).as_slice() != digest {
            return None;
        }
        Some(payload.to_vec())
    }

    fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
        if self.read_only {
            return Err("The preview can only read saved features.".into());
        }
        let path = self.path(key).ok_or("Invalid cache key")?;
        let parent = path.parent().ok_or("Cache entry has no folder")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        file.write_all(MAGIC).map_err(|e| e.to_string())?;
        file.write_all(Sha256::digest(bytes).as_slice())
            .map_err(|e| e.to_string())?;
        file.write_all(&(bytes.len() as u64).to_le_bytes())
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        let replaced = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        file.persist(&path)
            .map_err(|e| format!("Cannot save cache entry: {e}"))?;
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let total = {
            let mut total = self.total.lock().unwrap_or_else(|p| p.into_inner());
            *total = total.saturating_sub(replaced).saturating_add(size);
            *total
        };
        if let Ok(mut written) = self.written.lock() {
            written.insert(path);
        }
        if total > self.limit {
            self.prune();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};

    const PAYLOAD: usize = 100;
    /// Size of one stored entry.
    const ENTRY: u64 = (HEADER + PAYLOAD) as u64;

    fn entry_path(root: &Path, key: &str) -> PathBuf {
        root.join(".faber").join("cache").join(format!("{key}.bin"))
    }

    #[test]
    fn a_preview_reads_saved_features_without_writing_cache_entries() {
        let dir = tempfile::tempdir().unwrap();
        let editor = FileFeatureCache::new(dir.path());
        editor.put("existing", b"shape").unwrap();
        let preview = FileFeatureCache::read_only(dir.path());
        assert_eq!(
            preview.get("existing").as_deref(),
            Some(b"shape".as_slice())
        );
        assert!(preview.put("missing", b"other").is_err());
        assert!(editor.get("missing").is_none());
    }

    #[test]
    fn oldest_entries_go_first_above_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        // An earlier session wrote four entries. Their modified times set the order.
        let earlier = FileFeatureCache::new(dir.path());
        for (index, key) in ["k1", "k2", "k3", "k4"].into_iter().enumerate() {
            earlier.put(key, &[0u8; PAYLOAD]).unwrap();
            let file = fs::OpenOptions::new()
                .write(true)
                .open(entry_path(dir.path(), key))
                .unwrap();
            let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1000 + index as u64);
            file.set_modified(time).unwrap();
        }
        // The limit holds four entries. The fifth write goes above it.
        let cache = FileFeatureCache::with_limit(dir.path(), 4 * ENTRY);
        cache.put("k5", &[0u8; PAYLOAD]).unwrap();
        // Pruning stops at 80 % of the limit. The two oldest entries go.
        assert!(!entry_path(dir.path(), "k1").exists(), "k1 was kept");
        assert!(!entry_path(dir.path(), "k2").exists(), "k2 was kept");
        for key in ["k3", "k4", "k5"] {
            assert!(entry_path(dir.path(), key).exists(), "{key} was deleted");
        }
    }

    #[test]
    fn put_does_not_list_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileFeatureCache::new(dir.path());
        for index in 0..100 {
            cache.put(&format!("k{index}"), &[0u8; PAYLOAD]).unwrap();
        }
        // Opening lists the folder once. Writes keep a running total and do not list it.
        assert!(cache.folder_listings() <= 1, "{}", cache.folder_listings());
    }

    #[test]
    fn entries_of_the_current_build_stay() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileFeatureCache::with_limit(dir.path(), 2 * ENTRY);
        for key in ["k1", "k2", "k3", "k4"] {
            cache.put(key, &[0u8; PAYLOAD]).unwrap();
        }
        for key in ["k1", "k2", "k3", "k4"] {
            assert!(entry_path(dir.path(), key).exists(), "{key} was deleted");
        }
    }
}
