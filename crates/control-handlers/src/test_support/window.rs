use super::*;

/// The fake window: tabs in screen order, the one on screen, and what each
/// attention lane received.
pub(crate) struct FakeWindow {
    pub tabs: Vec<FakeTab>,
    pub active: usize,
    pub popups: Vec<AgentPopup>,
    pub toasts: Vec<String>,
    pub speaker_refusal: Option<String>,
    /// The main-click default the last taken press set.
    pub default_reach: HistoryReach,
    /// The config the window lends its tabs.
    pub config: AppConfig,
    /// The tool rail's magnet.
    pub rail_magnet: bool,
    /// What the chart settings owner answers, and every input it was given.
    pub chart_settings: Result<ChartSettingsResult, PresetError>,
    pub settings_asked: Vec<ChartSettingsInput>,
    /// Every layer switch asked: pane, layer and visibility.
    pub layer_switches: Vec<(u64, String, bool)>,
    /// The window's shared chart style.
    pub style: ChartStyle,
    /// The record-by-default setting, once a call kept one.
    pub record_by_default: Option<bool>,
    /// The bound workspace's indicator owner.
    pub indicators: FakeIndicators,
    /// The tabs whose market has the price tag's progress switched off; each
    /// fake tab shows one market.
    pub progress_off: Vec<u64>,
}

/// A fake indicator owner: what each indicator and script call reached it
/// with, and the refusal it answers when a test sets one.
#[derive(Default)]
pub(crate) struct FakeIndicators {
    /// Every script attached: name, source, attached by an operator.
    pub attached: Vec<(String, String, bool)>,
    /// Every guide set: tab, pane, slot, on.
    pub guides: Vec<(u64, u64, u64, bool)>,
    /// Every library script added to the chart.
    pub added: Vec<String>,
    /// Every editor action run.
    pub actions: Vec<ScriptEditorAction>,
    /// Every native edit applied: slot, edit.
    pub applied: Vec<(Option<u64>, wire::Prepared)>,
    /// The saved native indicators every target reads.
    pub saved: Vec<(u64, SavedIndicator, bool)>,
    /// What the picker, guide, editor and `saved` calls answer instead of
    /// succeeding; attach, detach and `apply` keep their own rules.
    pub refusal: Option<IndicatorRefusal>,
}

impl FakeIndicators {
    fn refused(&self) -> Result<(), IndicatorRefusal> {
        self.refusal.clone().map_or(Ok(()), Err)
    }
}

impl IndicatorPickerPort for &mut FakeIndicators {
    fn indicator_picker(&mut self, input: PickerInput) -> Result<PickerResult, IndicatorRefusal> {
        self.refused()?;
        Ok(PickerResult {
            open: true,
            tab_id: None,
            pane_id: None,
            query: String::new(),
            category: String::new(),
            categories: Vec::new(),
            entries: Vec::new(),
            total: 0,
            next_offset: Some(input.offset),
        })
    }
}

impl ScriptsPort for &mut FakeIndicators {
    fn attach_script(&mut self, name: String, source: String, by_operator: bool) -> AttachedScript {
        self.attached.push((name, source, by_operator));
        AttachedScript {
            tab_id: 1,
            pane_side: PaneSideDto::Flow,
            slot: self.attached.len() as u64 - 1,
        }
    }
    fn detach_operator_script(&mut self, slot: u64) -> Result<bool, IndicatorRefusal> {
        let Some((_, _, by_operator)) = self.attached.get(slot as usize) else {
            return Ok(false);
        };
        if *by_operator {
            Ok(true)
        } else {
            Err(IndicatorRefusal::TradersOwn)
        }
    }
}

impl IndicatorGuidePort for &mut FakeIndicators {
    fn set_indicator_guide(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        slot: u64,
        enabled: bool,
    ) -> Result<(), IndicatorRefusal> {
        self.refused()?;
        self.guides.push((tab_id, pane_id, slot, enabled));
        Ok(())
    }
}

impl ScriptEditorPort for &mut FakeIndicators {
    fn set_script_numeric_input(
        &mut self,
        _id: &str,
        _name: &str,
        _value: NumericValue,
    ) -> Result<(), IndicatorRefusal> {
        self.refused()
    }
    fn add_library_script(&mut self, id: &str) -> Result<(), IndicatorRefusal> {
        self.refused()?;
        self.added.push(id.to_owned());
        Ok(())
    }
    fn script_editor(
        &mut self,
        action: ScriptEditorAction,
    ) -> Result<Option<ScriptDocument>, IndicatorRefusal> {
        self.refused()?;
        self.actions.push(action);
        Ok(None)
    }
    fn script_library_state(&self, _offset: usize, _limit: usize) -> EditorResult {
        EditorResult::default()
    }
}

impl NativePort for &mut FakeIndicators {
    fn saved(
        &mut self,
        _target: &wire::Target,
    ) -> Result<Vec<(u64, SavedIndicator, bool)>, IndicatorRefusal> {
        self.refused()?;
        Ok(self.saved.clone())
    }
    fn settings_open(&self) -> bool {
        false
    }
    fn apply(
        &mut self,
        _target: &wire::Target,
        slot: Option<u64>,
        edit: wire::Prepared,
    ) -> Result<u64, IndicatorRefusal> {
        self.applied.push((slot, edit));
        Ok(slot.unwrap_or(7))
    }
}

