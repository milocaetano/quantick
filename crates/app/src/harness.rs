//! The harness port — where an agent's grip on the application docks.
//!
//! A *harness hook* here is an environment variable read once at launch that
//! puts the window into a state a hand would otherwise have to click it into:
//! a menu open, a pointer parked over the candles, a drawing half-placed
//! between two clicks, a page of older history asked for. It is how
//! `ui-harness` — and through it `visual-qa` and `trader-ux-review` — sees the
//! application at all. None of it is state the chart trades on.
//!
//! Before this port each hook was wired into `QuantickApp` by hand in three
//! places: a field, a line in the constructor's struct literal, and a
//! `std::env::var` call somewhere in the eight hundred lines of boot that
//! follow it. Twenty-three of the trunk's ninety-eight fields were that
//! wiring, sitting in the same struct as the state the chart does trade on, so
//! every module that touched the trunk saw them. `arch-review` dimension 9 is
//! the review half of that story and `crates/guards/src/size.rs` the
//! mechanical half; this module is the place the wiring goes instead.
//!
//! # Shape, and why it copies the dock
//!
//! [`Harness`] is deliberately the same shape `dock.rs` and
//! `surfaces::Surfaces` already use here: **a typed struct that names its
//! members**, built by one read of the environment, which the host *asks*
//! rather than reads. The trunk holds one field where it held twenty-three,
//! and it never touches a field of this struct — only the named methods
//! below, each of which says what the hook is for rather than what it is
//! made of.
//!
//! Two things follow from copying a pattern that already works here rather
//! than inventing a second one. A hook never reaches back into `QuantickApp`,
//! so it cannot grow a dependency on the trunk — everything here parses
//! strings and counts frames, which is why the whole module is testable
//! without a window. And **every multi-valued hook is a struct with
//! defaulting fields, not an enum**: [`DrawingsDemo`], [`FrvpDemo`],
//! [`DrawingDraft`] and [`HookFrame`] all grow a new option as an added field
//! that defaults to "did not ask", never as a `match` arm that reopens every
//! existing caller. That is the failure mode dimension 9 names in
//! `ChartLayer`'s 21 variants across 264 call sites, and it is why
//! `QUANTICK_DRAWINGS_DEMO_SHARED`, `_SELECT` and `QUANTICK_DRAWING_CONSTRAIN`
//! are fields here instead of the mid-frame `std::env::var` calls they were.
//!
//! The four enums that remain — [`ScriptedMenu`], [`ContextMenuPane`],
//! [`VenueHistoryDemo`], [`StrategyDemoMode`] — are hook *values*, not
//! response shapes: each names a set of mutually exclusive scenes, each is
//! matched at exactly one site, and adding a scene to one of them is the arm
//! that scene needs rather than a call site anybody else has to revisit.
//!
//! # Why the registry is a typed struct, not a map of strings
//!
//! [`Harness`] names its members as fields, the way [`crate::surfaces::Surfaces`]
//! does. A `HashMap<String, String>` of raw values would let a hook be added
//! without being named here, which sounds stronger until the host needs the
//! *parsed* thing — a pointer fraction, a settings tab, a campaign ending —
//! and every reader has to re-parse it, differently, at the moment it is
//! needed. That is precisely what this module replaced.
//!
//! So the trade is made in the open: adding a hook edits this file, on
//! purpose. What matters is that the edit is one field, one parse and one
//! accessor in a file of this size, instead of three edits spread through a
//! nine-thousand-line trunk — and that the trunk's own line stays a single
//! call that reads like the thing it asks for.
//!
//! # Adding a hook
//!
//! One field on [`Harness`] (or one defaulting field on the response struct
//! of a hook that already exists), one line in [`Harness::from_env`], one
//! accessor, and a row in `.claude/skills/ui-harness/references/hook-registry.md`.
//! A hook that belongs to a surface rather than to the window parses itself
//! beside that surface instead — `surfaces::drawing_chrome::apply_launch_hooks`
//! is the pattern — and the registry row is owed either way.

use eframe::egui;

use crate::history_reach::CampaignEnd;
use crate::indicator_panel::SettingsTab;

/// Frames the `QUANTICK_LOAD_OLDER` hook waits for a chart worth paging from.
///
/// It cannot fire at startup: paging asks for trades older than the ones on
/// screen, and at launch there are none — the feed would refuse the request as
/// `nothing_charted_yet`. So the hook waits for the first block to land, and
/// gives up rather than hanging a capture run on a bridge that never connects.
/// About ten seconds at 60 fps, which is longer than any bridge takes to say
/// hello and shorter than a person's patience with a frozen window.
pub(crate) const LOAD_OLDER_HOOK_FRAMES: u32 = 600;

/// Frames the `QUANTICK_HISTORY_NOTE` hook holds its sentence on screen for.
///
/// A *hold*, not a wait — which is why it is not
/// [`LOAD_OLDER_HOOK_FRAMES`]: that one is sized against how long a bridge
/// takes to say hello, and retuning it for bridge latency must not silently
/// retune how long a capture has to photograph a note.
///
/// Comfortably past `tab::HISTORY_NOTE_LINGER`, so a run that waits out a
/// slow source still finds the sentence up; the note keeps its ordinary
/// linger from the last raise once the hold ends, so even a hooked run
/// photographs a note that expires.
pub(crate) const HISTORY_NOTE_HOOK_FRAMES: u32 = 900;

/// Frames the `QUANTICK_LOAD_OLDER_CANDLES` hook has for the *whole* run.
///
/// Much larger than the trade twin's, and for a reason the trade twin does
/// not have: a page of prints is one venue round trip, while a span of
/// candles is several slices of several pages each, and the hook is
/// documented as reaching the old ninety-day default in thirteen of them.
/// Every frame spent waiting for a span costs one tick here, so the budget
/// has to cover the legitimate fetching as well as the hang it exists to
/// bound. About a minute at 60 fps: longer than thirteen spans take against
/// a venue that is answering, and far shorter than a capture run's patience
/// with one that is not.
pub(crate) const LOAD_OLDER_CANDLES_HOOK_FRAMES: u32 = 3_600;

