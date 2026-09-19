//! A headless test double for the control port: one tab over silent feed
//! channels, default chrome, and recorders for the assistant's lanes.
//!
//! It implements the families a handler can be driven through without the
//! window — tabs, chrome, health, alerts, scripts and the deal-recording
//! default — and deliberately not the layout, paper, layer or gateway
//! families, which hand out the window's own owners. A handler whose
//! signature names only the implemented families runs against it; one that
//! names more does not compile against it, which is the port's point.

use quantick_control::wire::{ActorContext, ActorKind};
use quantick_feed as feed;
use tokio::sync::mpsc;

use crate::config::{AppConfig, FeedConfig, ProviderKind};
use crate::dock::Dock;
use crate::style::ChartStyle;
use crate::tab::Tab;
use crate::timezone::TzOffset;
use crate::toolrail::ToolRail;

use super::super::super::arrangement_host::ArrangementHost;
use super::super::{
    Alerts, AlertsPort, ChromeDrawn, ChromePort, ChromeReads, HealthPort, HealthReads,
    RecordingPort, ScriptSlot, ScriptsPort, TabReads, TabsMut, TabsMutPort, TabsPort,
};

/// The feed and symbol the fake's one tab shows.
pub(crate) const FAKE_FEED_ID: &str = "binance";
pub(crate) const FAKE_SYMBOL: &str = "TESTUSDT";

/// A window with one tab and nothing drawn, whose lanes record what the
/// handler asked for.
pub(crate) struct FakeWindow {
    tabs: ArrangementHost,
    config: AppConfig,
    footprint_config: crate::footprint_config::FootprintConfig,
    presets: crate::drawings::presets::PresetStore,
    style: ChartStyle,
    tool_rail: ToolRail,
    dock: Dock,
    drawing_chrome: crate::surfaces::drawing_chrome::DrawingChromeSurface,
    health: super::super::super::health::HealthCounters,
    history: super::super::super::tabs::HistorySettings,
    /// The assistant's popup lane, read back by the test.
    pub(crate) agent_popup: crate::surfaces::AgentPopupSurface,
    /// The acknowledgement toast, read back by the test.
    pub(crate) toast: crate::surfaces::ToastSurface,
    audio: super::super::super::replay_and_history::AlertState,
    /// Every script attached: name, source, attached by an operator.
    pub(crate) attached: Vec<(String, String, bool)>,
    /// The deal-recording default last set.
    pub(crate) record_deals: Option<bool>,
}

impl FakeWindow {
    pub(crate) fn new() -> Self {
        let (_events_tx, events) = mpsc::channel(8);
        let (_book_tx, book_events) = mpsc::channel(8);
        let (commands, _commands_rx) = mpsc::channel(8);
        let tab = Tab::new(
            0,
            FAKE_FEED_ID.to_owned(),
            FAKE_SYMBOL.to_owned(),
            crate::state::BarSpec::Tick(50),
            quantick_feed::FeedHandle {
                events,
                book_events,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                latency: feed::unsplit_latency(),
                commands,
                replay: None,
            },
            crate::scratch::thread_dir("fake-window"),
        );
        let scratch = crate::scratch::thread_dir("fake-window-presets");
        Self {
            tabs: ArrangementHost::new(quantick_workspace::arrangement::TabId(1), tab),
            config: AppConfig {
                default_feed: FAKE_FEED_ID.to_owned(),
                default_symbol: FAKE_SYMBOL.to_owned(),
                feeds: vec![FeedConfig {
                    id: FAKE_FEED_ID.to_owned(),
                    name: "Binance".to_owned(),
                    provider: ProviderKind::Binance,
                    symbols: vec![FAKE_SYMBOL.to_owned()],
                    bubble_preset: None,
                    symbol_bubble_presets: Default::default(),
                    default_layout: None,
                    default_bars: None,
                    record_deals: false,
                }],
                metatrader: Default::default(),
                paper: Default::default(),
                deals: Default::default(),
                history: Default::default(),
            },
            footprint_config: crate::footprint_config::FootprintConfig::default(),
            presets: crate::drawings::presets::PresetStore::load_from(
                scratch.join("drawing-presets.toml"),
            ),
            style: ChartStyle::default(),
            tool_rail: ToolRail::default(),
            dock: Dock::new(),
            drawing_chrome: Default::default(),
            health: super::super::super::health::HealthCounters::new(),
            history: super::super::super::tabs::HistorySettings {
                progressive_history: true,
                history_reach: Default::default(),
                history_reach_span_minutes: 60,
                venue_lead_in: false,
            },
            agent_popup: Default::default(),
            toast: Default::default(),
            audio: super::super::super::replay_and_history::AlertState {
                alerts: Box::new(FakeSpeaker(None)),
                alert_failure: None,
            },
            attached: Vec::new(),
            record_deals: None,
        }
    }

