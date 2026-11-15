//! The palette a workspace's colour is chosen from, apart from its pixels:
//! the sixteen presets and the last custom colour as swatches, which one is
//! current and who holds the others; what a pick does (applies, or asks to
//! swap, in the question's words); and the Custom picker's warnings, which
//! never block. The menu's header popover draws it today; Manage and the
//! create dialogs reuse it unchanged.
//!
//! The warnings need the book and the bundled themes' trade and strip
//! colours, and this is the lowest crate that sees both.

use quantick_appearance::contrast::{contrast_ratio, oklab, oklab_distance};
use quantick_appearance::{PremultipliedRgba, ResolvedTheme, Token};
use quantick_workspace::workspace_colour::{
    self, ColourTaken, Colours, PRESETS, Rgb, WorkspaceColour, hex, initials,
};

use super::WorkspaceBook;

/// The seventeenth swatch's key: the last custom colour.
pub const LAST_CUSTOM_KEY: &str = "last_custom";
/// What a malformed Custom field says under it, Apply disabled.
pub const HEX_HINT: &str = "Use #RRGGBB, for example #3A7BD5";
/// Between a swatch's name and what it says of itself.
const SEPARATOR: &str = " \u{b7} ";

/// One colour to pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Swatch {
    /// The word a call and the scene name it by: `sky`, or [`LAST_CUSTOM_KEY`].
    pub key: String,
    pub colour: WorkspaceColour,
    /// The words under it: a preset's name, or the custom `#RRGGBB`.
    pub name: String,
    /// The workspace's own colour.
    pub current: bool,
    /// The workspace holding it, if another does.
    pub holder: Option<String>,
    /// [`Self::holder`]'s initials, as the swatch shows them.
    pub initials: Option<String>,
}

impl Swatch {
    /// What hovering it says: `Sky`, `Sky · current`, `Sky · used by Scalp`,
    /// `Last custom #3A7BD5`.
    #[must_use]
    pub fn tip(&self) -> String {
        let name = if self.key == LAST_CUSTOM_KEY {
            format!("Last custom {}", self.name)
        } else {
            self.name.clone()
        };
        match (&self.holder, self.current) {
            (_, true) => format!("{name}{SEPARATOR}current"),
            (Some(owner), false) => format!("{name}{SEPARATOR}used by {owner}"),
            (None, false) => name,
        }
    }
}

/// The palette as `book` stands for workspace `workspace`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub workspace: String,
    pub current: WorkspaceColour,
    /// The sixteen presets, in the palette's order.
    pub presets: Vec<Swatch>,
    /// The seventeenth swatch, once a custom colour was applied.
    pub last_custom: Option<Swatch>,
    /// The colour a new workspace would take now.
    pub next_free: WorkspaceColour,
    colours: Colours,
}

/// A colour's name in a sentence: `Sky`, or `Custom #3A7BD5`.
#[must_use]
pub fn said(colour: WorkspaceColour) -> String {
    match colour {
        WorkspaceColour::Preset(_) => colour.name().to_owned(),
        WorkspaceColour::Custom(rgb) => format!("Custom {}", hex(rgb)),
    }
}

