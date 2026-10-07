//! The quick bar switch, as a [`Surface`] — ProfitChart's type-a-number
//! period search.
//!
//! A bare digit typed over the focused chart opens a list of every bar the
//! number can mean (`15` minutes, seconds, hours, ticks, volume, dollar,
//! imbalance, trades, Renko); a letter after it (`50R`) keeps the kind that
//! declares it. The engine's registry reads the query and writes the list;
//! this file only reads keys and draws it. Enter applies the highlighted row
//! to the pane it opened over through `layout.pane.set_bar_spec`'s own path,
//! so an agent reaches the same outcome by that capability.

use eframe::egui;
use quantick_engine::bar_registry::{BUILTIN_BARS, BarConfiguration, quick_query_text};

use super::{Surface, SurfaceEnv, SurfaceResponse};
use crate::pane::PaneSide;
use crate::theme;

/// How far below the chart's top edge the list opens.
const TOP_MARGIN_PX: f32 = 24.0;
/// Wide enough for the longest summary, `imbalance(dollar 100000)`.
const WIDTH_PX: f32 = 260.0;

/// One bar change the trader chose: which pane, and the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BarSwitchRequest {
    pub tab: u64,
    pub side: PaneSide,
    pub config: BarConfiguration,
}

#[derive(Debug)]
struct Open {
    query: String,
    selected: usize,
    tab: u64,
    side: PaneSide,
    /// Opened by the harness hook on the first frame, before the workspace's
    /// focus has settled: it follows the focused pane rather than closing
    /// when focus lands somewhere else.
    follows_focus: bool,
    /// Focus is requested once, on the frame the list opens.
    focus_pending: bool,
}

/// Closed until a digit is typed over a chart.
#[derive(Default)]
pub(crate) struct BarSwitchSurface {
    open: Option<Open>,
}

/// The digits of one frame's typed text, if that text is only digits.
fn typed_digits(events: &[egui::Event]) -> Option<String> {
    let text: String = events
        .iter()
        .filter_map(|event| match event {
            egui::Event::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())).then_some(text)
}

impl BarSwitchSurface {
    /// Open over `side` of tab `tab` with `query` typed.
    pub fn open(&mut self, tab: u64, side: PaneSide, query: &str) {
        self.open = Some(Open {
            query: quick_query_text(query),
            selected: 0,
            tab,
            side,
            follows_focus: false,
            focus_pending: true,
        });
    }

    /// [`Self::open`] for the harness hook: the popup follows the focused
    /// pane instead of closing when it moves.
    pub fn open_following_focus(&mut self, tab: u64, side: PaneSide, query: &str) {
        self.open(tab, side, query);
        if let Some(open) = self.open.as_mut() {
            open.follows_focus = true;
        }
    }

    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// What the typed text offers, in the order shown.
    pub fn candidates(&self) -> Vec<BarConfiguration> {
        self.open
            .as_ref()
            .map(|open| BUILTIN_BARS.quick_matches(&open.query))
            .unwrap_or_default()
    }

    /// Open on a bare digit when no widget holds the keyboard.
    fn open_on_digit(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) {
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        let digits = ctx.input(|input| {
            let bare = !input.modifiers.ctrl && !input.modifiers.command && !input.modifiers.alt;
            bare.then(|| typed_digits(&input.events)).flatten()
        });
        if let Some(digits) = digits {
            self.open(env.active_tab, env.focused_side, &digits);
        }
    }
}

