//! Validated, exclusive copy creation and atomic source replacement.
use super::{ScriptDocument, ScriptEntry, ScriptLibrary, ScriptLibraryPort, ScriptOrigin};
use std::{io::Write, path::Path};

const MAX_SOURCE_BYTES: usize = 256 * 1024;

fn validate_id(id: &str) -> Result<(), String> {
    let stem = id
        .strip_suffix(".pine")
        .ok_or("script name must end in .pine")?;
    if stem.is_empty()
        || id.len() > 128
        || !stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("use letters, digits, underscores or hyphens followed by .pine".into());
    }
    Ok(())
}

fn write_source(path: &Path, source: &str, exclusive: bool) -> Result<(), String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err("source exceeds 256 KiB".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if exclusive {
        options.create_new(true);
    } else {
        options.truncate(true);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("cannot write script: {e}"))?;
    let result = file
        .write_all(source.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("cannot persist script: {e}"));
    drop(file);
    if result.is_err() && exclusive {
        let _ = std::fs::remove_file(path);
    }
    result
}

impl ScriptLibrary {
    pub(super) fn copy_document(
        &mut self,
        id: &str,
        new_id: &str,
    ) -> Result<ScriptDocument, String> {
        validate_id(new_id)?;
        if self
            .entries
            .iter()
            .any(|e| e.name.eq_ignore_ascii_case(new_id))
        {
            return Err("a script with that name already exists".into());
        }
        let original = ScriptLibraryPort::read(self, id)?;
        std::fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
        let path = self.directory.join(new_id);
        write_source(&path, &original.source, true)?;
        self.entries.push(ScriptEntry {
            name: new_id.into(),
            origin: ScriptOrigin::File(path),
        });
        ScriptLibraryPort::read(self, new_id)
    }

    pub(super) fn save_document(&mut self, document: &ScriptDocument) -> Result<(), String> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.name == document.id)
            .ok_or("unknown script")?;
        let ScriptOrigin::File(path) = &entry.origin else {
            return Err("built-in scripts are read-only; create an editable copy".into());
        };
        if document.read_only
            || path.parent() != Some(self.directory.as_path())
            || std::fs::symlink_metadata(path)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("script is not an editable library file".into());
        }
        // The temporary file is exclusive: never follow an existing link or overwrite a
        // concurrent writer's temporary data. Rename leaves the old source on failure.
        let temporary = path.with_extension("pine.saving");
        write_source(&temporary, &document.source, true)?;
        let result =
            std::fs::rename(&temporary, path).map_err(|e| format!("cannot replace script: {e}"));
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn script_copy_preserves_builtin() {
        let directory = crate::scratch::ScratchDir::new("copy");
        let mut library = ScriptLibrary::open(&directory);
        let original = ScriptLibraryPort::read(&library, "rsi.pine").unwrap();
        assert!(library.save(&original).is_err());
        let mut copy = library.copy("rsi.pine", "my-rsi.pine").unwrap();
        assert!(!copy.read_only);
        assert_ne!(copy.id, original.id);
        copy.source = "plot(close)".into();
        library.save(&copy).unwrap();
        assert_eq!(
            ScriptLibraryPort::read(&library, "rsi.pine").unwrap(),
            original
        );
        let reopened = ScriptLibrary::open(&directory);
        assert_eq!(ScriptLibraryPort::read(&reopened, &copy.id).unwrap(), copy);
        for invalid in [
            "my-rsi.pine",
            "MY-RSI.pine",
            "../escape.pine",
            "C:/escape.pine",
            "bad.pine/next.pine",
        ] {
            assert!(library.copy("rsi.pine", invalid).is_err(), "{invalid}");
        }
    }
}
