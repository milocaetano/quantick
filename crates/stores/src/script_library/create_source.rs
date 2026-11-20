use super::{
    MAX_SOURCE_BYTES, ScriptDocument, ScriptEntry, ScriptLibrary, ScriptOrigin, script_title,
};
use std::io::Write;

impl ScriptLibrary {
    /// Publish a complete new file without replacing another writer's source.
    pub fn create_source(&mut self, id: &str, source: &str) -> Result<ScriptDocument, String> {
        let stem = id.strip_suffix(".pine").ok_or("Use a .pine file name")?;
        if stem.is_empty()
            || id.len() > 128
            || !stem
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err("Use letters, digits, underscores or hyphens followed by .pine".into());
        }
        if source.len() > MAX_SOURCE_BYTES {
            return Err("Source exceeds 256 KiB".into());
        }
        std::fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
        let path = self.directory.join(id);
        let staging = self
            .directory
            .join(id.to_ascii_lowercase())
            .with_extension("pine.creating");
        let mut file = std::fs::File::create_new(&staging)
            .map_err(|e| format!("Cannot reserve strategy name: {e}"))?;
        let written = (|| -> std::io::Result<()> {
            let collision = || {
                std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "A script with that name already exists",
                )
            };
            if super::EMBEDDED_SCRIPTS
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(id))
            {
                return Err(collision());
            }
            for entry in std::fs::read_dir(&self.directory)? {
                if entry?.file_name().eq_ignore_ascii_case(id) {
                    return Err(collision());
                }
            }
            file.write_all(source.as_bytes())?;
            file.sync_all()
        })();
        drop(file);
        let published = written.and_then(|()| std::fs::hard_link(&staging, &path));
        let _ = std::fs::remove_file(staging);
        published.map_err(|e| format!("Cannot create strategy: {e}"))?;
        self.entries.push(ScriptEntry {
            name: id.into(),
            origin: ScriptOrigin::File(path),
        });
        Ok(ScriptDocument {
            id: id.into(),
            title: script_title(id),
            source: source.into(),
            read_only: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script_library::ScriptLibraryPort;
    #[test]
    fn create_source_exclusively_publishes_exact_bytes() {
        let directory = crate::scratch::ScratchDir::new("create-source");
        let mut store = ScriptLibrary::open(&directory);
        let mut other_writer = ScriptLibrary::open(&directory);
        let doc = store.create_source("new.pine", "exact bytes\r\n").unwrap();
        for id in ["../escape.pine", "rsi.pine", "NEW.pine"] {
            assert!(store.create_source(id, "bad").is_err());
        }
        assert!(other_writer.create_source("NEW.pine", "different").is_err());
        assert_eq!(
            ScriptLibraryPort::read(&ScriptLibrary::open(&directory), "new.pine").unwrap(),
            doc
        );
    }

    #[test]
    fn create_source_reserves_case_variants_across_concurrent_writers() {
        let directory = crate::scratch::ScratchDir::new("create-source-race");
        let barrier = std::sync::Barrier::new(2);
        let winners = std::thread::scope(|scope| {
            let handles =
                [("Mixed.pine", "First"), ("MIXED.pine", "Second")].map(|(id, source)| {
                    let mut store = ScriptLibrary::open(&directory);
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        store.create_source(id, source)
                    })
                });
            handles
                .into_iter()
                .filter_map(|handle| handle.join().unwrap().ok())
                .collect::<Vec<_>>()
        });
        assert_eq!(winners.len(), 1);
        let winner = &winners[0];
        let names: Vec<_> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsStr::new(&winner.id)]);
        assert_eq!(
            ScriptLibraryPort::read(&ScriptLibrary::open(&directory), &winner.id).unwrap(),
            *winner
        );
        assert_eq!(
            std::fs::read(directory.join(&winner.id)).unwrap(),
            winner.source.as_bytes()
        );
    }
}
