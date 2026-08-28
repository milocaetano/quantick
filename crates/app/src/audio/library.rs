//! The shipped alarm clips.
//!
//! Twenty-seven recordings under `crates/app/assets/alarms/`, embedded in
//! the binary so the sound a preset names exists on every machine the
//! preset is opened on. Two folders, two categories: the *standard* clips
//! behave like alarms (beeps, phones, a cuckoo) and the *nature* clips like
//! a room (rain, surf, a steam train) — long recordings a trader will want
//! cut, which is what [`super::PlayLength`] is for.
//!
//! Adding a clip is a file in one of the folders and a row in [`CLIPS`];
//! the catalogue test refuses a folder and a table that disagree. The
//! token is the file stem, so a hand-edited preset can be checked against
//! the folder by eye.

/// Which folder a clip lives in, which is also how the picker groups it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipCategory {
    /// `assets/alarms/standard/`: clips that behave like an alarm.
    Standard,
    /// `assets/alarms/nature/`: ambient clips, mostly long.
    Nature,
}

/// One recording: its stored token, its picker label, its folder and its
/// bytes as shipped (AAC in an MP4 container — the format the library came
/// in, decoded on play).
#[derive(Debug)]
pub struct Clip {
    pub token: &'static str,
    pub label: &'static str,
    pub category: ClipCategory,
    pub bytes: &'static [u8],
}

/// A row of [`CLIPS`]: the folder, the file stem (which is the token) and
/// the label the picker shows.
macro_rules! clip {
    ($category:ident, $folder:literal, $stem:literal, $label:literal) => {
        Clip {
            token: $stem,
            label: $label,
            category: ClipCategory::$category,
            bytes: include_bytes!(concat!("../../assets/alarms/", $folder, "/", $stem, ".m4a")),
        }
    };
}

/// Every shipped clip, in picker order: the standard folder first, then
/// nature, each alphabetical — the order the trader's own folders listed
/// them in.
pub const CLIPS: &[Clip] = &[
    clip!(Standard, "standard", "american-phone", "American phone"),
    clip!(Standard, "standard", "business-phone", "business phone"),
    clip!(Standard, "standard", "cuckoo", "cuckoo"),
    clip!(Standard, "standard", "english-phone", "English phone"),
    clip!(
        Standard,
        "standard",
        "high-pitched-beep",
        "high-pitched beep"
    ),
    clip!(Standard, "standard", "low-beep", "low beep"),
    clip!(Standard, "standard", "short-beep", "short beep"),
    clip!(Nature, "nature", "aviary", "aviary"),
    clip!(Nature, "nature", "brook", "brook"),
    clip!(Nature, "nature", "city", "city"),
    clip!(Nature, "nature", "dock", "dock"),
    clip!(Nature, "nature", "dockside", "dockside"),
    clip!(Nature, "nature", "ebb-tide", "ebb tide"),
    clip!(Nature, "nature", "everglades", "Everglades"),
    clip!(Nature, "nature", "foghorn", "foghorn"),
    clip!(Nature, "nature", "hail", "hail"),
    clip!(Nature, "nature", "northwoods", "northwoods"),
    clip!(Nature, "nature", "oceanside", "oceanside"),
    clip!(Nature, "nature", "rain", "rain"),
    clip!(Nature, "nature", "rainforest", "rainforest"),
    clip!(Nature, "nature", "steam-train", "steam train"),
    clip!(Nature, "nature", "summer-night", "summer night"),
    clip!(Nature, "nature", "surfs-up", "surf's up"),
    clip!(Nature, "nature", "thunderstorm", "thunderstorm"),
    clip!(Nature, "nature", "white-noise", "white noise"),
    clip!(Nature, "nature", "wind-chimes", "wind chimes"),
    clip!(Nature, "nature", "yosemite-falls", "Yosemite Falls"),
];

/// A position in [`CLIPS`]. An index rather than a reference so an
/// [`super::AlertSound`] stays `Copy` and compares by identity, not by five
/// megabytes of bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipId(usize);

impl ClipId {
    /// Every clip, in the library's order.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..CLIPS.len()).map(Self)
    }

    /// The clip whose file stem this is; `None` for a stem the library has
    /// no row for.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        CLIPS.iter().position(|clip| clip.token == token).map(Self)
    }

    /// The row itself.
    #[must_use]
    pub fn clip(self) -> &'static Clip {
        &CLIPS[self.0]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::*;

    /// The folder and the table agree, both ways: a file nobody listed is
    /// a sound the trader copied in and cannot pick, and a row nobody
    /// shipped would already have failed to compile. Checked per category,
    /// so a clip filed under the wrong folder is a finding too.
    #[test]
    fn the_catalogue_matches_the_assets_folder() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/alarms");
        for (category, folder) in [
            (ClipCategory::Standard, "standard"),
            (ClipCategory::Nature, "nature"),
        ] {
            let on_disk: BTreeSet<String> = std::fs::read_dir(root.join(folder))
                .expect("the category folder exists")
                .map(|entry| entry.expect("readable entry").path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "m4a"))
                .map(|path| {
                    path.file_stem()
                        .expect("a file has a stem")
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            let listed: BTreeSet<String> = CLIPS
                .iter()
                .filter(|clip| clip.category == category)
                .map(|clip| clip.token.to_owned())
                .collect();
            assert_eq!(on_disk, listed, "{folder} folder vs table");
        }
        assert_eq!(CLIPS.len(), 27, "the library the trader supplied");
    }

    /// Tokens are file stems: lower-case, digits and hyphens, so the name
    /// in a preset is the name on disk on every filesystem.
    #[test]
    fn tokens_are_portable_file_stems() {
        for clip in CLIPS {
            assert!(
                !clip.token.is_empty()
                    && clip
                        .token
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{:?} is not a portable stem",
                clip.token
            );
            assert!(!clip.bytes.is_empty(), "{} shipped empty", clip.token);
        }
    }

    /// An id resolves to the row it names and to nothing else.
    #[test]
    fn ids_round_trip_through_tokens() {
        for id in ClipId::all() {
            assert_eq!(ClipId::from_token(id.clip().token), Some(id));
        }
        assert_eq!(ClipId::from_token("missing"), None);
    }
}
