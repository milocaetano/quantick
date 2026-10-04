use super::*;

/// The legend is a key for what is on screen: exactly one entry per layer
/// that is both active as a family and switched on individually.
#[test]
fn the_legend_lists_only_the_layers_that_are_on() {
    let labels = |style: &OrderflowRenderStyle| -> Vec<String> {
        legend_entries(style, "liquidity".to_owned())
            .into_iter()
            .map(|(_, label)| label)
            .collect()
    };

    let all = OrderflowRenderStyle::default();
    assert_eq!(
        labels(&all),
        [
            "liquidity",
            "buy aggression",
            "sell aggression",
            "aggression-aligned depletion",
            "L2 reduction (unattributed)",
            "L2 gap",
        ]
    );

    let mut some = all.clone();
    some.show_liquidity = false;
    some.show_sell = false;
    some.show_unattributed = false;
    assert_eq!(
        labels(&some),
        ["buy aggression", "aggression-aligned depletion", "L2 gap"]
    );

    // Family switches still trump the per-layer ones: without L2 capture
    // no depth entry may appear, whatever its individual flag says. The
    // family is now both panes — the key describes the canvas, not one
    // pane of it.
    let mut bubbles_only = all.clone();
    bubbles_only.depth_layer = false;
    bubbles_only.lane_depth_layer = false;
    assert_eq!(labels(&bubbles_only), ["buy aggression", "sell aggression"]);

    // A layer the candles have switched off but the tape still draws keeps
    // its key: withholding it would deny a mark that is on screen.
    let mut tape_only = all.clone();
    tape_only.depth_layer = false;
    tape_only.aggression_layer = false;
    assert_eq!(
        labels(&tape_only),
        labels(&all),
        "the tape alone still earns every key"
    );

    let mut nothing = all;
    nothing.depth_layer = false;
    nothing.aggression_layer = false;
    nothing.lane_depth_layer = false;
    nothing.lane_aggression_layer = false;
    assert!(labels(&nothing).is_empty());
}

/// A reduction kind switched off leaves the canvas, not just the legend.
///
/// The trader's report: unchecking "L2 reduction (unattributed)" took the
/// entry out of the legend and left every violet mark painting. The
/// projection filters those events by threshold and never by choice - both
/// kinds are factual and the history stays complete - so the renderer is
/// the only place the switch can be honoured, and it was not asking. The
/// legend then said the layer was off while the trader looked straight at
/// it, which is the data-honesty rule inverted.
#[test]
fn the_legend_and_the_canvas_agree_about_a_reduction_kind() {
    // The legend already honoured both switches; the canvas did not. Pin
    // them to the same two flags so they cannot drift apart again.
    let entries = |aligned: bool, unattributed: bool| {
        let style = OrderflowRenderStyle {
            depth_layer: true,
            aggression_layer: true,
            show_aligned: aligned,
            show_unattributed: unattributed,
            ..OrderflowRenderStyle::default()
        };
        legend_entries(&style, "liquidity".to_owned())
            .into_iter()
            .map(|(_, label)| label)
            .collect::<Vec<_>>()
    };
    let both = entries(true, true);
    assert!(both.iter().any(|l| l.contains("unattributed")));
    assert!(both.iter().any(|l| l.contains("aggression-aligned")));

    let aligned_only = entries(true, false);
    assert!(
        !aligned_only.iter().any(|l| l.contains("unattributed")),
        "the legend drops the entry"
    );
    assert!(
        aligned_only
            .iter()
            .any(|l| l.contains("aggression-aligned"))
    );
}
