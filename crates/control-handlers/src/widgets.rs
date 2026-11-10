//! The widget panes' wiring below the window: what each pane reads from the
//! active tab, the scene it paints, the open set, and the three capabilities.
//! The window lends the tab's book, tape and config ([`WidgetFeed`]) and
//! paints the [`WidgetScene`] it gets back; `widget.read` returns the same
//! scene's text, so the two cannot disagree.
//!
//! A scene is rebuilt only when what it reads changed: the tab, the tape's
//! last print, the book's time or the body's size. The tape is copied once
//! per change and shared by every pane that reads it.

use std::borrow::Cow;
use std::collections::BTreeMap;

use quantick_chart::widgets::big_trades::{BigTradesLayout, NotionalUnit, big_trades_view};
use quantick_chart::widgets::depth::{
    DEPTH_RESTING_BAR_SHARE, DEPTH_VOLUME_BAR_SHARE, depth_rows_per_side, depth_view,
};
use quantick_chart::widgets::liquidations::{
    LIQUIDATION_HISTOGRAM_ROWS, LiquidationsLayout, liquidations_view,
};
use quantick_chart::widgets::open_interest::{
    OpenInterestLayout, open_interest_plot_px, open_interest_view,
};
use quantick_chart::widgets::scene::{ROW_PX, WidgetScene};
use quantick_chart::widgets::spot_perp_cvd::{
    SpotPerpCvdLayout, spot_perp_cvd_view, spot_perp_plot_px,
};
use quantick_civil::TzOffset;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::authority::WIDGET_MODULE_ID;
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::widget::{
    MAX_READ_TEXTS, WIDGET_CHANGED_EVENT_KIND, WidgetChip, WidgetInput, WidgetRead, WidgetState,
    close_descriptor, open_descriptor, read_widget_descriptor,
};
use quantick_engine::trade_tape::TradeTape;
use quantick_orderflow::engine::BookLadder;
use quantick_orderflow::history::Aggression;
use quantick_orderflow::widgets::big_trades::{BigTradesParams, big_trades};
use quantick_orderflow::widgets::depth::{DepthInput, build_depth};
use quantick_stores::config::{FeedCapabilities, NotionalKind, WidgetSetup};
use rust_decimal::Decimal;
use serde_json::{Value, json};

use crate::dock::ActionDock;
use crate::notify::NotifyAccess;

/// The most recent prints a pane reads from the tape.
pub const WIDGET_TAPE_PRINTS: usize = 20_000;

/// The most rows Big Trades keeps.
pub const BIG_TRADES_MAX_ROWS: usize = 200;

const UNAVAILABLE: &str = "widget panes are unavailable here";

/// What Depth says on a feed with no order book.
pub const DEPTH_NOT_PROVIDED: &str = "feed does not provide an order book";

/// What a pane reads from the tab on screen, lent by the window each frame.
#[derive(Clone, Copy)]
pub struct WidgetFeed<'a> {
    /// The tab's stable id: a new tab is a new tape and book.
    pub tab_id: u64,
    pub capabilities: FeedCapabilities,
    /// The flow pane's published book and the exchange time it is current at.
    pub ladder: Option<(&'a BookLadder, i64)>,
    /// The book the pane saw before, kept by [`WidgetDeck`].
    pub earlier: Option<(&'a BookLadder, i64)>,
    /// The order-flow engine's grouping, for an asset whose config names no
    /// tick.
    pub fallback_tick: Decimal,
    pub trades: &'a TradeTape,
    pub setup: &'a WidgetSetup,
    /// What a quantity counts (`BTC`, `contracts`), from the asset's mode.
    pub base_unit: &'a str,
    pub tz: TzOffset,
    /// The tape's recent prints, when [`WidgetDeck`] already copied them;
    /// `None` reads them from `trades`.
    pub prints: Option<&'a [Aggression]>,
}

impl WidgetFeed<'_> {
    fn tick(&self) -> Decimal {
        self.setup.tick().unwrap_or(self.fallback_tick)
    }

    fn unit(&self) -> NotionalUnit {
        match self.setup.notional_unit {
            NotionalKind::Usd => NotionalUnit::USD,
            NotionalKind::Quote => NotionalUnit::QUOTE,
        }
    }

    fn prints(&self) -> Cow<'_, [Aggression]> {
        self.prints
            .map_or_else(|| Cow::Owned(self.recent_prints()), Cow::Borrowed)
    }

    fn recent_prints(&self) -> Vec<Aggression> {
        let trades = self.trades.last_n(WIDGET_TAPE_PRINTS);
        trades
            .map(|trade| Aggression {
                agg_id: trade.agg_id,
                timestamp_ms: trade.timestamp_ms,
                price: trade.price,
                quantity: trade.quantity,
                side: trade.side,
                generation: None,
            })
            .collect()
    }
}