/// How long `QUANTICK_CONTROL_EVIDENCE=screenshot` waits for the window to
/// hand over a rasterised frame.
///
/// A window that presents answers on the frame after the request; a headless
/// or occluded one never does. About two seconds at 60 fps: long enough for a
/// surface that is coming up, short enough that a capture run gets a bundle
/// with an honest gap instead of waiting for one that will never arrive.
pub(crate) const CONTROL_EVIDENCE_HOOK_FRAMES: u32 = 120;

/// What `QUANTICK_STRATEGY_DEMO` stages: the armed instance itself, or the
/// arming dialog a screenshot of the form needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StrategyDemoMode {
    Armed,
    Popup,
    /// The arming dialog with the **alarm switched on** and its share gate
    /// picked, so every alarm control is on screen at once. The section
    /// folds itself away while the checkbox is clear — which is right for a
    /// trader and useless for a capture — so `popup` alone can never
    /// photograph it.
    AlarmPopup,
    /// The same dialog with the **sound picker dropped open** — the three
    /// headings and the thirty-two names under them. A combo box's list
    /// exists only while it is open, and opening it is a click no scripted
    /// run has a hand for, so the scene asks egui to open it on the frame
    /// the dialog appears.
    AlarmSounds,
    /// An armed instance whose region's drawn span no longer reaches the
    /// next bar — the badge clause telling the trader to stretch it right.
    /// A live tape reaches it only by walking past a hand-drawn right edge,
    /// which is minutes of market no capture can wait for. The instance
    /// stays armed and its alarm stays live: the region holds, it is not
    /// disarmed, so dragging the band forward resumes it with no button.
    EndedBadge,
    /// An instance whose region lost its footing on the series — the badge
    /// clause that says the bot is paused and why. Reached on a real chart
    /// only by a re-cut that strands an anchor, which a scripted run has no
    /// way to provoke on demand.
    PausedBadge,
    /// An **alarm-only** instance armed on the region, carrying a standing
    /// preview mark: the badge that says "this places nothing" and the
    /// provisional label, in one frame. Both are states a real tape reaches
    /// only when a force bar happens to be half-formed, which no capture
    /// can wait for.
    AlarmBadge,
}

impl StrategyDemoMode {
    /// Read one off `QUANTICK_STRATEGY_DEMO`; `None` for anything else, so a
    /// typo stages nothing rather than the wrong scene.
    fn from_token(token: &str) -> Option<Self> {
        match token.trim() {
            "1" | "armed" => Some(Self::Armed),
            "popup" => Some(Self::Popup),
            "alarm" => Some(Self::AlarmPopup),
            "alarm-sounds" => Some(Self::AlarmSounds),
            "alarm-badge" => Some(Self::AlarmBadge),
            "ended-badge" => Some(Self::EndedBadge),
            "paused-badge" => Some(Self::PausedBadge),
            _ => None,
        }
    }
}

/// Which pane a scripted right-click should land on.
///
/// The two panes now open different menus, so "open the context menu" is no
/// longer one instruction — a capture has to say which canvas it is asking
/// about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextMenuPane {
    /// The candles, left of the divider.
    Chart,
    /// The rolling tape, right of it.
    Tape,
    /// The price gutter — the axis's own menu (Inverted chart, and the
    /// compass's price half).
    Axis,
    /// The bottom time strip — the time axis's own menu (the compass's time
    /// half). The candles' segment of it: past the lane divider the strip is
    /// the tape's window and carries no menu.
    Time,
}

impl ContextMenuPane {
    /// Read one off `QUANTICK_CONTEXT_MENU`; `None` for anything else, so a
    /// typo opens no menu rather than the wrong one.
    pub(crate) fn from_env_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chart" | "candles" => Some(Self::Chart),
            "tape" | "lane" => Some(Self::Tape),
            "axis" | "scale" => Some(Self::Axis),
            "time" | "clock" => Some(Self::Time),
            _ => None,
        }
    }
}

/// Which venue-history frame `QUANTICK_VENUE_HISTORY_DEMO` opens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VenueHistoryDemo {
    /// The finished prefix: the seam divider with no wait behind it.
    Complete,
    /// A run still arriving: prefix installed, loading indicator still up.
    Partial,
}

impl VenueHistoryDemo {
    fn from_token(token: &str) -> Option<Self> {
        match token.trim() {
            "1" => Some(Self::Complete),
            "partial" => Some(Self::Partial),
            _ => None,
        }
    }
}

/// Which menu `QUANTICK_MENU` presses open.
///
/// A menu bar button is not something a scripted run can click, and every
/// entry behind one is therefore invisible to a capture without a hook. The
/// hook delivers a real click on the button's own published rectangle rather
/// than reaching into egui's popup state, so what opens is exactly what a
/// trader's click opens.
///
/// A token this build does not know opens nothing rather than the wrong menu:
/// a capture of the wrong surface that passes is worse than one that fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptedMenu {
    /// The Workspace menu — the only door to save, export, open and locate.
    Workspace,
    /// The toolbar's history caret — the reach chips, the span the `by time`
    /// reach pulls, the page size and the candle reach.
    History,
}

impl ScriptedMenu {
    /// Every menu this hook can open, by the token that names it.
    const ALL: [(&'static str, Self); 2] =
        [("workspace", Self::Workspace), ("history", Self::History)];

    fn from_token(token: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(token))
            .map(|(_, menu)| menu)
    }
}

/// What the `QUANTICK_DRAWINGS_DEMO` family asks the drawings demo for.
///
/// A struct rather than an enum of scenes, and this is the hook the rule was
/// written for: the demo already carries three independent switches
/// (`_SHARED`, `_SELECT`, and the `bands` spelling of the main variable), and
/// each of them used to be its own `std::env::var` call read halfway through
/// the applier. Every future one is another field defaulting to "did not
/// ask", visible to a reader of this file and to nobody else.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct DrawingsDemo {
    /// `QUANTICK_DRAWINGS_DEMO=bands`: a band set on every indicator pane as
    /// well. `=1` stays exactly what it was, so every screenshot taken of the
    /// old hook still is.
    pub bands: bool,
    /// `QUANTICK_DRAWINGS_DEMO_SHARED=1`: open the split and share a drawing
    /// across it — a shared object has nothing to be shared *with* on one
    /// pane.
    pub shared: bool,
    /// `QUANTICK_DRAWINGS_DEMO_SELECT=<tool id>`: which object ends up
    /// selected. Selection is what puts an object's handles on screen, so
    /// "show me the channel's handles" is a question no screenshot could
    /// answer while only the last-placed tool was ever selected.
    pub select_tool: Option<String>,
}