/// The palette for workspace `name`; `None` for a name the book lacks.
#[must_use]
pub fn palette(book: &WorkspaceBook, name: &str) -> Option<Palette> {
    let current = book.colour(name)?;
    let listed = book.list();
    let names: Vec<&str> = listed.iter().map(|row| row.name.as_str()).collect();
    let swatch = |key: String, colour: WorkspaceColour, label: String| {
        let holder = (listed.iter())
            .find(|row| row.name != name && row.colour.identical(colour))
            .map(|row| row.name.clone());
        Swatch {
            key,
            colour,
            name: label,
            current: colour == current,
            initials: holder.as_deref().map(|owner| initials(owner, &names)),
            holder,
        }
    };
    let presets = (0..PRESETS.len() as u8)
        .filter_map(WorkspaceColour::preset)
        .map(|colour| swatch(colour.key(), colour, colour.name().to_owned()))
        .collect();
    let last_custom = book.last_custom().map(|rgb| {
        swatch(
            LAST_CUSTOM_KEY.to_owned(),
            WorkspaceColour::Custom(rgb),
            hex(rgb),
        )
    });
    let held: Vec<WorkspaceColour> = listed.iter().map(|row| row.colour).collect();
    Some(Palette {
        workspace: name.to_owned(),
        current,
        presets,
        last_custom,
        next_free: workspace_colour::next_free(&held, false),
        colours: listed
            .into_iter()
            .map(|row| (row.name, row.colour))
            .collect(),
    })
}

/// What picking a colour does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Free, or the workspace's own: set at once.
    Apply(WorkspaceColour),
    /// One other workspace holds it: the question replaces the grid.
    Ask(SwapQuestion),
    /// No swap can free it; the reason.
    Refused(String),
}

/// The question a held colour asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapQuestion {
    pub colour: WorkspaceColour,
    pub owner: String,
    /// What the owner holds after a swap: the workspace's old colour.
    pub owner_gets: WorkspaceColour,
    /// `Sky is used by Scalp. Swap? Scalp becomes Violet.`
    pub words: String,
}

impl Palette {
    /// Every swatch in focus order: the presets, then the seventeenth.
    pub fn swatches(&self) -> impl Iterator<Item = &Swatch> {
        self.presets.iter().chain(&self.last_custom)
    }

    /// The swatch a call's or a press's word names: a preset's key or
    /// [`LAST_CUSTOM_KEY`].
    #[must_use]
    pub fn swatch(&self, key: &str) -> Option<&Swatch> {
        self.swatches().find(|swatch| swatch.key == key)
    }

    /// What picking `colour` does; `typed` when it came from the Custom
    /// field, whose value equal to a preset says which preset it is.
    #[must_use]
    pub fn pick(&self, colour: WorkspaceColour, typed: bool) -> Pick {
        let mut trial = self.colours.clone();
        match workspace_colour::pick(&mut trial, &self.workspace, colour, false) {
            Ok(_) => Pick::Apply(colour),
            Err(ColourTaken::Held {
                owner,
                owner_would_get,
            }) => {
                let held = self.colours.get(&owner).copied().unwrap_or(colour);
                // A typed value equal to a preset says which preset it is.
                let subject = match (typed, colour, held) {
                    (true, WorkspaceColour::Custom(rgb), WorkspaceColour::Preset(_)) => {
                        format!("{} is {},", hex(rgb), held.name())
                    }
                    _ => format!("{} is", said(colour)),
                };
                let gets = said(owner_would_get);
                Pick::Ask(SwapQuestion {
                    colour,
                    words: format!("{subject} used by {owner}. Swap? {owner} becomes {gets}."),
                    owner,
                    owner_gets: owner_would_get,
                })
            }
            Err(taken) => Pick::Refused(super::WorkspaceRefusal::ColourTaken(taken).to_string()),
        }
    }
}

/// Below this OKLab distance in either theme a colour is hard to tell from
/// another workspace's: the palette's tightest pair is 0.036 apart.
pub const CLOSE_DELTA_E: f32 = 0.03;
/// Below this OKLab distance a colour reads as a buy or sell colour.
pub const NEAR_TRADE_DELTA_E: f32 = 0.06;
/// A trade colour with less OKLCh chroma than this is no hue (Light's black
/// `sell`), so nothing is near it.
pub const TRADE_MIN_CHROMA: f32 = 0.05;
/// Below this contrast against a theme's tab strip a colour is lost there:
/// #978's "lost on the strip" number.
pub const LOW_STRIP_CONTRAST: f32 = 1.5;

