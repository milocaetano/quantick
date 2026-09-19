//! The alarm's voice: which sound a cue names, how long it plays, and the
//! catalogue of shipped clips a preset may pick from.
//!
//! A closed vocabulary, on purpose. A preset naming a sound no build owns
//! would be an alarm that plays on one machine and not on the next, so the
//! set is the platform's five scheme sounds plus every row of [`CLIPS`] —
//! a token, a label and a category per clip. The bytes stay with the
//! player that decodes them; the strategy kernel only needs to name a sound
//! and say where it is cut.

use std::time::Duration;

/// One of the sounds a trader may pick.
///
/// A closed set: the platform's five scheme sounds, and every clip in the
/// shipped [`CLIPS`]. Not a path to a file — a preset naming a sound that
/// no build owns would be an alarm that plays on one machine and not on the
/// next. Adding a clip is a file under `assets/alarms/` and a row in
/// [`CLIPS`]; nothing else in the app learns about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlertSound {
    /// The system's "look here" sound. The quiet one, and the default the
    /// annotate tier has always used.
    #[default]
    Information,
    Question,
    /// The system's warning sound — more insistent than information, which
    /// is the point of offering it to an alarm.
    Exclamation,
    /// The system's error sound: the most attention-getting of the set.
    Critical,
    /// The plain default beep.
    Beep,
    /// One of the shipped alarm clips, played by the app itself.
    Clip(ClipId),
}

/// Where a sound comes from, which is also how the picker groups them and
/// which folder of `assets/alarms/` a clip is filed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundCategory {
    /// The operating system's own scheme sounds. Never a clip's category.
    System,
    /// `assets/alarms/standard/`: clips that behave like an alarm — beeps,
    /// phones, a cuckoo.
    Standard,
    /// `assets/alarms/nature/`: ambient clips — rain, surf, a steam train —
    /// mostly long, which is what the cut is for.
    Nature,
}

impl SoundCategory {
    /// Picker order: the sounds the app has always had first, then the
    /// clips that behave like alarms, then the ones that behave like a
    /// room.
    pub const ALL: [Self; 3] = [Self::System, Self::Standard, Self::Nature];

    /// The heading the picker shows over the group.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Standard => "standard alarms",
            Self::Nature => "nature alarms",
        }
    }
}

impl AlertSound {
    /// The platform's sounds, in the order the dialog lists them: quietest
    /// first, so walking the list escalates.
    pub const PLATFORM: [Self; 5] = [
        Self::Information,
        Self::Question,
        Self::Exclamation,
        Self::Critical,
        Self::Beep,
    ];

    /// Every sound a trader may pick, grouped by [`SoundCategory::ALL`]
    /// and, within a group, in the library's own order.
    pub fn all() -> impl Iterator<Item = Self> {
        Self::PLATFORM
            .into_iter()
            .chain(ClipId::all().map(Self::Clip))
    }

    /// The sounds under one heading of the picker.
    pub fn in_category(category: SoundCategory) -> impl Iterator<Item = Self> {
        Self::all().filter(move |sound| sound.category() == category)
    }

    /// Which heading of the picker this sound sits under.
    #[must_use]
    pub fn category(self) -> SoundCategory {
        match self {
            Self::Clip(id) => id.clip().category,
            _ => SoundCategory::System,
        }
    }

    /// Whether a [`PlayLength`] can shorten this sound. The library's clips
    /// are cut wherever the cue says; a platform sound is one beep the
    /// operating system plays whole, and there is nothing to cut.
    #[must_use]
    pub fn can_be_cut(self) -> bool {
        matches!(self, Self::Clip(_))
    }