/// What the `QUANTICK_FRVP_DEMO` family asks the fixed-range profile demo for.
///
/// Same shape and same reason as [`DrawingsDemo`]: `compare` and `stress` are
/// spellings of the main variable, `select` is a satellite of its own, and a
/// fourth scene is a fourth field.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrvpDemo {
    /// `=compare`: two adjacent profiles over the same stretch of map, one in
    /// each over-heatmap mode.
    pub compare: bool,
    /// `=stress`: a venue history longer than any single fold pass, with one
    /// profile over the whole of it.
    pub stress: bool,
    /// `QUANTICK_FRVP_DEMO_SELECT=1`: leave the profile selected, so the strip
    /// a trader edits it from is on screen too.
    pub select: bool,
}

/// What `QUANTICK_DRAWING_DRAFT` asks for: the half-placed state that lives
/// between two clicks, and how the parked hand is constrained while it waits.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrawingDraft {
    /// How many anchors of the armed tool are already down when the run opens.
    pub anchors: usize,
    /// `QUANTICK_DRAWING_CONSTRAIN=1`: the parked hand holds a level, as a
    /// held modifier would.
    pub constrain: bool,
}

/// What one frame's tick of a budgeted hook came to.
///
/// A struct with defaulting fields, for the reason this whole module states:
/// a budgeted hook that later needs to report a third thing gains a field
/// here that defaults to `false`, and every existing caller keeps compiling
/// and keeps meaning what it meant.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HookFrame {
    /// This frame spent the last of the budget, and the hook has disarmed
    /// itself. The trunk logs its own give-up sentence: only it knows what the
    /// chart was holding at the time.
    pub gave_up: bool,
}

/// A hook that owes some number of things and has a frame budget to wait in.
///
/// Three hooks share this shape — `QUANTICK_LOAD_OLDER`,
/// `QUANTICK_LOAD_OLDER_CANDLES` and `QUANTICK_HISTORY_NOTE` — and shared it
/// as three hand-written copies of `Option<(T, u32)>` before this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Budgeted<T> {
    /// What is still owed: pages, spans, or the ending whose sentence is held.
    owed: T,
    /// Frames left to wait for the chart to be ready for it.
    frames: u32,
}

impl<T> Budgeted<T> {
    /// Spend one frame of budget. `None` means the budget just ran out and the
    /// caller should disarm.
    fn spend_frame(mut self) -> Option<Self> {
        self.frames = self.frames.checked_sub(1)?;
        Some(self)
    }
}

