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
