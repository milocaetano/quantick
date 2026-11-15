//! Every workspace's colour: one of sixteen presets or a custom `#RRGGBB`,
//! stored with the workspace and never derived from where it sits in a list.
//!
//! Uniqueness is the store's invariant, kept by [`settle`] after every change
//! to the set of workspaces: no two workspaces hold identical colours (the
//! same value in either theme), and a workspace without one takes the next
//! free colour in the book's own order (`Default` first, then by name):
//! `Default` Slate, every other the lowest free preset, and past sixteen a
//! generated custom. So a new workspace takes the next free colour, a deleted
//! one frees its own without recolouring any other, and a stored book that
//! predates colours gets them deterministically on load.
//!
//! [`WorkspaceColour::rgb`] is the one resolver every surface paints from.
use std::collections::BTreeMap;

/// A straight sRGB colour.
pub type Rgb = [u8; 3];

/// One preset: its name on screen and the value it paints in each theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub name: &'static str,
    pub light: Rgb,
    pub dark: Rgb,
}

const fn rgb(value: u32) -> Rgb {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

const fn preset(name: &'static str, light: u32, dark: u32) -> Preset {
    Preset {
        name,
        light: rgb(light),
        dark: rgb(dark),
    }
}

/// The palette. A file stores an index, so the order never changes: no
/// green, red, gold or orange, which already mean buy, sell, the POC and
/// unsaved edits.
pub const PRESETS: [Preset; 16] = [
    preset("Cobalt", 0x2F5B_D8, 0x7393_F5),
    preset("Orchid", 0xA63D_A0, 0xDA7B_D4),
    preset("Sky", 0x1F86_BF, 0x5DBD_EB),
    preset("Violet", 0x6A3F_C4, 0xA887_F2),
    preset("Slate", 0x5660_6E, 0xA3AD_BA),
    preset("Fuchsia", 0xC02B_86, 0xEF6D_B7),
    preset("Petrol", 0x145F_78, 0x4FA6_C2),
    preset("Lavender", 0x8C7C_D0, 0xC6BB_F6),
    preset("Ultramarine", 0x2C2F_9E, 0x8085_F2),
    preset("Mauve", 0x8A6A_8E, 0xC3A6_C7),
    preset("Denim", 0x4A6F_A5, 0x90AE_DD),
    preset("Grape", 0x6E24_96, 0xBE78_E6),
    preset("Ink", 0x252B_36, 0xE3E7_EE),
    preset("Plum", 0x6B2A_5E, 0xB86F_A6),
    preset("Ice", 0x5E9F_D0, 0xB3DB_F5),
    preset("Blossom", 0xCF6F_B6, 0xF4B3_DD),
];

/// `Default`'s colour while it is free: Slate, the steel grey-blue.
pub const DEFAULT_PRESET: u8 = 4;

/// #978's eight words, each to its nearest preset over both themes, one to one.
const LEGACY: [(&str, u8); 8] = [
    ("steel", 4),
    ("violet", 11),
    ("cyan", 2),
    ("magenta", 1),
    ("azure", 0),
    ("indigo", 3),
    ("chalk", 12),
    ("cobalt", 8),
];

/// A workspace's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorkspaceColour {
    /// An index into [`PRESETS`]: its Light value in Light, its Dark in Dark.
    Preset(u8),
    /// One value in both themes.
    Custom(Rgb),
}

impl WorkspaceColour {
    /// The preset at `index`, if the palette has one.
    #[must_use]
    pub const fn preset(index: u8) -> Option<Self> {
        if (index as usize) < PRESETS.len() {
            Some(Self::Preset(index))
        } else {
            None
        }
    }

    /// The value painted in the dark or the light theme: the one resolver.
    #[must_use]
    pub const fn rgb(self, dark: bool) -> Rgb {
        match self {
            Self::Preset(index) if dark => PRESETS[index as usize].dark,
            Self::Preset(index) => PRESETS[index as usize].light,
            Self::Custom(rgb) => rgb,
        }
    }