impl FakeWindow {
    pub fn new(tabs: Vec<FakeTab>) -> Self {
        Self {
            tabs,
            active: 0,
            popups: Vec::new(),
            toasts: Vec::new(),
            speaker_refusal: None,
            default_reach: FAKE_DEFAULT_REACH,
            config: fake_config(&[]),
            rail_magnet: false,
            chart_settings: Err(PresetError::Refused("the fake keeps no chart settings")),
            settings_asked: Vec::new(),
            layer_switches: Vec::new(),
            style: ChartStyle::default(),
            record_by_default: None,
            indicators: FakeIndicators::default(),
            progress_off: Vec::new(),
        }
    }

    /// The pane `pane_id` on the tab `tab_id`.
    pub(super) fn pane_mut(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
    ) -> Result<&mut FakePane, ControlError> {
        let tab = self.tabs.iter_mut().find(|tab| tab.id == tab_id.get());
        let tab = tab.ok_or_else(|| ControlError::invalid_request("no open tab has that id"))?;
        let pane = tab.panes.iter_mut().find(|pane| pane.id == pane_id.get());
        pane.ok_or_else(|| ControlError::invalid_request("the call names an unknown pane"))
    }

    /// The actor every fake call is made as.
    pub fn assistant() -> ActorContext {
        use quantick_control::limits::CONTROL_RUNTIME_ID_BYTES;
        ActorContext {
            actor_kind: ActorKind::Agent,
            principal_id: PrincipalId::from_bytes([1; CONTROL_RUNTIME_ID_BYTES]),
            client_name: "fake assistant".to_owned(),
            connection_id: ConnectionId::from_bytes([2; CONTROL_RUNTIME_ID_BYTES]),
            request_id: RequestId::new("fake-1").expect("a literal request id is valid"),
            reason: None,
            requested_at_unix_ms: 0,
        }
    }

    /// The trader's own hand on the same door: a call the window makes for a
    /// human gesture.
    pub fn trader() -> ActorContext {
        ActorContext {
            actor_kind: ActorKind::HumanUi,
            client_name: "fake trader".to_owned(),
            ..Self::assistant()
        }
    }
}

impl TabDirectory for FakeWindow {
    fn active_tab_index(&self) -> usize {
        self.active
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == tab_id)
    }
    fn tab_id_at(&self, index: usize) -> u64 {
        self.tabs[index].id
    }
}

impl PaneBubblesPort for FakeWindow {
    type Theme = ();
    fn bubble_theme(&self) {}
    fn pane_bubbles(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
    ) -> Result<&mut dyn BubblePane<()>, ControlError> {
        Ok(self.pane_mut(tab_id, pane_id)?)
    }
}

impl IndicatorControlHost for FakeWindow {
    fn indicators(&mut self) -> Box<dyn IndicatorOwner + '_> {
        Box::new(&mut self.indicators)
    }
}

impl MarketTabHost for FakeWindow {
    fn market_tab_mut(&mut self, index: usize) -> Option<LentTab<'_>> {
        Some(LentTab {
            tab: self.tabs.get_mut(index)?,
            config: &self.config,
            history_default: &mut self.default_reach,
        })
    }
    fn drawing_gesture_on(&self, _feed_id: &str, _from: &str, _to: &str) -> bool {
        false
    }
    fn carry_market_drawings(&mut self, _feed_id: &str, _from: &str, _to: &str) -> usize {
        0
    }
}

/// The fake window's desk: its tabs' recorders and the record-by-default row.
struct FakeDesk<'a> {
    tabs: &'a mut Vec<FakeTab>,
    record_by_default: &'a mut Option<bool>,
}

impl crate::desk::DeskHost for FakeWindow {
    fn desk(&mut self) -> Option<Box<dyn crate::desk::DeskOwner + '_>> {
        let (tabs, record_by_default) = (&mut self.tabs, &mut self.record_by_default);
        Some(Box::new(FakeDesk {
            tabs,
            record_by_default,
        }))
    }
}

impl DealRecordingPort for FakeDesk<'_> {
    fn deal_recorder(&mut self, index: usize) -> Option<&mut dyn DealRecorder> {
        Some(self.tabs.get_mut(index)?)
    }
}

impl SettingsOwner for FakeDesk<'_> {
    fn readings(&self) -> SettingsReadings {
        SettingsReadings {
            record_deals: self.record_by_default.unwrap_or_default(),
            ..SettingsReadings::default()
        }
    }
    fn settle(&mut self, act: SettingsAct) -> Result<(), SettleError> {
        if let SettingsAct::RecordDeals(on) = act {
            *self.record_by_default = Some(on);
        }
        Ok(())
    }
}

impl crate::desk::DeskOwner for FakeDesk<'_> {
    fn act(&mut self, _act: crate::desk::DeskAct) -> bool {
        false
    }
}

/// The fake window's attention lanes are its own fields, lent for the call.
impl AttentionHost for FakeWindow {
    fn attention(&mut self) -> Box<dyn Attention + '_> {
        Box::new(self)
    }
}

impl Attention for &mut FakeWindow {
    fn show_popup(&mut self, popup: AgentPopup) {
        self.popups.push(popup);
    }
    fn show_toast(&mut self, message: String) {
        self.toasts.push(message);
    }
    fn sound_alert(&mut self) -> Option<String> {
        self.speaker_refusal.clone()
    }
}

/// The fake window keeps no paper desk: every trade action finds no
/// chart open.
impl PaperHost for FakeWindow {
    fn paper(&mut self) -> Box<dyn PaperOwner + '_> {
        Box::new(NoDesk)
    }
}
