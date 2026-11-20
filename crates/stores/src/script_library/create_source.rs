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
        if self
            .entries
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(id))
        {
            return Err("A script with that name already exists".into());
        }
        std::fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
        let path = self.directory.join(id);
        let staging = path.with_extension("pine.creating");
        let mut file = std::fs::File::create_new(&staging)
            .map_err(|e| format!("Cannot create temporary script: {e}"))?;
        let written = file
            .write_all(source.as_bytes())
            .and_then(|()| file.sync_all());
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
        assert!(other_writer.create_source("new.pine", "different").is_err());
        assert_eq!(
            ScriptLibraryPort::read(&ScriptLibrary::open(&directory), "new.pine").unwrap(),
            doc
        );
    }
}
