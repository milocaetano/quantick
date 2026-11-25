//! Every supported UI behaviour as one table: the rows `quantick_operability`
//! keeps per family, joined with the `ui` entries of declared control calls.
//!
//! `quantick_operability` depends on nothing, so it cannot see a call
//! declared here; the join sits beside the declarations instead. The order of
//! [`FAMILIES`] is the order the matrix renders and a reviewer reads.

use quantick_operability::UiBehaviour;
use quantick_operability::registry::{
    chart, control, drawings, flatten, indicators, layers, replay, script_editor,
    strategy_describe, total, trading, workspace,
};

/// Every family's rows, in the order the matrix renders them.
///
/// A family that owns rows in two places in that reading declares two slices
/// rather than one: where a row is declared cannot move a line of the document.
pub const FAMILIES: &[&[UiBehaviour]] = &[
    control::WINDOW_AND_TABS,
    replay::ROWS,
    control::LAYOUT_TABS,
    chart::CANVAS,
    layers::ROWS,
    chart::HISTORY,
    indicators::ROWS,
    script_editor::ROWS,
    strategy_describe::ROWS,
    trading::ROWS,
    control::FEED_RECOVERY,
    drawings::TOOL_RAIL,
    drawings::OBJECTS,
    workspace::DESK,
    crate::workspace_book::UI_ROWS,
    workspace::MANAGE,
    control::CHROME,
];

const TOTAL: usize = total(FAMILIES);
// A static: the joined rows outgrew what a const array may copy.
static JOINED: [UiBehaviour; TOTAL] = flatten(FAMILIES);

/// Every supported UI behaviour, in the order the matrix renders.
///
/// Grouped by what the trader is doing rather than by which registry
/// registered it, because "what can I not do without a mouse" is the
/// question this answers.
pub const UI_BEHAVIOURS: &[UiBehaviour] = &JOINED;

/// The committed contents of `docs/control-plane/ui-behaviour-matrix.md`.
pub fn ui_behaviour_matrix_markdown() -> String {
    quantick_operability::matrix::ui_behaviour_matrix_markdown(UI_BEHAVIOURS)
}

#[cfg(test)]
mod tests {
    use quantick_operability::Mapping;
    use quantick_operability::matrix::GENERATED_MARKER;

    use super::{UI_BEHAVIOURS, ui_behaviour_matrix_markdown};

    fn committed_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("crates/control-schema sits two levels below the workspace root")
            .join("docs/control-plane/ui-behaviour-matrix.md")
    }

    /// The authoritative check on the document: the committed file is what the
    /// generator emits today, byte for byte.
    #[test]
    fn the_committed_matrix_is_what_the_generator_emits() {
        let path = committed_path();
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        assert_eq!(
            committed,
            ui_behaviour_matrix_markdown(),
            "the committed UI behaviour matrix is stale. Regenerate it:\n  \
             cargo run -p quantick-app -- --dump-ui-behaviour-matrix > \
             docs/control-plane/ui-behaviour-matrix.md"
        );
    }

    /// Every row is legible on its own: an identifier that reads like a
    /// capability id, a title, a reach, and — where it is excluded — a reason
    /// long enough to argue with.
    ///
    /// A generator that dropped a column would still pass the byte comparison
    /// above, because that test regenerates the file it compares against.
    #[test]
    fn every_row_carries_an_identifier_a_title_a_reach_and_a_reason() {
        for behaviour in UI_BEHAVIOURS {
            assert!(
                !behaviour.id.is_empty()
                    && behaviour
                        .id
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c)),
                "behaviour id is not a dotted lowercase identifier: {}",
                behaviour.id
            );
            assert!(!behaviour.title.is_empty(), "{} has no title", behaviour.id);
            assert!(!behaviour.reach.is_empty(), "{} has no reach", behaviour.id);
            if let Mapping::Excluded { reason, .. } = behaviour.mapping {
                assert!(
                    reason.len() > 40,
                    "{}'s exclusion reason is too short to argue with: {reason}",
                    behaviour.id
                );
            }
        }
    }

    /// The rendered document carries the marker, the regeneration command and
    /// a row for every behaviour.
    #[test]
    fn the_rendered_matrix_carries_the_marker_and_every_row() {
        let markdown = ui_behaviour_matrix_markdown();
        assert!(markdown.contains(GENERATED_MARKER), "no generated marker");
        assert!(
            markdown.contains("--dump-ui-behaviour-matrix"),
            "the document does not say how to regenerate itself"
        );
        for behaviour in UI_BEHAVIOURS {
            assert!(
                markdown.contains(&format!("| `{}` |", behaviour.id)),
                "{} has no row in the rendered matrix",
                behaviour.id
            );
        }
    }

    /// The tape header's window selector and `chart.tape_view.set` with `file`
    /// are one act, and the row says that flag is what makes them one.
    #[test]
    fn the_tape_window_selector_maps_to_the_tape_view_call() {
        let window = UI_BEHAVIOURS
            .iter()
            .find(|behaviour| behaviour.id == "chart.tape.window")
            .expect("the selector has a row");
        assert!(
            matches!(window.mapping, Mapping::Capabilities(ids) if ids == ["chart.tape_view.set"])
        );
        assert!(window.reach.contains("selector"), "{}", window.reach);
        assert!(window.reach.contains("`file`"), "{}", window.reach);
    }

    /// The toolbar's market button and `tab.market.set` are one act: an
    /// operator reading the matrix finds the capability behind the button, and
    /// is not told the picker is the only door to a market.
    #[test]
    fn the_market_button_maps_to_the_switch_capability() {
        let source = UI_BEHAVIOURS
            .iter()
            .find(|behaviour| {
                matches!(behaviour.mapping, Mapping::Capabilities(ids) if ids.contains(&"tab.market.set"))
            })
            .expect("a row maps tab.market.set");
        assert!(
            source.reach.contains("market button"),
            "{} names the market button: {}",
            source.id,
            source.reach
        );
        let open = UI_BEHAVIOURS
            .iter()
            .find(|behaviour| behaviour.id == "tab.market.open")
            .expect("opening a tab has a row");
        assert!(
            matches!(open.mapping, Mapping::Capabilities(ids) if ids == ["market.palette.open"]),
            "+ and Ctrl+T open the palette, the same door market.palette.open opens"
        );
    }
}