impl Surface for BarSwitchSurface {
    fn id(&self) -> &'static str {
        "bar_switch"
    }

    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        if self.open.is_none() {
            self.open_on_digit(ctx, env);
        }
        let candidates = self.candidates();
        let id = self.id();
        let Some(open) = self.open.as_mut() else {
            return SurfaceResponse::default();
        };
        let query_id = egui::Id::new(id).with("query");
        // The switch answers for the pane it opened over: once another pane
        // is focused, nothing it holds applies.
        if open.follows_focus {
            open.tab = env.active_tab;
            open.side = env.focused_side;
        }
        let pane_moved = env.active_tab != open.tab || env.focused_side != open.side;
        // Keys are the switch's only while its field holds the keyboard; once
        // focus is elsewhere they belong to whoever holds it.
        let typing = !pane_moved && ctx.memory(|memory| memory.has_focus(query_id));
        let (escape, enter, down, up) = ctx.input(|input| {
            (
                input.key_pressed(egui::Key::Escape),
                typing && input.key_pressed(egui::Key::Enter),
                typing && input.key_pressed(egui::Key::ArrowDown),
                typing && input.key_pressed(egui::Key::ArrowUp),
            )
        });
        let last = candidates.len().saturating_sub(1);
        if down {
            open.selected = (open.selected + 1).min(last);
        }
        if up {
            open.selected = open.selected.saturating_sub(1);
        }
        open.selected = open.selected.min(last);
        let mut clicked = None;
        let mut window = egui::Window::new("Bars")
            .id(egui::Id::new(id))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size(egui::vec2(WIDTH_PX, 0.0))
            .order(egui::Order::Foreground);
        window = match env.focused_chart_area {
            Some(area) => {
                window.fixed_pos(area.center_top() + egui::vec2(-WIDTH_PX / 2.0, TOP_MARGIN_PX))
            }
            None => window.anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 96.0)),
        };
        let asked = open.query.clone();
        let shown = window.show(ctx, |ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut open.query)
                    .id(query_id)
                    .hint_text("bar size")
                    .desired_width(f32::INFINITY),
            );
            if open.focus_pending {
                open.focus_pending = false;
                edit.request_focus();
                if let Some(mut state) = egui::TextEdit::load_state(ctx, edit.id) {
                    let end = egui::text::CCursor::new(open.query.chars().count());
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(end)));
                    state.store(ctx, edit.id);
                }
            }
            // Up and Down walk the list; left to egui they would move focus
            // onto a row, and Enter would then press that row instead. egui
            // honours the filter from the frame after focus arrives.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    edit.id,
                    egui::EventFilter {
                        vertical_arrows: true,
                        ..Default::default()
                    },
                );
            });
            let kept = quick_query_text(&open.query);
            if kept != open.query {
                open.query = kept;
            }
            // A new query is a new list: a highlight kept by index would land
            // on a different kind (15's fourth row is tick, 150's is volume).
            if open.query != asked {
                open.selected = 0;
            }
            if candidates.is_empty() {
                ui.label(egui::RichText::new("no bar at this size").color(theme::TEXT_SUPPORT));
            }
            for (index, config) in candidates.iter().enumerate() {
                let row = ui.selectable_label(index == open.selected, config.quick_label());
                if row.clicked() {
                    clicked = Some(index);
                }
            }
        });
        let chosen = clicked
            .filter(|_| !pane_moved)
            .or((enter && !candidates.is_empty()).then_some(open.selected));
        let request = chosen.map(|index| BarSwitchRequest {
            tab: open.tab,
            side: open.side,
            config: candidates[index],
        });
        // It closes with its pane, or once the keyboard went elsewhere while
        // the pointer is off the list (a row being pressed holds focus for
        // the frame before its click lands).
        let over_list = shown.is_some_and(|shown| shown.response.contains_pointer());
        let abandoned =
            !open.focus_pending && !ctx.memory(|memory| memory.has_focus(query_id)) && !over_list;
        if escape || request.is_some() || open.query.is_empty() || pane_moved || abandoned {
            self.open = None;
            ctx.memory_mut(|memory| memory.surrender_focus(query_id));
        }
        SurfaceResponse {
            bar_switch: request,
            ..SurfaceResponse::default()
        }
    }

    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, env: &SurfaceEnv<'_>) {
        if let Some(query) =
            crate::hooks::captured::var("QUANTICK_BAR_SWITCH").filter(|query| !query.is_empty())
        {
            self.open_following_focus(env.active_tab, env.focused_side, &query);
        }
    }
}

crate::hooks::declare_hooks!["QUANTICK_BAR_SWITCH"];

#[cfg(test)]
#[path = "tests/bar_switch.rs"]
mod bar_switch_tests;