    /// A window whose speaker refuses with `reason`.
    pub(crate) fn with_refusing_speaker(reason: &'static str) -> Self {
        let mut window = Self::new();
        window.audio.alerts = Box::new(FakeSpeaker(Some(reason)));
        window
    }

    /// The fake assistant every handler test acts as.
    pub(crate) fn assistant() -> ActorContext {
        use quantick_control::id::{ConnectionId, PrincipalId, RequestId};
        ActorContext {
            actor_kind: ActorKind::Agent,
            principal_id: PrincipalId::from_bytes(
                [1; quantick_control::limits::CONTROL_RUNTIME_ID_BYTES],
            ),
            client_name: "fake assistant".to_owned(),
            connection_id: ConnectionId::from_bytes(
                [2; quantick_control::limits::CONTROL_RUNTIME_ID_BYTES],
            ),
            request_id: RequestId::new("fake-1").expect("a literal request id is valid"),
            reason: None,
            requested_at_unix_ms: 0,
        }
    }
}

impl TabsPort for FakeWindow {
    fn tab_reads(&self) -> TabReads<'_> {
        TabReads::new(
            &self.tabs,
            &self.config,
            &self.footprint_config,
            &self.presets,
        )
    }
}

impl TabsMutPort for FakeWindow {
    fn tabs_mut(&mut self) -> TabsMut<'_> {
        TabsMut::new(&mut self.tabs, &self.config)
    }
}

impl ChromePort for FakeWindow {
    fn chrome_reads(&self) -> ChromeReads<'_> {
        ChromeReads::new(
            &self.tabs,
            &self.style,
            &self.tool_rail,
            &self.dock,
            TzOffset::default(),
            &self.drawing_chrome,
            ChromeDrawn::default(),
        )
    }
}

impl HealthPort for FakeWindow {
    fn health_reads(&self) -> HealthReads<'_> {
        HealthReads::new(false, &self.health, &self.history)
    }
}

impl AlertsPort for FakeWindow {
    fn alerts(&mut self) -> Alerts<'_> {
        Alerts::new(&mut self.agent_popup, &mut self.toast, &mut self.audio)
    }
}

/// A speaker that refuses with a fixed reason, or plays when it has none.
struct FakeSpeaker(Option<&'static str>);

impl crate::audio::AlertSink for FakeSpeaker {
    fn play(&mut self, _cues: &[crate::audio::Cue]) -> Result<(), &'static str> {
        self.0.map_or(Ok(()), Err)
    }
}

impl ScriptsPort for FakeWindow {
    fn attach_script(&mut self, name: String, source: String, by_operator: bool) -> ScriptSlot {
        self.attached.push((name, source, by_operator));
        let slot = self.attached.len() as u64 - 1;
        (
            1,
            crate::pane::PaneSide::Flow,
            crate::indicator_worker::SlotId(slot),
        )
    }

    fn detach_operator_script(&mut self, slot: u64) -> Result<bool, ()> {
        Ok((slot as usize) < self.attached.len())
    }
}

impl RecordingPort for FakeWindow {
    fn set_deal_recording_default(&mut self, enabled: bool) {
        self.record_deals = Some(enabled);
    }
}