    /// The name on screen: the preset's, or `Custom`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Preset(index) => PRESETS[index as usize].name,
            Self::Custom(_) => "Custom",
        }
    }

    /// The word a call and the journal use: `sky`, or `#3A7BD5`.
    #[must_use]
    pub fn key(self) -> String {
        match self {
            Self::Preset(_) => self.name().to_lowercase(),
            Self::Custom(rgb) => hex(rgb),
        }
    }

    /// The name and the value painted in a theme: `Sky #5DBDEB`.
    #[must_use]
    pub fn describe(self, dark: bool) -> String {
        format!("{} {}", self.name(), hex(self.rgb(dark)))
    }

    /// A preset's name in any case, or `#RRGGBB` in any case.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        if let Some(rgb) = parse_hex(word) {
            return Some(Self::Custom(rgb));
        }
        let found = PRESETS.iter().position(|p| p.name.eq_ignore_ascii_case(word));
        found.and_then(|index| Self::preset(index as u8))
    }

    /// One of #978's eight words, through its fixed table.
    #[must_use]
    pub fn legacy(word: &str) -> Option<Self> {
        let found = LEGACY.iter().find(|(old, _)| *old == word);
        found.and_then(|(_, index)| Self::preset(*index))
    }

    /// The same value in either theme: two workspaces never hold these.
    #[must_use]
    pub fn identical(self, other: Self) -> bool {
        self.rgb(false) == other.rgb(false) || self.rgb(true) == other.rgb(true)
    }
}

/// `#RRGGBB`, upper case.
#[must_use]
pub fn hex([r, g, b]: Rgb) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// `#RRGGBB` in any case.
#[must_use]
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// Who holds which colour, by workspace name.
pub type Colours = BTreeMap<String, WorkspaceColour>;

fn free(held: &[WorkspaceColour], colour: WorkspaceColour) -> bool {
    !held.iter().any(|other| other.identical(colour))
}

/// The colour a workspace takes beside `held`: Slate for `Default` while it
/// is free, else the lowest free preset, else [`generated`].
#[must_use]
pub fn next_free(held: &[WorkspaceColour], default: bool) -> WorkspaceColour {
    let slate = WorkspaceColour::Preset(DEFAULT_PRESET);
    if default && free(held, slate) {
        return slate;
    }
    let presets = (0..PRESETS.len() as u8).map(WorkspaceColour::Preset);
    presets
        .into_iter()
        .find(|colour| free(held, *colour))
        .unwrap_or_else(|| generated(held))
}

/// How much each round past the first lifts the candidates' OKLab lightness.
const ROUND_LIGHTNESS: f64 = 0.04;
/// Rounds of lifted candidates before every value is walked in order.
const ROUNDS: u32 = 25;

/// A custom colour while all sixteen presets are held: the OKLab midpoint
/// of a preset's Light and Dark values, preset by preset, the first not
/// identical to one in `held`; every candidate taken, the next round lifts
/// their lightness by [`ROUND_LIGHTNESS`], clamped. Deterministic.
#[must_use]
pub fn generated(held: &[WorkspaceColour]) -> WorkspaceColour {
    for round in 0..=ROUNDS {
        for preset in &PRESETS {
            let [l1, a1, b1] = oklab(preset.light);
            let [l2, a2, b2] = oklab(preset.dark);
            let lift = f64::from(round) * ROUND_LIGHTNESS;
            let lightness = ((l1 + l2) / 2.0 + lift).clamp(0.0, 1.0);
            let candidate = WorkspaceColour::Custom(srgb([lightness, (a1 + a2) / 2.0, (b1 + b2) / 2.0]));
            if free(held, candidate) {
                return candidate;
            }
        }
    }
    let every = (0..1_u32 << 24).map(|value| WorkspaceColour::Custom(rgb(value)));
    every
        .into_iter()
        .find(|candidate| free(held, *candidate))
        .unwrap_or(WorkspaceColour::Preset(DEFAULT_PRESET))
}

/// OKLab (Björn Ottosson) of a straight sRGB colour.
fn oklab(rgb: Rgb) -> [f64; 3] {
    let linear = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b] = rgb.map(linear);
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

/// The straight sRGB colour of an OKLab one, each channel clamped and rounded.
fn srgb([lightness, a, b]: [f64; 3]) -> Rgb {
    let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    let linear = [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    ];
    linear.map(|c| {
        let c = c.clamp(0.0, 1.0);
        let gamma = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (gamma * 255.0).round() as u8
    })
}

