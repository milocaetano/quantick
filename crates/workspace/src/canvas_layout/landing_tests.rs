//! A recipe landed on a tab's canvas and read back off it: the laid canvas
//! builds no standing chart again, and a reading holds the charts the
//! canvas shows, padded or cut sizes and folds reading as no edit.

use super::*;
use crate::workspace_recipe::{ContextPaneRecipe, DeclaredLayout, WorkspaceRecipe};

/// A recipe numbers its canvas's slots its own way. Laid over a tab whose
/// seats were read against the preset a switch picked first, it seats
/// every chart afresh: a renumbered slot is no removal, so no standing
/// chart is built again, which lost the bar rule and layers the landing
/// had just put on it.
#[test]
fn a_laid_recipe_canvas_builds_no_standing_chart_again() {
    let stack = crate::canvas::preset("stack2+flow").unwrap();
    let mut state =
        crate::canvas::CanvasState::from_preset(crate::canvas::preset("time+flow").unwrap());
    let mut seats = crate::canvas::Seats::default();
    seats.refresh(&state);
    state.apply_preset(stack);
    seats.clear();
    seats.refresh(&state);
    let recipe = WorkspaceRecipe {
        layout: DeclaredLayout::TimeTimeAndFlow,
        canvas: Some(crate::canvas::CanvasState::from_preset(stack)),
        ..WorkspaceRecipe::default()
    };
    lay_recipe_canvas(&mut state, &mut seats, &recipe);
    seats.refresh(&state);
    assert_eq!(seats.take_reseed(), None, "no chart is built again");
    assert_eq!(state, recipe_canvas(&recipe));
}

/// A tab keeps, off screen, the charts an earlier layout built. What it
/// reads holds the charts its canvas shows: back on one chart from two,
/// the second is no part of it, so a tab switched from a two-chart
/// workspace onto a one-chart one reads as the one-chart recipe.
#[test]
fn a_reading_holds_only_the_charts_its_canvas_shows() {
    let read = WorkspaceRecipe {
        layout: DeclaredLayout::TimeAndFlow,
        context_panes: vec![ruled("time:1m"), ruled("renko:20")],
        ..WorkspaceRecipe::default()
    };
    let seats = crate::canvas::Seats::default();
    let cut = |recipe: &WorkspaceRecipe| {
        let state = recipe.canvas_state();
        recipe.clone().cut_to_canvas(&state, &seats).context_panes
    };
    assert_eq!(cut(&read), [ruled("time:1m")], "the second is off screen");
    let two = WorkspaceRecipe {
        layout: DeclaredLayout::TimeTimeAndFlow,
        ..read
    };
    assert_eq!(cut(&two), two.context_panes, "both charts show");
}

/// The top chart of two removed, the chart left keeps its seat: what
/// the tab reads is that chart's pane, not the removed one's.
#[test]
fn a_reading_after_a_removal_holds_the_chart_still_shown() {
    let mut state =
        crate::canvas::CanvasState::from_preset(crate::canvas::preset("stack2+flow").unwrap());
    let mut seats = crate::canvas::Seats::default();
    seats.refresh(&state);
    let top = state.tree().panes()[0].0;
    state.remove(top).unwrap();
    let read = WorkspaceRecipe {
        layout: DeclaredLayout::TimeTimeAndFlow,
        context_panes: vec![ruled("time:1m"), ruled("renko:20")],
        ..WorkspaceRecipe::default()
    };
    let shown = read.cut_to_canvas(&state, &seats).context_panes;
    assert_eq!(shown, [ruled("renko:20")], "the chart left on screen");
}

fn ruled(bars: &str) -> ContextPaneRecipe {
    ContextPaneRecipe {
        bars: Some(bars.to_owned()),
        ..ContextPaneRecipe::default()
    }
}

/// A context size or fold past the end of a list is automatic and open,
/// as a missing entry is: a reading padded with them, or cut short of a
/// recipe's, holds that recipe; a real size or fold past the end does not.
#[test]
fn padding_a_size_or_fold_list_is_no_edit() {
    let recipe = WorkspaceRecipe {
        layout: DeclaredLayout::TimeAndFlow,
        context_heights: vec![0.4, 0.0],
        context_panes_collapsed: vec![false],
        ..WorkspaceRecipe::default()
    };
    let padded = WorkspaceRecipe {
        context_heights: vec![0.4, 0.0, 0.0],
        context_panes_collapsed: vec![false, false, false],
        ..recipe.clone()
    };
    let short = WorkspaceRecipe {
        context_heights: vec![0.4],
        context_panes_collapsed: Vec::new(),
        ..recipe.clone()
    };
    assert!(recipe.holds(&padded) && recipe.holds(&short));
    let sized = WorkspaceRecipe {
        context_heights: vec![0.4, 0.0, 0.3],
        ..recipe.clone()
    };
    let folded = WorkspaceRecipe {
        context_panes_collapsed: vec![false, true],
        ..recipe.clone()
    };
    assert!(!recipe.holds(&sized) && !recipe.holds(&folded));
}