/// Every environment hook the window itself owns, read once and named.
///
/// The trunk holds exactly one of these. See the module documentation for why
/// it is a typed struct the host asks rather than a bag of flags the host
/// holds.
///
/// [`Default`] is derived rather than written, and that is what keeps this
/// module's promise honest: adding a hook is **one field and one line in
/// [`Harness::from_env`]**, with nothing else in the file to keep in step. A
/// hand-written "every hook unset" constructor was a third list saying the
/// same thing, and a third list is where the next hook gets forgotten.
/// It is **gated to test builds**, though. `Harness::default()` is every hook
/// unset — a harness that read no environment — and in production that is
/// indistinguishable at the call site from `from_env`, so a future edit
/// reaching for the shorter name would disarm every hook with no compile
/// error. Tests get it; the trunk has exactly one way to build a harness.
#[cfg_attr(test, derive(Default))]
pub(crate) struct Harness {
    /// `QUANTICK_LAYOUT_PICKER`: a pending request to open the toolbar's
    /// layout popover. Drained by the frame that honours it — the popover is a
    /// popup egui owns, so the hook asks for it through the same call the
    /// click makes rather than faking the surface.
    layout_picker_autostart: bool,
    /// `QUANTICK_DEAL_RECORDING`: `on`/`off` override the default a tab's
    /// deal recorder opens on; `menu` opens the REC popover once.
    deal_recording: Option<crate::deal_recording::RecordingHook>,
    /// `QUANTICK_WINDOW_MAXIMIZED`: maximise on the first frame that has a
    /// window to maximise.
    maximize: bool,
    /// `QUANTICK_FOOTPRINT_AUTOSTART`, kept rather than applied and forgotten,
    /// so tabs opened later (a replay autostart) get it too.
    footprint: bool,
    /// `QUANTICK_CANDLE_WIDTH`, in pixels per bar, re-applied every frame —
    /// see `QuantickApp::apply_scripted_view` for why once at boot is not
    /// enough.
    candle_width: Option<f32>,
    /// `QUANTICK_PAN_PX`: a drag on the candles, in pixels, applied every
    /// frame until it lands. Negative pushes the chart left into the
    /// projection margin; positive walks back into history.
    pan_px: Option<f32>,
    /// `QUANTICK_INDICATOR_SETTINGS=1`: open the settings dialog for the first
    /// indicator once its inputs have arrived from the worker. Armed at boot,
    /// fired (and disarmed) by the first frame that can honour it.
    ///
    /// **This and [`Self::settings_autostart`] read the same variable and do
    /// not agree about `=1`**: this one means "the first indicator", while the
    /// index parser below reads `1` as an index and means the *second*. Both
    /// then arm, and the later of the two writes wins, so a run that sets `=1`
    /// with two indicators loaded photographs the second one's dialog while
    /// this line promises the first. That is how the hook behaved before this
    /// module existed and it is left alone here, because collecting the hooks
    /// was not licensed to change what any of them does; the move is what
    /// makes the disagreement visible on two adjacent lines for the first
    /// time. Reconciling it is a change to what the hook accepts, and belongs
    /// to a change that says so.
    indicator_settings_dialog: bool,
    /// `QUANTICK_INDICATOR_SETTINGS=<index>[:<tab>]`: which indicator to open
    /// the dialog on, and on which tab, once its view exists. Cleared by the
    /// first open, so a dialog the run then closes stays closed.
    settings_autostart: Option<(usize, SettingsTab)>,
    /// `QUANTICK_POINTER`: where to park the mouse, as a fraction of the flow
    /// pane's candle area. Re-delivered every frame, because a pointer is a
    /// position the app is *told* about continuously.
    pointer: Option<egui::Vec2>,
    /// `QUANTICK_CONTEXT_MENU`: which pane a scripted run asked to
    /// right-click, until the click lands.
    context_menu: Option<ContextMenuPane>,
    /// That press's matching release, on the frame after it.
    context_menu_release: Option<egui::Pos2>,
    /// `QUANTICK_MENU`: which menu bar button to press open.
    menu: Option<ScriptedMenu>,
    /// That press's matching release, on the frame after it.
    menu_release: Option<egui::Pos2>,
    /// `QUANTICK_DRAWINGS_DEMO`: one of every registered drawing on the flow
    /// pane as soon as it has bars to anchor them to. Consumed once.
    drawings_demo: Option<DrawingsDemo>,
    /// `QUANTICK_DRAWINGS_DEMO_RECUT`: the re-cut scene — objects still on
    /// their own instants, and one the new series cannot reach.
    drawings_demo_recut: bool,
    /// `QUANTICK_FRVP_DEMO`: one fixed-range volume profile, placed to
    /// straddle the venue-prefix seam when there is one.
    frvp_demo: Option<FrvpDemo>,
    /// `QUANTICK_AVWAP_DEMO`: one anchored VWAP placed on the flow pane once
    /// it has bars — the band stack and anchor marker, photographable from a
    /// fresh launch. Consumed once, like the other demos.
    avwap_demo: bool,
    /// `QUANTICK_VENUE_HISTORY_DEMO`: a venue candle prefix delivered to the
    /// focused tab through the feed's own path, so the seam divider — and,
    /// with `=partial`, a run still arriving — can be photographed from a
    /// fresh launch.
    ///
    /// The seam only exists where venue candles meet bars cut from prints,
    /// which on a live feed means waiting on a real venue for a real quarter
    /// of history. That is not a state a scripted capture can reach, and
    /// "arrived halfway" is not a state it can reach *at all*: it lasts a few
    /// seconds, once, at a moment nothing controls. Consumed once.
    venue_history_demo: Option<VenueHistoryDemo>,
    /// `QUANTICK_STRATEGY_DEMO`: rectangle + armed instance (`1`) or the
    /// arming dialog over it (`popup`), for validation runs. Consumed once
    /// the chart has bars enough, like the drawings demo.
    strategy_demo: Option<StrategyDemoMode>,
    /// `QUANTICK_REPLAY_RESTART_AFTER=<n>`: take the replay transport's own
    /// Restart once the session has closed `n` round trips.
    ///
    /// The one state where a closed-trade mark is asked to paint against a
    /// tape that has not reached its fill: the seek keeps the trades (they
    /// happened) and rebuilds the bars under them. It is reached in the app
    /// by pressing Restart mid-session, which a scripted capture cannot do,
    /// and it is the state the marks used to pile up in. Consumed once.
    replay_restart: Option<usize>,
    /// `QUANTICK_DRAWING_DRAFT`: how many anchors of the armed tool are
    /// already placed when the run opens, with the pointer parked where the
    /// next one would go. Consumed once.
    drawing_draft: Option<DrawingDraft>,
    /// `QUANTICK_LOAD_OLDER`: pages of older *trades* still owed, and the
    /// frame budget left to wait for a chart to ask from.
    load_older: Option<Budgeted<usize>>,
    /// `QUANTICK_LOAD_OLDER_CANDLES`: spans of older *candles* still owed, and
    /// the frame budget left to wait for a first reply to reach back from. The
    /// trade twin of this is [`Self::load_older`]; they are two records with
    /// two capabilities, so they are two hooks.
    load_older_candles: Option<Budgeted<usize>>,
    /// `QUANTICK_HISTORY_NOTE`: the ending whose sentence the loading lane is
    /// asked to carry, and the frame budget left to hold it up.
    ///
    /// A settled reach speaks only when a real venue really refuses, which is
    /// a market condition and not a setting: on the feeds a validation run can
    /// arrange, the reach either lands its session or the source declares it
    /// cannot page at all and the button never takes a press. Without this the
    /// whole surface is invisible to anything but a bad afternoon.
    history_note: Option<Budgeted<CampaignEnd>>,
    /// Frames `QUANTICK_CONTROL_EVIDENCE=screenshot` has spent waiting for a
    /// rasterised window.
    ///
    /// The counter is the harness's; the request it counts for belongs to the
    /// control plane and stays on the trunk with the rest of that cluster.
    evidence_frames: u32,
}