/// Restore the invariant over `order`, the workspaces in the book's order
/// with `Default` first: a name no longer there frees its colour, a colour
/// held twice stays with its first holder, and every workspace without one
/// takes [`next_free`].
pub fn settle(colours: &mut Colours, order: &[&str]) {
    colours.retain(|name, _| order.contains(&name.as_str()));
    let mut held = Vec::new();
    for name in order {
        match colours.get(*name) {
            Some(colour) if free(&held, *colour) => held.push(*colour),
            _ => {
                colours.remove(*name);
            }
        }
    }
    for (place, name) in order.iter().enumerate() {
        if colours.contains_key(*name) {
            continue;
        }
        let colour = next_free(&held, place == 0);
        held.push(colour);
        colours.insert((*name).to_owned(), colour);
    }
}

/// What picking a colour did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColourOutcome {
    pub workspace: String,
    pub colour: WorkspaceColour,
    /// `false` when the workspace already held it.
    pub changed: bool,
    /// The workspace that held the colour, which swapped.
    pub swapped_with: Option<String>,
    /// What [`Self::swapped_with`] holds now: the picker's old colour.
    pub holder_colour: Option<WorkspaceColour>,
}

/// A colour another workspace holds, picked without `swap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColourTaken {
    pub owner: String,
    /// What the owner would take in a swap: the picker's colour.
    pub owner_would_get: WorkspaceColour,
}

/// Give `name` the colour `colour`. A workspace holding an identical one is
/// refused unless `swap`, then takes `name`'s old colour, so no workspace is
/// ever left without one. The caller has checked that `name` is a workspace.
///
/// # Errors
/// Another workspace holds the colour and `swap` is false.
pub fn pick(
    colours: &mut Colours,
    name: &str,
    colour: WorkspaceColour,
    swap: bool,
) -> Result<ColourOutcome, ColourTaken> {
    let previous = colours.get(name).copied();
    let holder = colours
        .iter()
        .find(|(held, c)| held.as_str() != name && c.identical(colour))
        .map(|(held, _)| held.clone());
    if let Some(owner) = holder.clone() {
        let mut held: Vec<WorkspaceColour> = colours.values().copied().collect();
        held.push(colour);
        let owner_would_get = previous.unwrap_or_else(|| next_free(&held, false));
        if !swap {
            return Err(ColourTaken {
                owner,
                owner_would_get,
            });
        }
        colours.insert(owner, owner_would_get);
    }
    colours.insert(name.to_owned(), colour);
    Ok(ColourOutcome {
        workspace: name.to_owned(),
        colour,
        changed: previous != Some(colour),
        holder_colour: holder.as_ref().and_then(|owner| colours.get(owner).copied()),
        swapped_with: holder,
    })
}

/// The most characters a held swatch's initials take.
pub const INITIALS_MAX: usize = 4;

/// The shortest start of `name`, up to [`INITIALS_MAX`] characters, no other
/// of `among` shares, case aside: `Scalp` and `Swing` are `Sc` and `Sw`,
/// `Default` is `D`. Names alike that far take one character less and their
/// place among the names alike, in `among`'s order: `Scalp` and `Scalper`
/// are `Sca1` and `Sca2`.
#[must_use]
pub fn initials(name: &str, among: &[&str]) -> String {
    let start = |text: &str, letters: usize| -> String { text.chars().take(letters).collect() };
    let alike = |other: &str, letters: usize| {
        start(other, letters).to_lowercase() == start(name, letters).to_lowercase()
    };
    let unique = |letters: usize| !among.iter().any(|o| *o != name && alike(o, letters));
    if let Some(letters) = (1..=INITIALS_MAX).find(|letters| unique(*letters)) {
        return start(name, letters);
    }
    let mut twins = among.iter().filter(|other| alike(other, INITIALS_MAX));
    let place = twins
        .position(|other| *other == name)
        .map_or(1, |at| at + 1);
    format!("{}{place}", start(name, INITIALS_MAX - 1))
}

#[cfg(test)]
#[path = "workspace_colour_tests.rs"]
mod tests;
