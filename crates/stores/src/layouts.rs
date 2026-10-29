//! Layout document validation and guarded disk persistence.
use quantick_workspace::layout_document::{LayoutBook, parse};
use std::path::Path;

/// The layout document filename inside the cockpit home.
pub const LAYOUTS_FILE: &str = "layouts.toml";

/// Parse a layouts file, reporting why it is not one. The gate a bundle
/// section goes through — see [`crate::bundle`].
///
/// # Errors
/// Returns the layout parser's reason when the document is invalid.
pub fn validate(text: &str) -> Result<(), String> {
    parse(text).map(|_| ()).map_err(|error| error.to_string())
}

/// What [`load`] found on disk.
#[derive(Debug, PartialEq)]
pub enum Loaded {
    /// No file: a cockpit that has never had layouts, which the caller
    /// migrates or starts fresh.
    Missing,
    /// A file this build reads.
    Book(LayoutBook),
    /// A file this build refuses, with why. Nothing of it is used: half a
    /// book would resurrect half a workspace. The caller starts fresh, and
    /// the refused file is set aside under a `.broken` name so the next save
    /// does not write over the trader's only copy — `set_aside` says whether
    /// that rename succeeded; when it did not, the caller must not save at
    /// all.
    Refused { reason: String, set_aside: bool },
}

/// Read the layouts file.
#[must_use]
pub fn load(path: &Path) -> Loaded {
    let refused = |reason: String| {
        let aside = path.with_extension("toml.broken");
        // A session that writes no store moves no file either; `false` then
        // keeps the caller from saving over the trader's only copy.
        let set_aside = quantick_workspace::write_refusal::guard_write(path).is_ok()
            && std::fs::rename(path, &aside).is_ok();
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LAYOUTS_UNREADABLE",
            path = %path.display(),
            set_aside_to = %aside.display(),
            set_aside,
            reason = %reason,
            action = if set_aside {
                "starting_with_one_layout"
            } else {
                "starting_with_one_layout_and_never_saving_over_the_file"
            },
            "the layouts file could not be read"
        );
        Loaded::Refused { reason, set_aside }
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        // Not "no file": a file that is there and cannot be read — held by
        // another process, denied, or not text. Refused like a file that
        // does not parse, for the same reason: what this session starts on
        // is not what the trader saved, and must never be written over it.
        Err(error) => return refused(error.to_string()),
    };
    match parse(&text) {
        Ok(book) => Loaded::Book(book),
        Err(reason) => refused(reason.to_string()),
    }
}

/// Write the book. Temp sibling + rename, like every other store: a crash
/// mid-write leaves the previous file, never half of the new one.
pub fn save(path: &Path, book: &LayoutBook) {
    if quantick_workspace::write_refusal::guard_write(path).is_err() {
        return;
    }
    match toml::to_string_pretty(book) {
        Ok(text) => {
            let temp = path.with_extension("toml.tmp");
            let written = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path));
            if let Err(error) = written {
                let _ = std::fs::remove_file(&temp);
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LAYOUTS_WRITE_FAILED",
                    path = %path.display(),
                    %error,
                    action = "layouts_not_saved",
                    "could not save the layouts"
                );
            }
        }
        Err(error) => tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LAYOUTS_WRITE_FAILED",
            %error,
            action = "layouts_not_saved",
            "could not encode the layouts"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator_state::{SavedIndicator, SavedInput, SavedKind};
    use quantick_workspace::layout_document::{DrawingKey, SavedBand, SavedDrawing, SavedPoint};

    fn ema() -> SavedIndicator {
        SavedIndicator {
            kind: SavedKind::native("native.ema"),
            hidden: false,
            mouse_vertical_line: false,
            inputs: vec![SavedInput::Int(20), SavedInput::Source("close".to_owned())],
            plot_styles: Vec::new(),
        }
    }

    fn key(pane: usize) -> DrawingKey {
        DrawingKey {
            feed: "binance".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            pane,
        }
    }

    fn level(price: f64) -> SavedDrawing {
        SavedDrawing {
            tool: "horizontal-line".to_owned(),
            name: Some("max".to_owned()),
            author: None,
            points: vec![SavedPoint {
                time_ms: Some(1_000),
                price,
                bar: 3.0,
            }],
            band: SavedBand::Price,
            color: [255, 200, 0, 255],
            width_px: 1.5,
            fill_alpha: 0,
            locked: false,
            hidden: false,
            magnet: false,
            shared: false,
            payload: None,
            text: None,
            id: None,
        }
    }

    #[test]
    fn the_file_round_trips_and_refuses_what_it_cannot_read() {
        let dir = crate::scratch::ScratchDir::new("layouts");
        let path = dir.join(LAYOUTS_FILE);
        assert_eq!(load(&path), Loaded::Missing);

        let mut book = LayoutBook::starter(vec![ema()]);
        let second = book.create(Some("levels")).unwrap();
        book.switch(second).unwrap();
        book.active_mut()
            .replace_drawings(&key(1), vec![level(100.0)], |_| true);
        save(&path, &book);
        assert_eq!(load(&path), Loaded::Book(book.clone()));
        assert!(validate(&std::fs::read_to_string(&path).unwrap()).is_ok());

        std::fs::write(&path, "version = 99\nactive = 1\nnext_id = 2\n").unwrap();
        assert!(matches!(
            load(&path),
            Loaded::Refused { reason, set_aside: true } if reason.contains("version 99")
        ));
        assert!(
            path.with_extension("toml.broken").exists(),
            "the refused file is set aside, never written over"
        );
        assert!(!path.exists());

        std::fs::write(
            &path,
            "version = 1\nactive = 5\nnext_id = 2\n[[layouts]]\nid = 1\nname = \"a\"\n",
        )
        .unwrap();
        assert!(matches!(load(&path), Loaded::Refused { reason, .. } if reason.contains("active")));
        assert!(validate("not = [toml").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