impl Harness {
    /// Read every hook this module owns, once.
    ///
    /// One read at one moment, rather than a `std::env::var` wherever a value
    /// happens to be wanted: a hook re-read halfway through a frame is a hook
    /// whose meaning depends on when it is asked, which is not something a
    /// capture run can reason about.
    ///
    /// Everything here fails soft. A value that does not parse leaves its hook
    /// unset rather than panicking or guessing, because a validation run that
    /// photographed an invented state and called it a pass is worse than one
    /// that photographed nothing. The two hooks that name something from a
    /// registry — `QUANTICK_HISTORY_NOTE` here, `QUANTICK_HISTORY_REACH` on
    /// the trunk — say so out loud in the log instead of failing silently.
    pub(crate) fn from_env() -> Self {
        Self {
            layout_picker_autostart: flag("QUANTICK_LAYOUT_PICKER"),
            deal_recording: crate::deal_recording::RecordingHook::parse(
                std::env::var(crate::deal_recording::RECORDING_HOOK_ENV)
                    .ok()
                    .as_deref(),
            ),
            maximize: flag("QUANTICK_WINDOW_MAXIMIZED"),
            footprint: flag("QUANTICK_FOOTPRINT_AUTOSTART"),
            candle_width: read("QUANTICK_CANDLE_WIDTH")
                .and_then(|value| value.trim().parse::<f32>().ok()),
            pan_px: read("QUANTICK_PAN_PX")
                .and_then(|value| value.trim().parse::<f32>().ok())
                .filter(|px| px.is_finite()),
            indicator_settings_dialog: read("QUANTICK_INDICATOR_SETTINGS")
                .is_some_and(|value| value == "1"),
            settings_autostart: read("QUANTICK_INDICATOR_SETTINGS")
                .and_then(|value| parse_settings_hook(&value)),
            pointer: read("QUANTICK_POINTER").and_then(|value| {
                let parsed = parse_pointer_fraction(&value);
                if parsed.is_none() {
                    tracing::warn!(
                        target: "quantick",
                        value = %value,
                        "POINTER_HOOK_REJECTED: expected <fx>,<fy> with both in 0..=1"
                    );
                }
                parsed
            }),
            context_menu: read("QUANTICK_CONTEXT_MENU")
                .and_then(|value| ContextMenuPane::from_env_value(&value)),
            context_menu_release: None,
            menu: read("QUANTICK_MENU").and_then(|value| ScriptedMenu::from_token(value.trim())),
            menu_release: None,
            drawings_demo: read("QUANTICK_DRAWINGS_DEMO")
                .filter(|value| matches!(value.as_str(), "1" | "bands"))
                .map(|value| DrawingsDemo {
                    bands: value == "bands",
                    shared: flag("QUANTICK_DRAWINGS_DEMO_SHARED"),
                    select_tool: read("QUANTICK_DRAWINGS_DEMO_SELECT"),
                }),
            drawings_demo_recut: flag("QUANTICK_DRAWINGS_DEMO_RECUT"),
            frvp_demo: read("QUANTICK_FRVP_DEMO")
                .filter(|value| matches!(value.trim(), "1" | "compare" | "stress"))
                .map(|value| FrvpDemo {
                    compare: value.trim() == "compare",
                    stress: value.trim() == "stress",
                    select: read("QUANTICK_FRVP_DEMO_SELECT").is_some_and(|v| v.trim() == "1"),
                }),
            avwap_demo: read("QUANTICK_AVWAP_DEMO").is_some_and(|value| value.trim() == "1"),
            venue_history_demo: read("QUANTICK_VENUE_HISTORY_DEMO")
                .and_then(|value| VenueHistoryDemo::from_token(&value)),
            strategy_demo: read("QUANTICK_STRATEGY_DEMO")
                .and_then(|value| StrategyDemoMode::from_token(&value)),
            // The replay seek, scripted: restart the recording once the
            // session has closed this many round trips. Zero is refused with
            // everything else that does not parse — a restart before any trade
            // closed photographs nothing this hook exists to show.
            replay_restart: read("QUANTICK_REPLAY_RESTART_AFTER")
                .and_then(|value| value.trim().parse::<usize>().ok())
                .filter(|trades| *trades > 0),
            // How many anchors of the armed tool are already down when the run
            // opens — the half-placed state a screenshot cannot otherwise
            // reach, because it lives between two clicks.
            drawing_draft: read("QUANTICK_DRAWING_DRAFT")
                .and_then(|value| value.trim().parse::<usize>().ok())
                .filter(|anchors| *anchors > 0)
                .map(|anchors| DrawingDraft {
                    anchors,
                    constrain: flag("QUANTICK_DRAWING_CONSTRAIN"),
                }),
            // Pages of older trades fetched at launch — the "+ older" button
            // pressed, without a hand. The button's whole point is what it
            // does *after* the click, and the bars it prepends are the
            // surface: a capture that can only photograph the enabled button
            // proves the affordance exists and nothing about whether it works.
            load_older: read("QUANTICK_LOAD_OLDER")
                .and_then(|value| value.trim().parse::<usize>().ok())
                .filter(|pages| *pages > 0)
                .map(|owed| Budgeted {
                    owed,
                    frames: LOAD_OLDER_HOOK_FRAMES,
                }),
            // The same door onto the candle reach. A chart opens on one week
            // (`feed::TIME_HISTORY_SPAN_MS`) and the quarter is asked for a
            // week at a time, so "what does a deep chart look like" is a state
            // no capture could otherwise reach without a hand on the menu.
            load_older_candles: read("QUANTICK_LOAD_OLDER_CANDLES")
                .and_then(|value| value.trim().parse::<usize>().ok())
                .filter(|spans| *spans > 0)
                .map(|owed| Budgeted {
                    owed,
                    frames: LOAD_OLDER_CANDLES_HOOK_FRAMES,
                }),
            history_note: read("QUANTICK_HISTORY_NOTE").and_then(parse_history_note),
            evidence_frames: 0,
        }
    }

    /// Whether the layout popover should open this frame, and never again:
    /// one shot, so a trader's click can close it.
    pub(crate) fn take_layout_picker_autostart(&mut self) -> bool {
        std::mem::take(&mut self.layout_picker_autostart)
    }

    /// The recording default the launch imposed, if it imposed one.
    pub(crate) fn deal_recording_default(&self) -> Option<bool> {
        self.deal_recording.and_then(|hook| hook.default_override())
    }

    /// Whether the launch asked for the REC popover, once.
    pub(crate) fn take_deal_recording_menu(&mut self) -> bool {
        if self.deal_recording == Some(crate::deal_recording::RecordingHook::Menu) {
            self.deal_recording = None;
            true
        } else {
            false
        }
    }

    /// Whether the window should maximise itself, once.
    pub(crate) fn take_maximize(&mut self) -> bool {
        std::mem::take(&mut self.maximize)
    }

    /// Whether every tab — including one opened later by a replay autostart —
    /// should open with the candle footprint on.
    pub(crate) fn footprint(&self) -> bool {
        self.footprint
    }

    /// The scripted zoom and pan, re-applied every frame.
    pub(crate) fn scripted_view(&self) -> (Option<f32>, Option<f32>) {
        (self.candle_width, self.pan_px)
    }

