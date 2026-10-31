//! Which toolbar groups stay inline at a width: the rule, with no widget in it.
//!
//! The painter asks once a frame and draws what comes back; everything folded
//! stays reachable through its overflow menu.

// Width estimates for the overflow rule, in pixels. egui sizes widgets while
// drawing them, so the plan is decided up front from these; they only need to
// be right enough that folding starts before clipping does.
/// Margins, separators and inter-group spacing.
const W_SLACK: f32 = 90.0;
/// SOURCE with the full feed name.
const W_SOURCE_FULL: f32 = 350.0;
/// SOURCE with the feed shrunk to its initial.
const W_SOURCE_SHRUNK: f32 = 260.0;
/// The `History: <target> ▾` split button.
const W_HISTORY: f32 = 150.0;
/// The LAYERS icon group (bubbles, heatmap, live strip, footprint).
const W_LAYERS: f32 = 100.0;
/// One 28 px icon button (LOOK, PANELS, or the overflow `⋯`).
const W_ICON: f32 = 28.0;
/// Which groups stay inline at the current width. Everything folded is
/// reachable through the `⋯` overflow menu instead — never hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollapsePlan {
    /// LOOK renders as its own icon.
    pub look_inline: bool,
    /// PANELS renders as its own icon.
    pub panels_inline: bool,
    /// HISTORY renders as the split button.
    pub history_inline: bool,
    /// TRADE renders as the BUY/SELL pair.
    pub trade_inline: bool,
    /// The feed combo shows its full display name.
    pub feed_full_name: bool,
}

impl CollapsePlan {
    /// Nothing folded.
    pub const FULL: Self = Self {
        look_inline: true,
        panels_inline: true,
        history_inline: true,
        trade_inline: true,
        feed_full_name: true,
    };

    /// Whether the `⋯` menu is needed to reach a folded group. A shrunk feed
    /// name loses no affordance, so it alone needs no overflow.
    #[must_use]
    pub fn overflow_needed(self) -> bool {
        !(self.look_inline && self.panels_inline && self.history_inline && self.trade_inline)
    }

    /// Estimated width of the toolbar under this plan. The `⋯` slot is
    /// always reserved: it appears exactly when something folds, so counting
    /// it conditionally would make the first fold width-neutral and leave
    /// the planner skipping straight past it. `trade_width` comes in as a
    /// parameter: TRADE reserves its open-state width (the toolbar's `W_TRADE`).
    #[must_use]
    pub fn width(self, trade_width: f32) -> f32 {
        let mut width = W_SLACK + W_ICON + W_LAYERS;
        width += if self.feed_full_name {
            W_SOURCE_FULL
        } else {
            W_SOURCE_SHRUNK
        };
        if self.history_inline {
            width += W_HISTORY;
        }
        if self.trade_inline {
            width += trade_width;
        }
        if self.look_inline {
            width += W_ICON;
        }
        if self.panels_inline {
            width += W_ICON;
        }
        width
    }
}

/// Decide what folds at `available` width, following the §6 collapse order.
/// The last plan is accepted even when it still overflows — there is nothing
/// left to fold, and the symbol and LAYERS are never candidates.
#[must_use]
pub fn collapse_plan(available: f32, trade_width: f32) -> CollapsePlan {
    let mut plan = CollapsePlan::FULL;
    let steps: [fn(&mut CollapsePlan); 5] = [
        |plan| plan.look_inline = false,
        |plan| plan.panels_inline = false,
        |plan| plan.history_inline = false,
        |plan| plan.trade_inline = false,
        |plan| plan.feed_full_name = false,
    ];
    for fold in steps {
        if plan.width(trade_width) <= available {
            return plan;
        }
        fold(&mut plan);
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flat TRADE pair's width — the old `W_TRADE` constant, kept as the
    /// tests' baseline now that the group is measured from its labels.
    const FLAT_TRADE_W: f32 = 110.0;

    /// How many §6 collapse steps a plan has taken. `None` when the plan is
    /// not one of the six canonical states — folding out of order.
    fn stage(plan: CollapsePlan) -> Option<usize> {
        let states = [
            (true, true, true, true, true),
            (false, true, true, true, true),
            (false, false, true, true, true),
            (false, false, false, true, true),
            (false, false, false, false, true),
            (false, false, false, false, false),
        ];
        states.iter().position(|&state| {
            state
                == (
                    plan.look_inline,
                    plan.panels_inline,
                    plan.history_inline,
                    plan.trade_inline,
                    plan.feed_full_name,
                )
        })
    }

    #[test]
    fn a_wide_toolbar_folds_nothing() {
        let plan = collapse_plan(10_000.0, FLAT_TRADE_W);
        assert_eq!(stage(plan), Some(0));
        assert!(!plan.overflow_needed());
    }

    #[test]
    fn a_hopeless_width_ends_fully_folded() {
        let plan = collapse_plan(0.0, FLAT_TRADE_W);
        assert_eq!(stage(plan), Some(5));
        assert!(plan.overflow_needed());
    }

    #[test]
    fn folding_follows_the_documented_order_and_never_skips() {
        let mut last_stage = 5;
        // Sweep from narrow to wide: the collapse stage must only decrease,
        // and every plan must be one of the canonical §6 states.
        let mut width = 100.0;
        while width <= 2000.0 {
            let plan = collapse_plan(width, FLAT_TRADE_W);
            let stage = stage(plan).expect("plans fold strictly in the §6 order");
            assert!(
                stage <= last_stage,
                "widening from {width} px must never fold more"
            );
            last_stage = stage;
            width += 10.0;
        }
        assert_eq!(last_stage, 0, "the sweep must reach the unfolded plan");
    }

    #[test]
    fn look_folds_first_and_alone_at_a_mildly_tight_width() {
        // One pixel short of the full plan: exactly LOOK moves to overflow.
        let full_width = CollapsePlan::FULL.width(FLAT_TRADE_W);
        let plan = collapse_plan(full_width - 1.0, FLAT_TRADE_W);
        assert_eq!(stage(plan), Some(1));
        assert!(plan.overflow_needed());
    }

    #[test]
    fn a_shrunk_feed_name_alone_never_requires_the_overflow_menu() {
        let plan = CollapsePlan {
            feed_full_name: false,
            ..CollapsePlan::FULL
        };
        assert!(!plan.overflow_needed());
    }
}
