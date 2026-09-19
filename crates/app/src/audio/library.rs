//! The shipped alarm clips' bytes.
//!
//! The catalogue — which clips exist, their tokens, labels and folders — is
//! `quantick_strategy::sound::CLIPS`, so a preset can name a sound without
//! the window. What only the window needs is the recording itself:
//! twenty-seven files under `crates/app/assets/alarms/`, embedded here so
//! the sound a preset names exists on every machine the preset is opened
//! on, in the catalogue's order. The tests hold the two tables to one
//! another and both to the folders.

use super::ClipId;

/// One shipped recording: the token the catalogue lists it under, and the
/// bytes (AAC in an MP4 container — the format the library came in, decoded
/// on play).
struct Recording {
    token: &'static str,
    bytes: &'static [u8],
}

macro_rules! bytes {
    ($folder:literal, $stem:literal) => {
        Recording {
            token: $stem,
            bytes: include_bytes!(concat!("../../assets/alarms/", $folder, "/", $stem, ".m4a")),
        }
    };
}

/// Every shipped recording, in the catalogue's order.
const RECORDINGS: &[Recording] = &[
    bytes!("standard", "american-phone"),
    bytes!("standard", "business-phone"),
    bytes!("standard", "cuckoo"),
    bytes!("standard", "english-phone"),
    bytes!("standard", "high-pitched-beep"),
    bytes!("standard", "low-beep"),
    bytes!("standard", "short-beep"),
    bytes!("nature", "aviary"),
    bytes!("nature", "brook"),
    bytes!("nature", "city"),
    bytes!("nature", "dock"),
    bytes!("nature", "dockside"),
    bytes!("nature", "ebb-tide"),
    bytes!("nature", "everglades"),
    bytes!("nature", "foghorn"),
    bytes!("nature", "hail"),
    bytes!("nature", "northwoods"),
    bytes!("nature", "oceanside"),
    bytes!("nature", "rain"),
    bytes!("nature", "rainforest"),
    bytes!("nature", "steam-train"),
    bytes!("nature", "summer-night"),
    bytes!("nature", "surfs-up"),
    bytes!("nature", "thunderstorm"),
    bytes!("nature", "white-noise"),
    bytes!("nature", "wind-chimes"),
    bytes!("nature", "yosemite-falls"),
];

/// The bytes of the clip `id` names.
pub(crate) fn bytes(id: ClipId) -> &'static [u8] {
    let recording = &RECORDINGS[id.index()];
    debug_assert_eq!(recording.token, id.clip().token);
    recording.bytes
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::*;
    use quantick_strategy::sound::{CLIPS, SoundCategory};

    /// The catalogue and this table are one list: the same tokens in the
    /// same order, and no recording shipped empty.
    #[test]
    fn the_recordings_follow_the_catalogue_row_for_row() {
        assert_eq!(RECORDINGS.len(), CLIPS.len());
        for (recording, clip) in RECORDINGS.iter().zip(CLIPS) {
            assert_eq!(recording.token, clip.token);
            assert!(!recording.bytes.is_empty(), "{} shipped empty", clip.token);
        }
        for id in ClipId::all() {
            assert!(!bytes(id).is_empty());
        }
    }

    /// The folder and the catalogue agree, both ways: a file nobody listed
    /// is a sound the trader copied in and cannot pick, and a row nobody
    /// shipped would already have failed to compile. Checked per category,
    /// so a clip filed under the wrong folder is a finding too.
    #[test]
    fn the_catalogue_matches_the_assets_folder() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/alarms");
        for (category, folder) in [
            (SoundCategory::Standard, "standard"),
            (SoundCategory::Nature, "nature"),
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
    }
}