    /// The name shown in the picker.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Information => "information",
            Self::Question => "question",
            Self::Exclamation => "exclamation",
            Self::Critical => "critical",
            Self::Beep => "default beep",
            Self::Clip(id) => id.clip().label,
        }
    }

    /// The token a preset file stores. Kept separate from [`Self::label`]
    /// so the words on screen can be reworded without silently voiding
    /// every saved preset that named one. A clip's token is its file stem
    /// under `assets/alarms/`, so a hand-edited preset can be checked
    /// against the folder.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Information => "information",
            Self::Question => "question",
            Self::Exclamation => "exclamation",
            Self::Critical => "critical",
            Self::Beep => "beep",
            Self::Clip(id) => id.clip().token,
        }
    }

    /// Read a stored token. `None` for anything this build does not know —
    /// a preset naming a sound that does not exist is refused whole by its
    /// reader, like every other field it cannot honour.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        Self::PLATFORM
            .into_iter()
            .find(|sound| sound.token() == token)
            .or_else(|| ClipId::from_token(token).map(Self::Clip))
    }
}

/// How much of a sound plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayLength {
    /// The sound as recorded, to its end.
    #[default]
    Whole,
    /// Cut at this point, however long the recording is. A cap longer than
    /// the clip changes nothing; a cap of zero is a cue that says nothing,
    /// which is why the preset that stores one has a floor.
    Capped(Duration),
}

impl PlayLength {
    /// The seconds a preset stores, as a length.
    #[must_use]
    pub const fn seconds(secs: u32) -> Self {
        Self::Capped(Duration::from_secs(secs as u64))
    }
}

/// One request to be heard: which sound, and for how long.
///
/// The unit every consumer speaks in and every sink accepts. Small and
/// `Copy` on purpose — an armed instance keeps one, a tab queues a few per
/// frame, and a preset compiles into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cue {
    pub sound: AlertSound,
    pub length: PlayLength,
}

impl Cue {
    /// The one way a stored setting becomes a cue: the sound, cut after
    /// `secs` when there are seconds and the sound can be cut. A platform
    /// beep is whole whatever the setting says — the operating system
    /// plays it in one piece — so two presets naming the same beep with
    /// different cuts compile to the *same* cue, and the per-frame guard
    /// against stacked beeps sees one sound, not two. The preset compiler
    /// and the dialog's Test button both come through here, so what the
    /// trader auditions is what the armed instance will play.
    #[must_use]
    pub fn new(sound: AlertSound, secs: Option<u32>) -> Self {
        match secs {
            Some(secs) if sound.can_be_cut() => Self::cut_after(sound, secs),
            _ => Self::whole(sound),
        }
    }

    /// The sound, whole — what every caller before the length existed
    /// meant.
    #[must_use]
    pub const fn whole(sound: AlertSound) -> Self {
        Self {
            sound,
            length: PlayLength::Whole,
        }
    }

    /// The sound, cut after `secs`. Prefer [`Self::new`], which knows
    /// which sounds a cut applies to.
    #[must_use]
    pub const fn cut_after(sound: AlertSound, secs: u32) -> Self {
        Self {
            sound,
            length: PlayLength::seconds(secs),
        }
    }
}

/// One recording: its stored token, its picker label and its folder. The
/// bytes as shipped (AAC in an MP4 container) are the player's, looked up
/// by the same token.
#[derive(Debug)]
pub struct Clip {
    pub token: &'static str,
    pub label: &'static str,
    /// Never [`SoundCategory::System`]: that heading is the platform's.
    /// The catalogue test holds the line.
    pub category: SoundCategory,
}

/// A row of [`CLIPS`]: the folder (the player finds the bytes by it), the
/// file stem (which is the token) and the label the picker shows.
macro_rules! clip {
    ($category:ident, $folder:literal, $stem:literal, $label:literal) => {
        Clip {
            token: $stem,
            label: $label,
            category: SoundCategory::$category,
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
/// [`AlertSound`] stays `Copy` and compares by identity, not by five
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

    /// The clip's position in [`CLIPS`] — what a table kept in the same
    /// order, such as the player's recordings, is indexed by.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(
                clip.category != SoundCategory::System,
                "a clip is never filed under the platform's own heading"
            );
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