/// Lays one pane out for a feed in a `width` by `height` body.
pub type SceneBuilder = fn(&WidgetFeed<'_>, f32, f32) -> WidgetScene;

/// One pane: its id, header title, the capability it needs, what it reads
/// and its scene.
pub struct WidgetSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub gate: fn(&FeedCapabilities) -> bool,
    /// Reads the order book: the deck keeps the book before the current one.
    pub book: bool,
    /// Reads the tape: a new print rebuilds the scene.
    pub tape: bool,
    pub scene: SceneBuilder,
}

/// The DOM ladder from the flow pane's book and tape.
#[must_use]
pub fn depth_scene(feed: &WidgetFeed<'_>, width: f32, height: f32) -> WidgetScene {
    if !feed.capabilities.book_capture {
        return WidgetScene::empty(DEPTH_NOT_PROVIDED, Vec::new(), width, height);
    }
    let prints = feed.prints();
    let model = feed.ladder.and_then(|(ladder, ladder_at_ms)| {
        build_depth(&DepthInput {
            earlier: feed.earlier,
            ladder,
            ladder_at_ms,
            prints: &prints,
            tick: feed.tick(),
            rows_per_side: depth_rows_per_side(height),
        })
    });
    let (resting, volume) = (DEPTH_RESTING_BAR_SHARE, DEPTH_VOLUME_BAR_SHARE);
    depth_view(model.as_ref(), feed.tick(), resting * width, volume * width).paint(width, height)
}

/// The prints at or above the configured threshold. The window totals are
/// measured to the tape's newest print, so a replay reads its own time.
#[must_use]
pub fn big_trades_scene(feed: &WidgetFeed<'_>, width: f32, height: f32) -> WidgetScene {
    let min_notional = Decimal::from(feed.setup.big_trade_min());
    let layout = BigTradesLayout {
        presets: &feed.setup.big_trade_presets,
        min_notional,
        merge_fills: feed.setup.merge_fills,
        tick: feed.tick(),
        tz: feed.tz,
        unit: feed.unit(),
    };
    let params = BigTradesParams {
        min_notional,
        merge_fills: feed.setup.merge_fills,
        max_rows: BIG_TRADES_MAX_ROWS,
    };
    let prints = feed.prints();
    let model = prints
        .last()
        .map(|newest| big_trades(&prints, &params, newest.timestamp_ms));
    big_trades_view(model.as_ref(), &layout).paint(width, height)
}

/// No feed streams liquidations yet, so the pane says so.
#[must_use]
pub fn liquidations_scene(feed: &WidgetFeed<'_>, width: f32, height: f32) -> WidgetScene {
    let layout = LiquidationsLayout {
        tick: feed.tick(),
        tz: feed.tz,
        unit: feed.unit(),
        histogram_width_px: width,
        histogram_height_px: LIQUIDATION_HISTOGRAM_ROWS * ROW_PX,
    };
    liquidations_view(None, &layout).paint(width, height)
}

/// No feed polls open interest or funding yet, so the pane says so.
#[must_use]
pub fn open_interest_scene(feed: &WidgetFeed<'_>, width: f32, height: f32) -> WidgetScene {
    let (plot_width_px, plot_height_px) = open_interest_plot_px(width, height);
    let layout = OpenInterestLayout {
        tick: feed.tick(),
        base_unit: feed.base_unit,
        unit: feed.unit(),
        plot_width_px,
        plot_height_px,
    };
    open_interest_view(None, &layout).paint(width, height)
}