/// Whether two colours `distance` apart are hard to tell apart.
#[must_use]
pub fn is_close(distance: f32) -> bool {
    distance < CLOSE_DELTA_E
}

/// Whether a colour `distance` from a trade colour reads as one.
#[must_use]
pub fn is_near_trade(distance: f32) -> bool {
    distance < NEAR_TRADE_DELTA_E
}

/// Whether a colour at `ratio` on a tab strip is lost there.
#[must_use]
pub fn is_low_contrast(ratio: f32) -> bool {
    ratio < LOW_STRIP_CONTRAST
}

/// One warning the Custom picker shows under its field. None blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColourWarning {
    /// Hard to tell from `workspace`'s colour, named in `colour`.
    Close {
        workspace: String,
        colour: String,
    },
    NearBuy,
    NearSell,
    /// Lost on the Dark (`dark`) or the Light tab strip.
    LowContrast {
        dark: bool,
    },
}

impl ColourWarning {
    /// The line under the field.
    #[must_use]
    pub fn words(&self) -> String {
        match self {
            Self::Close { workspace, colour } => {
                format!("Hard to tell from {workspace}'s colour ({colour}).")
            }
            Self::NearBuy => "Close to the buy green.".to_owned(),
            Self::NearSell => "Close to the sell red.".to_owned(),
            Self::LowContrast { dark } => {
                let theme = if *dark { "Dark" } else { "Light" };
                format!("Low contrast on the {theme} tab strip.")
            }
        }
    }
}

fn ink([r, g, b]: Rgb) -> PremultipliedRgba {
    PremultipliedRgba::from_rgb(r, g, b)
}

/// A bundled theme's `buy` or `sell` colour that is a hue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TradeInk {
    pub dark: bool,
    pub buy: bool,
    pub ink: PremultipliedRgba,
}

/// The `buy` and `sell` colours of both bundled themes that are hues.
#[must_use]
pub fn trade_inks() -> Vec<TradeInk> {
    let mut found = Vec::new();
    for dark in [true, false] {
        let theme = ResolvedTheme::bundled(dark);
        for (buy, token) in [(true, Token::Buy), (false, Token::Sell)] {
            let ink = theme.color(token);
            let [_, a, b] = oklab(ink);
            if a.hypot(b) >= TRADE_MIN_CHROMA {
                found.push(TradeInk { dark, buy, ink });
            }
        }
    }
    found
}

/// Every warning picking `colour` for workspace `name` raises, in order:
/// each workspace it is close to in the book's order, the buy green, the
/// sell red, then the Dark and the Light tab strip.
#[must_use]
pub fn warnings(book: &WorkspaceBook, name: &str, colour: WorkspaceColour) -> Vec<ColourWarning> {
    let mut found = Vec::new();
    for row in book.list().into_iter().filter(|row| row.name != name) {
        let apart = |dark| oklab_distance(ink(colour.rgb(dark)), ink(row.colour.rgb(dark)));
        if is_close(apart(true)) || is_close(apart(false)) {
            let colour = said(row.colour);
            found.push(ColourWarning::Close {
                workspace: row.name,
                colour,
            });
        }
    }
    let trades = trade_inks();
    let near = |buy: bool| {
        (trades.iter()).any(|trade| {
            trade.buy == buy
                && is_near_trade(oklab_distance(ink(colour.rgb(trade.dark)), trade.ink))
        })
    };
    if near(true) {
        found.push(ColourWarning::NearBuy);
    }
    if near(false) {
        found.push(ColourWarning::NearSell);
    }
    for dark in [true, false] {
        let strip = ResolvedTheme::bundled(dark)
            .title_strip()
            .color(Token::TabStrip);
        if is_low_contrast(contrast_ratio(ink(colour.rgb(dark)), strip)) {
            found.push(ColourWarning::LowContrast { dark });
        }
    }
    found
}

#[cfg(test)]
#[path = "palette_tests.rs"]
mod tests;
