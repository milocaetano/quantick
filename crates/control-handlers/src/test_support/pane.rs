use super::*;

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

/// One fake pane read with the tab it sits on, as the window's adapter reads
/// a pane with its tab.
struct FakePaneRead<'a> {
    tab: &'a FakeTab,
    pane: &'a FakePane,
}

impl ChartPaneSource for FakePaneRead<'_> {
    fn pane_id(&self) -> u64 {
        self.pane.id
    }
    fn state(&self) -> &ChartState {
        &self.pane.state
    }
    fn pagination_revision(&self) -> u64 {
        self.pane.pagination_revision
    }
    fn closed_slots(&self) -> usize {
        self.pane.prefix.len() + self.pane.state.bars().len()
    }
    fn seam_slot(&self) -> usize {
        self.pane.prefix.len()
    }
    fn closed_bar(&self, slot: usize) -> Option<&Bar> {
        self.pane
            .prefix
            .get(slot)
            .or_else(|| self.pane.state.bars().get(slot - self.pane.prefix.len()))
    }
    fn venue_prefix(&self) -> &[Bar] {
        &self.pane.prefix
    }
    fn slot_open_time(&self, slot: usize) -> Option<i64> {
        match self.pane.prefix.get(slot) {
            Some(bar) => Some(bar.open_time),
            None => self
                .pane
                .state
                .slot_open_time(slot - self.pane.prefix.len()),
        }
    }
    fn visible_slots(&self) -> Option<(usize, usize)> {
        self.pane.visible_slots
    }
    fn viewport(&self) -> ViewportSnapshot {
        fake_viewport(self.pane)
    }
    fn history_paging(&self) -> bool {
        self.tab.history_paging
    }
    fn venue_record_starts_inside(&self, _interval_ms: i64) -> bool {
        self.pane.record_starts_inside
    }
    fn provenance(&self) -> BarProvenanceContext {
        BarProvenanceContext {
            engine_price: "exchange_trade".to_owned(),
            engine_volume: "exchange_trade".to_owned(),
            engine_side: "exchange_aggressor".to_owned(),
            venue_aggressor_split: false,
        }
    }
    fn layers(&self, _: &ChartStyle) -> Vec<LayerSnapshot> {
        self.pane
            .layers
            .iter()
            .map(|(layer, _)| layer.clone())
            .collect()
    }
    fn layer(&self, layer_id: &str, _: &ChartStyle) -> Option<LayerRead> {
        let (layer, blocked) = self
            .pane
            .layers
            .iter()
            .find(|(layer, _)| layer.id == layer_id)?;
        Some(LayerRead {
            layer: layer.clone(),
            blocked: *blocked,
        })
    }
}

/// A fake pane's framing: the slots a test laid out, the rest fixed.
fn fake_viewport(pane: &FakePane) -> ViewportSnapshot {
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

/// A fake tab has no pointer: a mark resolved against it names the tab and
/// its first pane and says why the pointer is missing.
impl MarkTab for FakeTab {
    fn cursor_snapshot(&self, tab_id: u64, _config: &AppConfig) -> CursorSnapshot {
        CursorSnapshot {
            active_tab_id: WireU64::new(tab_id),
            focused_pane_id: WireU64::new(self.panes.first().map_or(0, |pane| pane.id)),
            focused_pane_side: PaneSideDto::Flow,
            pointer: None,
            pointer_availability: unavailable("fake_window_has_no_pointer"),
            semantic_scene: unavailable("fake_window_has_no_pointer"),
        }
    }
}

impl crate::script_strategy::ArmedTab for FakeTab {
    fn armed_on(
        &self,
        pane_id: u64,
    ) -> Vec<quantick_session::strategy_reading::ArmedReading<quantick_drawings::DrawingId>> {
        let pane = self.panes.iter().find(|pane| pane.id == pane_id);
        pane.map_or_else(Vec::new, |pane| pane.armed.clone())
    }
}

impl AddressedPanePort for FakeWindow {
    type Pane = FakePane;
    fn addressed_pane(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
    ) -> Result<&mut FakePane, ControlError> {
        self.pane_mut(tab_id, pane_id)
    }
}

impl ChartStylePort for FakeWindow {
    fn apply_chart_settings(
        &mut self,
        input: ChartSettingsInput,
    ) -> Result<ChartSettingsResult, PresetError> {
        self.settings_asked.push(input);
        self.chart_settings.clone()
    }
    fn chart_style(&self) -> &ChartStyle {
        &self.style
    }
    fn switch_chart_layer(&mut self, tab_id: u64, pane_id: u64, layer_id: &str, visible: bool) {
        let Ok(pane) = self.pane_mut(WireU64::new(tab_id), WireU64::new(pane_id)) else {
            return;
        };
        let layer = pane
            .layers
            .iter_mut()
            .find(|(layer, _)| layer.id == layer_id);
        let Some((layer, _)) = layer else { return };
        layer.requested = visible;
        self.layer_switches
            .push((pane_id, layer_id.to_owned(), visible));
    }
    /// The switch is kept for the pane's tab, the market it shows; a repeat
    /// is no change.
    fn set_price_tag_progress(
        &mut self,
        tab: u64,
        pane: u64,
        on: bool,
    ) -> Result<bool, ControlError> {
        self.pane_mut(WireU64::new(tab), WireU64::new(pane))?;
        let was_on = !self.progress_off.contains(&tab);
        self.progress_off.retain(|off| *off != tab);
        if !on {
            self.progress_off.push(tab);
        }
        Ok(was_on != on)
    }
}

impl ChartPort for FakeWindow {
    fn price_tag_progress(&self, tab_id: u64) -> bool {
        !self.progress_off.contains(&tab_id)
    }
    fn visit_chart_panes(&self, visit: &mut dyn FnMut(ChartPaneEntry<'_>, PaneFraming)) {
        for (index, tab) in self.tabs.iter().enumerate() {
            for (slot, pane) in tab.panes.iter().enumerate() {
                let shown = index == self.active;
                let framing = PaneFraming {
                    visible: shown,
                    focused: shown && slot == 0,
                };
                let read = FakePaneRead { tab, pane };
                visit(entry(tab, slot, &read), framing);
            }
        }
    }
    fn visit_chart_pane(
        &self,
        tab_id: u64,
        pane_id: u64,
        visit: &mut dyn FnMut(ChartPaneEntry<'_>),
    ) -> Result<(), MissingPane> {
        let tab = self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .ok_or(MissingPane::Tab)?;
        let (slot, pane) = tab
            .panes
            .iter()
            .enumerate()
            .find(|(_, pane)| pane.id == pane_id)
            .ok_or(MissingPane::Pane)?;
        visit(entry(tab, slot, &FakePaneRead { tab, pane }));
        Ok(())
    }
}

fn entry<'a>(tab: &'a FakeTab, slot: usize, read: &'a FakePaneRead<'a>) -> ChartPaneEntry<'a> {
    let side = if slot == 0 {
        PaneSideDto::Flow
    } else {
        PaneSideDto::Time
    };
    ChartPaneEntry {
        tab_id: tab.id,
        side,
        pane_index: slot,
        feed_id: &tab.feed_id,
        symbol: &tab.symbol,
        pane: read,
        market_clock_ms: None,
    }
}