/// No feed pairs a spot and a perpetual tape yet, so the pane says so.
#[must_use]
pub fn spot_perp_cvd_scene(feed: &WidgetFeed<'_>, width: f32, height: f32) -> WidgetScene {
    let (plot_width_px, plot_height_px) = spot_perp_plot_px(width, height);
    let layout = SpotPerpCvdLayout {
        base_unit: feed.base_unit,
        plot_width_px,
        plot_height_px,
    };
    spot_perp_cvd_view(None, &layout).paint(width, height)
}

/// What a cached scene was built from.
#[derive(Clone, Copy, PartialEq, Eq)]
struct SceneKey {
    tab_id: u64,
    tape: Option<TapeKey>,
    ladder_at_ms: Option<i64>,
    size: (u32, u32),
}

/// The tape's length and its newest print's id and time.
type TapeKey = (usize, u64, i64);

/// The open panes, the book each saw before, the shared prints and their
/// scenes.
#[derive(Default)]
pub struct WidgetDeck {
    open: Vec<&'static str>,
    /// The tab the kept book and prints came from.
    tab_id: Option<u64>,
    /// The latest book seen and the one before it, with their times.
    books: [Option<(BookLadder, i64)>; 2],
    prints: (Option<TapeKey>, Vec<Aggression>),
    scenes: BTreeMap<&'static str, (SceneKey, WidgetScene)>,
}

impl WidgetDeck {
    /// Open or close `id`; whether it is open now.
    pub fn set_open(&mut self, id: &'static str, open: bool) -> bool {
        self.open.retain(|shown| *shown != id);
        if open {
            self.open.push(id);
        }
        open
    }

    /// Open every pane a comma-separated `list` names that `registry`
    /// knows; the rest open nothing.
    pub fn open_listed(&mut self, registry: &'static [WidgetSpec], list: &str) {
        for id in list.split(',').map(str::trim) {
            if let Ok(spec) = find(registry, id) {
                self.set_open(spec.id, true);
            }
        }
    }

    #[must_use]
    pub fn is_open(&self, id: &str) -> bool {
        self.open.contains(&id)
    }

    /// The open panes, in the order they opened.
    #[must_use]
    pub fn open_ids(&self) -> Vec<&'static str> {
        self.open.clone()
    }

    /// `spec`'s scene for `feed` in a `width` by `height` body, rebuilt
    /// only when what it reads changed.
    pub fn scene(
        &mut self,
        spec: &WidgetSpec,
        feed: &WidgetFeed<'_>,
        (width, height): (f32, f32),
    ) -> &WidgetScene {
        if self.tab_id != Some(feed.tab_id) {
            self.tab_id = Some(feed.tab_id);
            self.books = [None, None];
            self.prints = (None, Vec::new());
        }
        if spec.book {
            self.observe(feed);
        }
        let tape = feed.trades.last().map(|trade| {
            let len = feed.trades.len();
            (len, trade.agg_id, trade.timestamp_ms)
        });
        let key = SceneKey {
            tab_id: feed.tab_id,
            tape: tape.filter(|_| spec.tape),
            ladder_at_ms: feed.ladder.map(|(_, at)| at).filter(|_| spec.book),
            size: (width.round() as u32, height.round() as u32),
        };
        let fresh = !matches!(self.scenes.get(spec.id), Some((cached, _)) if *cached == key);
        if fresh {
            if spec.tape && self.prints.0 != tape {
                self.prints = (tape, feed.recent_prints());
            }
            let [current, earlier] = &self.books;
            let book = |kept| if spec.book { held(kept) } else { None };
            let feed = WidgetFeed {
                ladder: book(current),
                earlier: book(earlier),
                prints: spec.tape.then_some(self.prints.1.as_slice()),
                ..*feed
            };
            let scene = (spec.scene)(&feed, width, height);
            self.scenes.insert(spec.id, (key, scene));
        }
        &self.scenes[spec.id].1
    }

    /// Keep the book the panes saw before the current one; a missing book
    /// forgets both, so a stale ladder never stands in for it.
    fn observe(&mut self, feed: &WidgetFeed<'_>) {
        let Some((ladder, at_ms)) = feed.ladder else {
            self.books = [None, None];
            return;
        };
        match &self.books[0] {
            Some((_, seen)) if *seen == at_ms => {}
            _ => {
                let current = self.books[0].take();
                self.books = [Some((ladder.clone(), at_ms)), current];
            }
        }
    }
}