    /// The scripted zoom alone, for the tab-opening path that has no pan to
    /// apply.
    pub(crate) fn candle_width(&self) -> Option<f32> {
        self.candle_width
    }

    /// Whether the boot hook's deferred half is still waiting for a slot that
    /// can show a dialog.
    pub(crate) fn wants_indicator_settings_dialog(&self) -> bool {
        self.indicator_settings_dialog
    }

    /// The dialog opened: disarm, so a dialog the run then closes stays
    /// closed.
    pub(crate) fn indicator_settings_dialog_opened(&mut self) {
        self.indicator_settings_dialog = false;
    }

    /// Which indicator the settings dialog should open on, and on which tab.
    pub(crate) fn settings_autostart(&self) -> Option<(usize, SettingsTab)> {
        self.settings_autostart
    }

    /// That dialog opened: disarm, for the same reason.
    pub(crate) fn settings_autostart_opened(&mut self) {
        self.settings_autostart = None;
    }

    /// Where the pointer is parked, as a fraction of the flow pane's candle
    /// area. Resolving that into window points needs the pane, so the trunk
    /// does it.
    pub(crate) fn pointer(&self) -> Option<egui::Vec2> {
        self.pointer
    }

    /// Which pane still owes a scripted right-click.
    ///
    /// Peeked rather than taken: the divider the click aims at is published by
    /// the draw, so the first frames have no position to click at and the hook
    /// has to stay armed until one appears.
    pub(crate) fn context_menu(&self) -> Option<ContextMenuPane> {
        self.context_menu
    }

    /// The press went out: disarm the pane and hold its release until the
    /// frame that lets the button up.
    pub(crate) fn context_menu_pressed(&mut self, position: egui::Pos2) {
        self.context_menu = None;
        self.context_menu_release = Some(position);
    }

    /// That release, once.
    pub(crate) fn take_context_menu_release(&mut self) -> Option<egui::Pos2> {
        self.context_menu_release.take()
    }

    /// Which menu bar button still owes a scripted press.
    pub(crate) fn menu(&self) -> Option<ScriptedMenu> {
        self.menu
    }

    /// The press went out: disarm the button and hold its release.
    pub(crate) fn menu_pressed(&mut self, position: egui::Pos2) {
        self.menu = None;
        self.menu_release = Some(position);
    }

    /// That release, once.
    pub(crate) fn take_menu_release(&mut self) -> Option<egui::Pos2> {
        self.menu_release.take()
    }

    /// Whether the drawings demo is still owed.
    ///
    /// A bare `bool` rather than the request itself, because this is asked on
    /// **every frame** the applier is waiting for bars, and the request owns a
    /// `String`: handing it out would allocate sixty times a second to answer
    /// "not yet". The applier asks this, checks its bars, and only then takes
    /// the request with [`Self::take_drawings_demo`].
    pub(crate) fn drawings_demo_armed(&self) -> bool {
        self.drawings_demo.is_some()
    }

    /// The demo can place its objects: hand over what was asked for and
    /// consume the hook, so it never re-places ones the trader then deletes.
    pub(crate) fn take_drawings_demo(&mut self) -> Option<DrawingsDemo> {
        self.drawings_demo.take()
    }

    /// Whether the re-cut scene was asked for.
    pub(crate) fn drawings_demo_recut(&self) -> bool {
        self.drawings_demo_recut
    }

    /// What the fixed-range profile demo was asked for, if it was.
    pub(crate) fn frvp_demo(&self) -> Option<FrvpDemo> {
        self.frvp_demo
    }

    /// The demo placed its profile: consumed.
    pub(crate) fn frvp_demo_placed(&mut self) {
        self.frvp_demo = None;
    }

    /// Put the profile demo back: the scene it asked for needs more bars than
    /// this frame has, and it tries again on the next one.
    pub(crate) fn rearm_frvp_demo(&mut self, demo: FrvpDemo) {
        self.frvp_demo = Some(demo);
    }

    /// Whether the anchored-VWAP demo was asked for.
    pub(crate) fn avwap_demo(&self) -> bool {
        self.avwap_demo
    }

    /// It placed its anchor: consumed.
    pub(crate) fn avwap_demo_placed(&mut self) {
        self.avwap_demo = false;
    }

    /// Which venue-history frame to stage, if one was asked for.
    pub(crate) fn venue_history_demo(&self) -> Option<VenueHistoryDemo> {
        self.venue_history_demo
    }

    /// The prefix was delivered: consumed.
    pub(crate) fn venue_history_demo_staged(&mut self) {
        self.venue_history_demo = None;
    }

    /// Which strategy scene to stage, if one was asked for.
    pub(crate) fn strategy_demo(&self) -> Option<StrategyDemoMode> {
        self.strategy_demo
    }

    /// The rectangle exists: consumed.
    pub(crate) fn strategy_demo_staged(&mut self) {
        self.strategy_demo = None;
    }

    /// How many closed round trips the replay restart waits for.
    pub(crate) fn replay_restart_after(&self) -> Option<usize> {
        self.replay_restart
    }

    /// The transport took the Restart: consumed. Spent only once it did — a
    /// hook that cleared itself on a dropped command would leave the capture
    /// photographing an un-seeked timeline while the harness believed
    /// otherwise.
    pub(crate) fn replay_restart_taken(&mut self) {
        self.replay_restart = None;
    }

    /// The half-placed drawing the run opens on, if one was asked for.
    pub(crate) fn drawing_draft(&self) -> Option<DrawingDraft> {
        self.drawing_draft
    }

    /// The draft was staged: consumed.
    pub(crate) fn drawing_draft_staged(&mut self) {
        self.drawing_draft = None;
    }

    /// Pages of older trades still owed.
    pub(crate) fn load_older_pages(&self) -> Option<usize> {
        self.load_older.map(|hook| hook.owed)
    }

    /// Wait one frame for a chart worth paging from.
    pub(crate) fn spend_load_older_frame(&mut self) -> HookFrame {
        spend(&mut self.load_older)
    }

    /// A page actually went out: one fewer owed, and the budget is untouched —
    /// a page delivered is the feature working, not time spent waiting.
    pub(crate) fn load_older_page_sent(&mut self) {
        if let Some(hook) = self.load_older
            && hook.owed > 1
        {
            self.load_older = Some(Budgeted {
                owed: hook.owed - 1,
                ..hook
            });
        } else {
            self.load_older = None;
        }
    }

