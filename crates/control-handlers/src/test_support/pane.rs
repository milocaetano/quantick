#![cfg(test)]

use crate::axis_pane::{AxisPane, NativeTape};
use crate::bubble_pane::{AssetSave, BubbleLook, BubblePane, TickSpherePane};
use crate::chart_layer::LayerBlocked;
use crate::pane_marks::PaneMarks;
use crate::wire::bubble_preset::{ListInput, ListResult};
use crate::wire::chart::ViewportSnapshot;
use crate::wire::layers::LayerSnapshot;
use crate::wire::orderflow::BubblesStateSnapshot;
use crate::wire::tape_view::TapeViewSnapshot;
use quantick_chart::state::ChartState;
use quantick_control::wire::{CanonicalDecimal, WireU64};
use quantick_drawings::{DrawingTool, Drawings};
use quantick_engine::bar_registry::{BUILTIN_BARS, BarConfiguration};
use quantick_engine::{Bar, BarSpec, Trade};
use quantick_orderflow::tape_view::TapeEnd;
use quantick_orderflow::{HeatmapConfig, LaneWindow, TickTuning};
use quantick_stores::bubble_asset_store::SaveSwitch;
use rust_decimal::Decimal;

/// One pane of a fake tab: a real chart model, an optional venue prefix, and
/// the framing a frame would have laid out.
pub(crate) struct FakePane {
    pub id: u64,
    pub state: ChartState,
    pub prefix: Vec<Bar>,
    pub pagination_revision: u64,
    pub visible_slots: Option<(usize, usize)>,
    pub record_starts_inside: bool,
    /// What the tick sphere rule reads about the pane.
    pub sphere_facts: TickSpherePane,
    /// The pane's order-flow view, which serves its bubble calls.
    pub bubbles: Option<FakeBubbles>,
    /// Every price framing asked of the pane, `None` a fit.
    pub framed: Vec<Option<(f64, f64)>>,
    /// Every time strip act the pane took, by name.
    pub time_acts: Vec<&'static str>,
    /// The pane can show its live bars (overview, centre).
    pub shows_live: bool,
    /// The pane's native tape, when it draws one.
    pub tape: Option<FakeTape>,
    pub drawings: Drawings<DrawingTool>,
    /// The bar rule the pane's marks are cut to.
    pub bars: BarConfiguration,
    /// The registered layers, as the pane's menu reads them.
    pub layers: Vec<(LayerSnapshot, Option<LayerBlocked>)>,
    /// The strategies its tab has armed on its drawings.
    pub armed: Vec<quantick_session::strategy_reading::ArmedReading<quantick_drawings::DrawingId>>,
}

/// A fake native tape: what it was last moved to, and the frame it reports.
#[derive(Default)]
pub(crate) struct FakeTape {
    pub window: Option<(LaneWindow, bool)>,
    pub end: Option<TapeEnd>,
    pub frame: Option<TapeViewSnapshot>,
}

/// A fake order-flow view's bubble look: what it last wore and its asset.
#[derive(Default)]
pub(crate) struct FakeBubbles {
    /// The whole look last worn, and whether it brought its spheres.
    pub preset: Option<(String, bool)>,
    pub worn: Option<Option<String>>,
    pub tuning: TickTuning,
    /// The bound asset's key; `None` is a view bound to no asset.
    pub asset: Option<String>,
    pub save_changes: bool,
    pub config: HeatmapConfig,
}

impl FakePane {
    /// A tick-`ticks` pane fed `trades` live.
    pub fn with_trades(id: u64, ticks: u64, trades: impl IntoIterator<Item = Trade>) -> Self {
        let mut state = ChartState::new(BarSpec::Tick(ticks));
        for trade in trades {
            state.ingest_live(&trade);
        }
        Self {
            id,
            state,
            prefix: Vec::new(),
            pagination_revision: 0,
            visible_slots: None,
            record_starts_inside: false,
            sphere_facts: TickSpherePane::new(false, false),
            bubbles: None,
            framed: Vec::new(),
            time_acts: Vec::new(),
            shows_live: true,
            tape: None,
            drawings: Drawings::default(),
            bars: BUILTIN_BARS
                .parse(&format!("tick:{ticks}"))
                .expect("a tick rule"),
            layers: Vec::new(),
            armed: Vec::new(),
        }
    }
}