fn held(book: &Option<(BookLadder, i64)>) -> Option<(&BookLadder, i64)> {
    book.as_ref().map(|(ladder, at_ms)| (ladder, *at_ms))
}

/// The open state of `spec` and whether `capabilities` provide it.
#[must_use]
pub fn state(spec: &WidgetSpec, deck: &WidgetDeck, capabilities: &FeedCapabilities) -> WidgetState {
    WidgetState {
        id: spec.id.to_owned(),
        title: spec.title.to_owned(),
        open: deck.is_open(spec.id),
        provided: (spec.gate)(capabilities),
    }
}

/// What `widget.read` answers: the scene's chips, rows and text.
#[must_use]
pub fn read(state: WidgetState, market: String, scene: &WidgetScene) -> WidgetRead {
    WidgetRead {
        state,
        market,
        chips: scene
            .chips
            .iter()
            .map(|(text, on)| WidgetChip {
                text: text.clone(),
                on: *on,
            })
            .collect(),
        rows: scene.rows as u64,
        empty: scene.empty.clone(),
        texts: scene
            .texts()
            .take(MAX_READ_TEXTS)
            .map(str::to_owned)
            .collect(),
    }
}

/// What the window lends the three capabilities.
pub trait WidgetHost {
    /// Open or close the pane `id` names when `open` says so, then what it
    /// paints for the active tab; or why not.
    fn widget(&mut self, _id: &str, _open: Option<bool>) -> Result<WidgetRead, String> {
        Err(UNAVAILABLE.to_owned())
    }
}

/// `spec` in `registry` by id, or the ids there are.
///
/// # Errors
/// The unknown id and the known ones.
pub fn find<'a>(registry: &'a [WidgetSpec], id: &str) -> Result<&'a WidgetSpec, String> {
    registry.iter().find(|spec| spec.id == id).ok_or_else(|| {
        let known: Vec<&str> = registry.iter().map(|spec| spec.id).collect();
        format!("unknown widget '{id}'; known: {}", known.join(", "))
    })
}

/// Dock the three capabilities; open and close stay out of a replay's trace.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: WidgetHost + ?Sized,
    A: NotifyAccess,
{
    registry.register_outside_replay(open_descriptor(), open)?;
    registry.register_outside_replay(close_descriptor(), close)?;
    registry.register(read_widget_descriptor(), read_action)
}

fn open<H: WidgetHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    toggle(app, access, actor, input, true)
}

fn close<H: WidgetHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    toggle(app, access, actor, input, false)
}

fn toggle<H: WidgetHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
    open: bool,
) -> Result<Value, ControlError> {
    let input = parse(input)?;
    let state = app
        .widget(&input.id, Some(open))
        .map_err(ControlError::invalid_request)?
        .state;
    let payload = json!({"id": state.id, "open": state.open});
    access.record_event(NewEvent::by(
        WIDGET_MODULE_ID,
        WIDGET_CHANGED_EVENT_KIND,
        actor,
        payload,
    ));
    serde_json::to_value(state).map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn read_action<H: WidgetHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input = parse(input)?;
    let read = app
        .widget(&input.id, None)
        .map_err(ControlError::invalid_request)?;
    serde_json::to_value(read).map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn parse(input: &Value) -> Result<WidgetInput, ControlError> {
    serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}

#[cfg(test)]
#[path = "widgets_tests.rs"]
mod widgets_tests;