    /// Spans of older candles still owed.
    pub(crate) fn load_older_candle_spans(&self) -> Option<usize> {
        self.load_older_candles.map(|hook| hook.owed)
    }

    /// Wait one frame — for a first reply to reach back from, for a span still
    /// arriving, or for a command channel with room in it.
    pub(crate) fn spend_load_older_candles_frame(&mut self) -> HookFrame {
        spend(&mut self.load_older_candles)
    }

    /// A span's request actually went out: one fewer owed.
    pub(crate) fn load_older_candles_span_sent(&mut self) {
        if let Some(hook) = self.load_older_candles
            && hook.owed > 1
        {
            self.load_older_candles = Some(Budgeted {
                owed: hook.owed - 1,
                ..hook
            });
        } else {
            self.load_older_candles = None;
        }
    }

    /// The ending whose sentence is being held up, if one is.
    pub(crate) fn history_note_ending(&self) -> Option<CampaignEnd> {
        self.history_note.map(|hook| hook.owed)
    }

    /// Hold the sentence for one more frame. When the budget runs out the note
    /// keeps its ordinary linger from the last raise and leaves on its own, so
    /// even a hooked run photographs a note that expires.
    pub(crate) fn spend_history_note_frame(&mut self) -> HookFrame {
        spend(&mut self.history_note)
    }

    /// Wait one frame for the window to hand over a rasterised frame; `false`
    /// once it has waited [`CONTROL_EVIDENCE_HOOK_FRAMES`] of them.
    pub(crate) fn evidence_frame_waited(&mut self) -> bool {
        self.evidence_frames = self.evidence_frames.saturating_add(1);
        self.evidence_frames <= CONTROL_EVIDENCE_HOOK_FRAMES
    }
}

/// Arming, for tests: the hooks a test drives directly rather than through the
/// environment, because `set_var` is process-wide and the suite is threaded.
#[cfg(test)]
impl Harness {
    pub(crate) fn arm_pointer(&mut self, fraction: egui::Vec2) {
        self.pointer = Some(fraction);
    }

    pub(crate) fn arm_pan_px(&mut self, px: f32) {
        self.pan_px = Some(px);
    }

    pub(crate) fn arm_replay_restart(&mut self, after: usize) {
        self.replay_restart = Some(after);
    }

    pub(crate) fn arm_drawings_demo(&mut self, demo: DrawingsDemo) {
        self.drawings_demo = Some(demo);
    }

    pub(crate) fn arm_load_older(&mut self, pages: usize, frames: u32) {
        self.load_older = Some(Budgeted {
            owed: pages,
            frames,
        });
    }

    /// Pages still owed and frames still left, so a test can assert on the
    /// budget the way it used to assert on the tuple.
    pub(crate) fn load_older_remaining(&self) -> Option<(usize, u32)> {
        self.load_older.map(|hook| (hook.owed, hook.frames))
    }

    pub(crate) fn arm_settings_autostart(&mut self, index: usize, tab: SettingsTab) {
        self.settings_autostart = Some((index, tab));
    }
}

/// Spend one frame of a budgeted hook, disarming it when the budget runs out.
fn spend<T: Copy>(hook: &mut Option<Budgeted<T>>) -> HookFrame {
    let Some(armed) = *hook else {
        return HookFrame::default();
    };
    *hook = armed.spend_frame();
    HookFrame {
        gave_up: hook.is_none(),
    }
}

/// One environment variable, or `None` when it is unset or not valid Unicode.
fn read(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// The `=1` shape most switch hooks share: the value exactly, untrimmed.
///
/// Exactly, because that is what each of these hooks did before they were
/// collected here and this move changed no behaviour. Two of their
/// neighbours — `QUANTICK_AVWAP_DEMO` and `QUANTICK_FRVP_DEMO_SELECT` — trim
/// first, which is an inconsistency this module inherited rather than one it
/// introduced. It is visible here for the first time, in one place, which is
/// the point; making the eight agree is a change to what the hooks accept and
/// belongs to a mission that says so.
fn flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| value == "1")
}

/// Read a scripted pointer position off `QUANTICK_POINTER`.
///
/// `<fx>,<fy>`, both fractions of the flow pane's *candle* area — `0,0` its
/// top-left corner, `1,1` its bottom-right, `0.99,0.5` out in the projection
/// margin past the newest bar. The flow pane and not the focused one, so this
/// hook and `QUANTICK_CONTEXT_MENU` aim at the same canvas: a capture that
/// opened an axis menu on one pane and parked the mouse on another would be
/// photographing two different charts at once. Fractions rather than pixels
/// because the thing a capture wants to point at is a candle, and the candles'
/// pane moves with the window size, the lane divider and the indicator band: an
/// absolute pair that framed the right bar at one window size frames a
/// different one at the next, and a capture that photographs the wrong bar and
/// calls it a pass is worse than one that photographs nothing.
///
/// `None` for anything else — a typo must not silently place the pointer
/// somewhere the author did not ask for.
fn parse_pointer_fraction(value: &str) -> Option<egui::Vec2> {
    let (x, y) = value.trim().split_once(',')?;
    let x: f32 = x.trim().parse().ok()?;
    let y: f32 = y.trim().parse().ok()?;
    ((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y)).then(|| egui::vec2(x, y))
}

/// Parse `QUANTICK_INDICATOR_SETTINGS`: `<index>` or `<index>:<tab>`, where
/// the tab is `inputs`/`0` or `style`/`1`.
///
/// Nonsense is no hook at all rather than a guessed one: a validation run that
/// captured the wrong tab because a typo silently defaulted would be a
/// screenshot claiming to show something it does not.
fn parse_settings_hook(value: &str) -> Option<(usize, SettingsTab)> {
    let (index, tab) = value.split_once(':').unwrap_or((value, "inputs"));
    let index = index.trim().parse::<usize>().ok()?;
    let tab = match tab.trim().to_ascii_lowercase().as_str() {
        "inputs" | "0" => SettingsTab::Inputs,
        "style" | "1" => SettingsTab::Style,
        _ => return None,
    };
    Some((index, tab))
}