impl AxisPane for FakePane {
    fn frame_price_range(&mut self, low: f64, high: f64) {
        self.framed.push(Some((low, high)));
    }
    fn fit_prices(&mut self) {
        self.framed.push(None);
    }
    fn reset_time_strip(&mut self, tape: bool) -> bool {
        if tape && self.tape.is_none() {
            return false;
        }
        self.time_acts
            .push(if tape { "tape_opening" } else { "opening" });
        true
    }
    fn show_live_overview(&mut self) -> bool {
        if self.shows_live {
            self.time_acts.push("overview");
        }
        self.shows_live
    }
    fn centre_live(&mut self, zoom_in: bool) -> bool {
        if self.shows_live {
            self.time_acts
                .push(if zoom_in { "centre_zoom_in" } else { "centre" });
        }
        self.shows_live
    }
    fn framing(&self) -> ViewportSnapshot {
        fake_viewport(self)
    }
    fn native_tape(&mut self) -> Option<&mut dyn NativeTape> {
        Some(self.tape.as_mut()?)
    }
}

impl PaneMarks for FakePane {
    fn marks(&mut self) -> &mut Drawings<DrawingTool> {
        &mut self.drawings
    }
    fn marks_bars(&self) -> BarConfiguration {
        self.bars
    }
}

impl NativeTape for FakeTape {
    fn move_tape_window(&mut self, window: LaneWindow, file: bool) {
        self.window = Some((window, file));
    }
    fn move_tape_end(&mut self, end: TapeEnd) {
        self.end = Some(end);
    }
    fn tape_frame(&self) -> Option<TapeViewSnapshot> {
        self.frame.clone()
    }
}

impl BubblePane<()> for FakePane {
    fn tick_sphere_pane(&self) -> TickSpherePane {
        self.sphere_facts
    }
    fn bubble_look(&mut self) -> Option<&mut dyn BubbleLook<()>> {
        Some(self.bubbles.as_mut()?)
    }
    fn asset_save(&mut self) -> Option<&mut dyn AssetSave> {
        Some(self.bubbles.as_mut()?)
    }
}

impl BubbleLook<()> for FakeBubbles {
    fn wear_preset(&mut self, name: &str, spheres: bool, _: &()) -> Result<bool, String> {
        self.preset = Some((name.to_owned(), spheres));
        Ok(true)
    }
    fn restore_factory_look(&mut self) -> bool {
        self.preset.take().is_some()
    }
    fn presets_page(&self, _: &ListInput) -> Result<ListResult, String> {
        unreachable!("no fake test lists presets")
    }
    fn wear_tick(&mut self, preset: Option<&str>, tuning: TickTuning) -> Result<bool, String> {
        self.worn = Some(preset.map(str::to_owned));
        self.tuning = tuning;
        Ok(true)
    }
    fn look_snapshot(&self) -> BubblesStateSnapshot {
        BubblesStateSnapshot::from_config(&self.config, Decimal::ZERO, None, &[], None, Vec::new())
    }
    fn look_config(&self) -> &HeatmapConfig {
        &self.config
    }
}

impl AssetSave for FakeBubbles {
    fn asset_key(&self) -> Option<&str> {
        self.asset.as_deref()
    }
    fn switch_asset_save(&mut self, on: bool) -> Option<SaveSwitch> {
        self.asset.as_ref()?;
        let moved = self.save_changes != on;
        self.save_changes = on;
        Some(match (moved, on) {
            (false, _) => SaveSwitch::Unmoved,
            (true, true) => SaveSwitch::ScreenStored,
            (true, false) => SaveSwitch::Off,
        })
    }
}

/// A fake pane's framing: the slots a test laid out, the rest fixed.
pub(super) fn fake_viewport(pane: &FakePane) -> ViewportSnapshot {
    let (start, end) = pane.visible_slots.unwrap_or((0, 0));
    ViewportSnapshot {
        geometry_available: pane.visible_slots.is_some(),
        visible_start_slot: WireU64::new(start as u64),
        visible_end_slot_exclusive: WireU64::new(end as u64),
        pixels_per_bar: CanonicalDecimal::new("8").expect("canonical"),
        right_edge_bar: CanonicalDecimal::new("0").expect("canonical"),
        follows_live: true,
        price_auto_fit: true,
        price_axis_inverted: false,
        price_range: None,
        chart_width_px: None,
        tape: pane.tape.as_ref().and_then(|tape| tape.frame.clone()),
    }
}
