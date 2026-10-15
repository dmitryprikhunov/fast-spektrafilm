//! Byte-bounded LRU for immutable preparation. In-flight Arc owners are separate
//! from retained cache bytes; oversized entries are used without being retained.
use anyhow::Result;
use std::{collections::VecDeque, path::Path, sync::Arc};
pub(crate) struct ByteCache<T> {
    entries: VecDeque<(String, usize, Arc<T>)>,
    bytes: usize,
    limit: usize,
}
impl<T> ByteCache<T> {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            limit,
        }
    }
    pub fn get(&mut self, key: &str) -> Option<Arc<T>> {
        let index = self.entries.iter().position(|e| e.0 == key)?;
        let entry = self.entries.remove(index).unwrap();
        let value = entry.2.clone();
        self.entries.push_back(entry);
        Some(value)
    }
    pub fn insert(&mut self, key: String, bytes: usize, value: Arc<T>) {
        let bytes = bytes.saturating_add(key.len());
        if bytes > self.limit {
            return;
        }
        if let Some(i) = self.entries.iter().position(|e| e.0 == key) {
            self.bytes -= self.entries.remove(i).unwrap().1;
        }
        while self.bytes + bytes > self.limit {
            self.bytes -= self.entries.pop_front().unwrap().1;
        }
        self.bytes += bytes;
        self.entries.push_back((key, bytes, value));
    }
}
/// File identity includes canonical path, size and nanosecond modification time.
/// Replaced/edited asset files invalidate preparation without relying on cwd.
pub(crate) fn file_key(path: &Path) -> Result<String> {
    let path = path.canonicalize()?;
    let m = path.metadata()?;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}:{}:{}", m.dev(), m.ino(), m.ctime(), m.ctime_nsec())
    };
    #[cfg(not(unix))]
    let identity = format!("{:?}", m.created().ok());
    Ok(format!(
        "{}:{}:{:?}:{identity}",
        path.display(),
        m.len(),
        m.modified()?
    ))
}
pub(crate) fn data_key(path: &Path) -> Result<String> {
    fn walk(path: &Path, output: &mut String) -> Result<()> {
        let mut files = std::fs::read_dir(path)?
            .map(|e| e.map(|v| v.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        files.sort();
        for file in files {
            if file.is_dir() {
                walk(&file, output)?;
            } else {
                output.push_str(&file_key(&file)?);
                output.push('\n');
            }
        }
        Ok(())
    }
    let mut output = String::new();
    walk(path, &mut output)?;
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eviction_is_byte_bounded_and_keeps_live_owners() {
        let mut cache = ByteCache::new(10);
        let live = Arc::new(1);
        cache.insert("a".into(), 6, live.clone());
        cache.insert("b".into(), 4, Arc::new(2));
        assert!(cache.get("a").is_none());
        assert_eq!(*live, 1);
        cache.insert("large".into(), 9, Arc::new(3));
        assert!(cache.get("large").is_none());
        assert_eq!(*cache.get("b").unwrap(), 2);
    }
}
