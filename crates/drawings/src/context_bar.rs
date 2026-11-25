//! Which cells the selected drawing's context bar shows, decided from what
//! the object can be asked for. Painting, sizes and input stay with the
//! host; this is the part a script or a test can ask without a window.

use smallvec::SmallVec;

/// One cell of the bar, in paint order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Grip,
    /// Present only for an object an operator other than the trader placed:
    /// the chart says whose hand it was before the trader acts on it.
    Author,
    /// Arm the tool that drew this object again, so the next stroke starts
    /// from the bar the release just opened instead of from the rail.
    DrawAgain,
    Color,
    Width,
    /// A position's target zone colour and opacity, ending in its default.
    TargetColor,
    /// A position's stop zone colour and opacity, ending in its default.
    StopColor,
    /// A position's risk/reward, named risk first.
    RiskReward,
    /// Type/glyph size, for the tools drawn as glyphs rather than strokes.
    GlyphSize,
    Separator,
    Duplicate,
    Lock,
    /// This object's own OHLC magnet, for the tools the magnet moves.
    Magnet,
    Hide,
    Sharing,
    Settings,
    Delete,
}

/// What the selected object can actually be asked for. The bar is built from
/// this and nothing else — an unsupported property is absent, never a grey
/// button, which is the rule [`crate::MarkTool`] already states for
/// the inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub stroke_width: bool,
    pub glyph_size: bool,
    /// Whether this object was placed by something other than the trader.
    pub authored: bool,
    /// Whether the gear earns its slot. It does not when the inspector is
    /// pinned: its destination is already on screen.
    pub settings: bool,
    /// Whether this drawing has enough market-time information to be shown
    /// on the tab's other charts.
    pub sharing: bool,
    /// Whether the OHLC magnet moves this object's anchors: a tool that
    /// snaps by its own rule has no magnet to switch.
    pub magnet: bool,
    /// A long or short position: its two zone colours and its risk/reward
    /// take the colour and width cells, and every setting lives on the bar,
    /// so it has no gear.
    pub position: bool,
    /// Whether this object's tool is one the trader keeps drawing with —
    /// a freehand stroke — so the bar offers to arm it again.
    pub draw_again: bool,
}

/// The cells of a bar for an object with these capabilities.
///
/// A `SmallVec`, not a `Vec`: this is rebuilt on every frame something is
/// selected, and the bar never holds more cells than fit inline.
#[must_use]
pub fn slots(caps: Capabilities) -> SmallVec<[Slot; 14]> {
    let mut slots = SmallVec::from_slice(&[Slot::Grip]);
    if caps.authored {
        slots.push(Slot::Author);
        slots.push(Slot::Separator);
    }
    if caps.draw_again {
        slots.push(Slot::DrawAgain);
        slots.push(Slot::Separator);
    }
    if caps.position {
        slots.extend([Slot::TargetColor, Slot::StopColor, Slot::RiskReward]);
    } else {
        slots.push(Slot::Color);
    }
    if caps.stroke_width && !caps.position {
        slots.push(Slot::Width);
    }
    if caps.glyph_size {
        slots.push(Slot::GlyphSize);
    }
    slots.push(Slot::Separator);
    slots.push(Slot::Duplicate);
    slots.push(Slot::Lock);
    if caps.magnet {
        slots.push(Slot::Magnet);
    }
    slots.push(Slot::Hide);
    if caps.sharing {
        slots.push(Slot::Sharing);
    }
    if caps.settings && !caps.position {
        slots.push(Slot::Settings);
    }
    // The hairline before the trash is not decoration. It turns the 4 px gap
    // into 9 px, so a hand that misses the gear by a few pixels lands on
    // nothing instead of on the one irreversible button.
    slots.push(Slot::Separator);
    slots.push(Slot::Delete);
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(position: bool) -> Capabilities {
        Capabilities {
            stroke_width: true,
            glyph_size: false,
            authored: false,
            settings: true,
            sharing: true,
            magnet: true,
            position,
            draw_again: false,
        }
    }

    /// A position's bar is its two colours and its risk/reward, and no gear:
    /// every setting it has lives on the bar.
    #[test]
    fn a_position_trades_colour_width_and_gear_for_its_own_three_cells() {
        let position = slots(caps(true));
        assert_eq!(
            &position[1..4],
            &[Slot::TargetColor, Slot::StopColor, Slot::RiskReward]
        );
        for gone in [Slot::Color, Slot::Width, Slot::Settings] {
            assert!(!position.contains(&gone), "{gone:?}");
        }
        let other = slots(caps(false));
        assert!(other.contains(&Slot::Settings) && other.contains(&Slot::Color));
        assert!(!other.contains(&Slot::RiskReward));
    }
}
