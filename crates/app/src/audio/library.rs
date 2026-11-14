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
mod tests;