/// Read the ending `QUANTICK_HISTORY_NOTE` names, and arm the hold.
///
/// Named by the ending's own log token (`nothing_coming_back`,
/// `venue_exhausted`, `page_budget_spent`, …) and resolved through
/// `CampaignEnd::from_action`, so an ending that exists is photographable by
/// name and one that does not yields no note rather than the wrong one.
///
/// Refused out loud, exactly as `QUANTICK_HISTORY_REACH` refuses an unknown
/// reach: a capture run that silently got no note reads the surface as broken,
/// which is the conclusion this branch exists to make impossible.
fn parse_history_note(token: String) -> Option<Budgeted<CampaignEnd>> {
    match CampaignEnd::from_action(&token) {
        Some(end) if end.notice().is_some() => Some(Budgeted {
            owed: end,
            frames: HISTORY_NOTE_HOOK_FRAMES,
        }),
        Some(end) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HISTORY_NOTE_HOOK_SILENT_ENDING",
                ending = end.action(),
                action = "no_note_raised",
                "QUANTICK_HISTORY_NOTE named the one ending that says nothing"
            );
            None
        }
        None => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HISTORY_NOTE_HOOK_UNKNOWN",
                token = %token,
                accepted = %CampaignEnd::ALL.map(CampaignEnd::action).join(", "),
                action = "no_note_raised",
                "QUANTICK_HISTORY_NOTE names no ending this build has"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_budgeted_hook_gives_up_on_the_frame_that_spends_its_last() {
        let mut harness = Harness::default();
        harness.arm_load_older(2, 2);
        assert!(!harness.spend_load_older_frame().gave_up, "one frame left");
        assert_eq!(harness.load_older_remaining(), Some((2, 1)));
        assert!(!harness.spend_load_older_frame().gave_up, "the last frame");
        assert_eq!(harness.load_older_remaining(), Some((2, 0)));
        assert!(
            harness.spend_load_older_frame().gave_up,
            "the budget is finite"
        );
        assert_eq!(harness.load_older_remaining(), None, "and it disarmed");
    }

    #[test]
    fn a_page_that_went_out_costs_a_page_and_no_budget() {
        let mut harness = Harness::default();
        harness.arm_load_older(2, 10);
        harness.load_older_page_sent();
        assert_eq!(
            harness.load_older_remaining(),
            Some((1, 10)),
            "one still owed, on the same budget"
        );
        harness.load_older_page_sent();
        assert_eq!(harness.load_older_remaining(), None, "both pages asked for");
    }

    #[test]
    fn spending_a_hook_that_was_never_armed_reports_nothing() {
        let mut harness = Harness::default();
        assert_eq!(
            harness.spend_load_older_frame(),
            HookFrame::default(),
            "an unset hook never gave up, because it never waited"
        );
    }

    #[test]
    fn the_evidence_hook_waits_its_budget_and_then_stops() {
        let mut harness = Harness::default();
        for frame in 1..=CONTROL_EVIDENCE_HOOK_FRAMES {
            assert!(
                harness.evidence_frame_waited(),
                "frame {frame} is within the budget"
            );
        }
        assert!(
            !harness.evidence_frame_waited(),
            "the window never delivered a frame to rasterise"
        );
    }

    /// `QUANTICK_POINTER` parks the mouse over the candles, which is the only
    /// way a scripted run photographs anything that exists while a pointer is
    /// over the chart — the compass, the crosshair, every hover readout.
    ///
    /// Fractions of the *candles'* pane, so `0.5,0.5` frames the same place
    /// whatever the window size and whatever share the live lane has taken.
    /// Where that lands in window points is the trunk's half of the hook, and
    /// `the_pointer_hook_parks_the_mouse_among_the_candles` asserts it.
    #[test]
    fn a_pointer_fraction_outside_the_canvas_is_refused() {
        assert_eq!(
            parse_pointer_fraction("0.25, 0.75"),
            Some(egui::vec2(0.25, 0.75))
        );
        for refused in ["0.5", "2,0.5", "-0.1,0.5", "half,half", ""] {
            assert_eq!(
                parse_pointer_fraction(refused),
                None,
                "{refused:?} is not a position, and a typo must photograph nothing rather than the wrong place"
            );
        }
    }

    /// The harness hook is the only way a scripted capture can see the
    /// settings dialog, so it has to name both halves — and refuse nonsense
    /// rather than guess, which would produce a screenshot of the wrong tab.
    #[test]
    fn the_settings_hook_names_an_indicator_and_a_tab() {
        assert_eq!(
            parse_settings_hook("0"),
            Some((0, SettingsTab::Inputs)),
            "a bare index opens on Inputs"
        );
        assert_eq!(
            parse_settings_hook("2:style"),
            Some((2, SettingsTab::Style))
        );
        assert_eq!(
            parse_settings_hook("1:1"),
            Some((1, SettingsTab::Style)),
            "the numeric spelling works too"
        );
        assert_eq!(parse_settings_hook("0:colours"), None);
        assert_eq!(parse_settings_hook("first"), None);
        assert_eq!(parse_settings_hook(""), None);
    }

    #[test]
    fn every_scripted_menu_is_reachable_by_its_token() {
        assert_eq!(
            ScriptedMenu::from_token("workspace"),
            Some(ScriptedMenu::Workspace)
        );
        assert_eq!(
            ScriptedMenu::from_token("HISTORY"),
            Some(ScriptedMenu::History)
        );
        assert_eq!(ScriptedMenu::from_token("file"), None, "no such menu");
    }

    #[test]
    fn a_context_menu_pane_is_named_by_either_of_its_two_words() {
        assert_eq!(
            ContextMenuPane::from_env_value("candles"),
            Some(ContextMenuPane::Chart)
        );
        assert_eq!(
            ContextMenuPane::from_env_value("Lane"),
            Some(ContextMenuPane::Tape)
        );
        assert_eq!(
            ContextMenuPane::from_env_value("legend"),
            None,
            "no such pane"
        );
    }
}
