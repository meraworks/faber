//! Results of read calls. A result is kept until the revision changes.

use std::any::Any;
use std::collections::BTreeMap;

/// Kept results of read calls, all for one revision.
#[derive(Default)]
pub(crate) struct ReadCache {
    revision: u64,
    entries: BTreeMap<(&'static str, String), Box<dyn Any + Send>>,
}

impl ReadCache {
    /// Returns the kept result of a read at `revision`. A different revision clears the cache.
    pub(crate) fn get<T: Clone + 'static>(
        &mut self,
        revision: u64,
        tool: &'static str,
        key: &str,
    ) -> Option<T> {
        self.check(revision);
        self.entries
            .get(&(tool, key.to_string()))
            .and_then(|value| value.downcast_ref::<T>())
            .cloned()
    }

    /// Keeps the result of a read at `revision`.
    pub(crate) fn put<T: Send + 'static>(
        &mut self,
        revision: u64,
        tool: &'static str,
        key: String,
        value: T,
    ) {
        self.check(revision);
        self.entries.insert((tool, key), Box::new(value));
    }

    /// Drops all kept results.
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    fn check(&mut self, revision: u64) {
        if self.revision != revision {
            self.entries.clear();
            self.revision = revision;
        }
    }
}
