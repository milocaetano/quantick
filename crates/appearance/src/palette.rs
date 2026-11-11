//! Stable semantic names and the historical Dark bytes.
use crate::PremultipliedRgba;

pub const CANVAS: PremultipliedRgba = PremultipliedRgba::from_premultiplied([19, 23, 34, 255]);
pub const CHROME: PremultipliedRgba = PremultipliedRgba::from_premultiplied([23, 27, 38, 255]);
pub const INSET: PremultipliedRgba = PremultipliedRgba::from_premultiplied([16, 20, 29, 255]);
pub const CONTROL: PremultipliedRgba = PremultipliedRgba::from_premultiplied([35, 41, 54, 255]);
pub const BORDER: PremultipliedRgba = PremultipliedRgba::from_premultiplied([46, 54, 72, 255]);
pub const TEXT_PRIMARY: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([210, 218, 226, 255]);
pub const TEXT_MUTED: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([150, 160, 175, 255]);
pub const TEXT_FAINT: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([110, 120, 135, 255]);
pub const BUY: PremultipliedRgba = PremultipliedRgba::from_premultiplied([38, 166, 154, 255]);
pub const SELL: PremultipliedRgba = PremultipliedRgba::from_premultiplied([239, 83, 80, 255]);
pub const POC: PremultipliedRgba = PremultipliedRgba::from_premultiplied([255, 213, 79, 255]);
pub const ACCENT: PremultipliedRgba = PremultipliedRgba::from_premultiplied([138, 180, 248, 255]);
pub const AMBER: PremultipliedRgba = PremultipliedRgba::from_premultiplied([240, 185, 11, 255]);
pub const PANE_MARK: PremultipliedRgba = PremultipliedRgba::from_premultiplied([190, 156, 63, 217]);
pub const PANE_ACTION: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([170, 176, 191, 255]);
pub const OFF: PremultipliedRgba = PremultipliedRgba::from_premultiplied([242, 154, 75, 255]);
pub const CAPSULE_FILL: PremultipliedRgba = PremultipliedRgba::from_premultiplied([8, 10, 15, 189]);
pub const CAPSULE_BORDER: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([23, 23, 23, 23]);
pub const CAPSULE_SHADOW: PremultipliedRgba = PremultipliedRgba::from_premultiplied([0, 0, 0, 115]);
pub const BARE_HOVER: PremultipliedRgba = PremultipliedRgba::from_premultiplied([15, 15, 15, 15]);
pub const SEAM_LINE: PremultipliedRgba = PremultipliedRgba::from_premultiplied([60, 60, 60, 60]);
pub const DAY_RULE: PremultipliedRgba = PremultipliedRgba::from_premultiplied([30, 30, 30, 30]);
pub const SEAM_LABEL: PremultipliedRgba = PremultipliedRgba::from_premultiplied([80, 80, 80, 80]);
pub const WARN: PremultipliedRgba = PremultipliedRgba::from_premultiplied([255, 99, 71, 255]);
pub const REC: PremultipliedRgba = PremultipliedRgba::from_premultiplied([255, 77, 77, 255]);
pub const GAP_LINE: PremultipliedRgba = PremultipliedRgba::from_premultiplied([120, 92, 10, 150]);
pub const GAP_LABEL: PremultipliedRgba = PremultipliedRgba::from_premultiplied([154, 130, 58, 255]);
pub const TAG_BG: PremultipliedRgba = PremultipliedRgba::from_premultiplied([55, 63, 80, 255]);
pub const TEXT_SUPPORT: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([134, 146, 164, 255]);
pub const CHIP_INK: PremultipliedRgba = PremultipliedRgba::from_premultiplied([14, 18, 26, 255]);
pub const CASING: PremultipliedRgba = PremultipliedRgba::from_premultiplied([5, 7, 12, 235]);
pub const DRAW_VIOLET: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([197, 138, 249, 255]);
pub const DRAW_CYAN: PremultipliedRgba = PremultipliedRgba::from_premultiplied([77, 208, 225, 255]);
pub const FLOAT_SHADOW: PremultipliedRgba = PremultipliedRgba::from_premultiplied([0, 0, 0, 96]);
pub const CANDLE_BULL_OUTLINE: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([104, 224, 201, 255]);
pub const CANDLE_BEAR_OUTLINE: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([255, 137, 139, 255]);
pub const CANDLE_WICK: PremultipliedRgba =
    PremultipliedRgba::from_premultiplied([160, 169, 184, 255]);
pub const BUY_INK: PremultipliedRgba = PremultipliedRgba::from_rgb(136, 206, 199);
pub const SELL_INK: PremultipliedRgba = PremultipliedRgba::from_rgb(246, 160, 159);
pub const CASING_EXTRA_PX: f32 = 2.0;
pub const DRAWING_SWATCHES: [PremultipliedRgba; 8] = [
    ACCENT,
    TEXT_PRIMARY,
    TEXT_MUTED,
    BUY,
    SELL,
    POC,
    DRAW_VIOLET,
    DRAW_CYAN,
];
/// Pure red, green and blue, their pairwise mixes and white: the additive
/// wheel, offered beside the theme swatches. Theme-independent on purpose —
/// a trader who picks pure red means pure red on either canvas.
pub const DRAWING_PRIMARIES: [PremultipliedRgba; 7] = [
    PremultipliedRgba::from_rgb(255, 0, 0),
    PremultipliedRgba::from_rgb(0, 255, 0),
    PremultipliedRgba::from_rgb(0, 0, 255),
    PremultipliedRgba::from_rgb(255, 255, 0),
    PremultipliedRgba::from_rgb(0, 255, 255),
    PremultipliedRgba::from_rgb(255, 0, 255),
    PremultipliedRgba::from_rgb(255, 255, 255),
];

/// Stable configuration keys. Order is stable and forms the indexed render palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(usize)]
pub enum Token {
    FlowSurface,
    FlowDivider,
    FlowInk,
    FlowKeyDown,
    FlowKeyDownShade,
    FlowKeyDownRim,
    FlowKeyDownInk,
    FlowKeyUpTop,
    FlowKeyUpBottom,
    FlowKeyUpLip,
    FlowKeyUpEdge,
    FlowKeyUpShadow,
    FlowKeyUpInk,
    FlowBandRule,

    TradeSelectionHalo,
    CandlePreviewBackground,
    CandlePreviewBid,
    CandlePreviewAsk,
    CandlePreviewBuy,
    CandlePreviewSell,
    RetainedTapeShade,
    TextShadow,
    ReplayError,
    ReplayInset,
    ReplayWell,
    ErrorText,
    ActionInk,
    ModalScrim,
    WidgetOpenBorder,
    WidgetActiveBorder,
    WidgetNoninteractive,
    WidgetShadow,
    WidgetCursor,
    WidgetError,
    WidgetWarn,
    WidgetCode,
    DrawingReadoutPlate,
    DrawingSelectionHalo,
    Canvas,
    Chrome,
    Inset,
    Control,
    Border,
    TextPrimary,
    TextMuted,
    TextFaint,
    Buy,
    Sell,
    Poc,
    Accent,
    Amber,
    PaneMark,
    PaneAction,
    Off,
    CapsuleFill,
    CapsuleBorder,
    CapsuleShadow,
    BareHover,
    SeamLine,
    DayRule,
    SeamLabel,
    Warn,
    Rec,
    GapLine,
    GapLabel,
    TagBg,
    TextSupport,
    ChipInk,
    Casing,
    DrawViolet,
    DrawCyan,
    FloatShadow,
    CandleBullOutline,
    CandleBearOutline,
    CandleWick,
    BuyInk,
    SellInk,
    LoadingTitle,
    LoadingMeta,
    LoadingFaint,
    LoadingBorder,
    FootprintProfile,
    FootprintBevelHighlight,
    FootprintBevelShadow,
    CrosshairInk,
    FootprintTotalInk,
    LoadingLogoLow,
    LoadingLogoHigh,
    FlowBuy,
    FlowSell,
    TapePath,
    TapeLabel,
    QuantityShadow,
    DrawingDefault,
    DrawingBuy,
    DrawingSell,
    DrawingVwap,
    DrawingProfile,
    ChartGrid,
    TabStrip,
    Brand,
    ReplayInk,
    ToolbarSheet,
    TabIdleFill,
    TabIdleOutline,
    TabBand,
    HomeGlow,
    /// Ink of a light theme's axis labels. Dark writes its axes in `text_muted`.
    AxisInk,
    /// A light theme's rule between the plot and the price axis; transparent
    /// draws none. Dark rules its axes in the chart grid's own colour.
    PriceAxisRule,
    /// A light theme's rule along the top of the time axis.
    TimeAxisRule,
    /// The chart's plot and axes. A document that leaves it out uses its own
    /// `canvas`, which every other surface keeps reading.
    ChartCanvas,
    /// A light theme's last-price chip over a rising bar. Dark's chip wears
    /// the candle's own outline colour.
    LastPriceUp,
    /// A light theme's last-price chip over a falling bar.
    LastPriceDown,
    /// The filled replay key: the REPLAY badge, the chosen speed and a replay
    /// dialog's primary action. Dark's is its amber.
    ReplayPlate,
    /// Words on [`Token::ReplayPlate`].
    ReplayPlateInk,
    /// The plate a light theme lays under the chart's order and position
    /// labels, and under the report's drawdown chip.
    OrderPlate,
    /// Words on [`Token::OrderPlate`].
    OrderPlateInk,
    /// A gain written on a dark plate in a light theme.
    PlateGain,
    /// A loss written on a dark plate in a light theme.
    PlateLoss,
    /// The fill under a hovered control or row. Dark's is its border.
    HoverFill,
    /// The edge of a framed control at rest; Dark's is transparent.
    ControlEdge,
    /// The connection screen's action (`Reconnect`): Dark keeps it in the
    /// loading meta grey, a light theme writes it in its primary ink.
    ConnectionAction,
    /// A light theme's loss on its pale panels. Its `sell_ink` is the sell
    /// side's ink, near-black like its sell candles, so a loss reads this
    /// separate red. A dark theme writes losses in `sell`; Dark's value
    /// mirrors it.
    LossInk,
    /// A light theme's gain on a dark surface such as its slate flow pane.
    /// A dark theme writes gains in `buy`; Dark's value mirrors it.
    BrightGainInk,
    /// A light theme's loss on a dark surface. Dark's value mirrors `sell`.
    BrightLossInk,
    /// The trade ledger's day card, one step off the panel; its trade
    /// rows wear it too.
    LedgerCard,
    /// The day card's one-pixel border.
    LedgerCardEdge,
    /// A session's box inside its day card.
    LedgerSession,
    /// The session box's one-pixel border.
    LedgerSessionEdge,
    /// The selected trade row.
    LedgerSelection,
    /// The order-flow pane's band, under its mark and keys: its own fill, apart
    /// from `flow_surface`, which also paints the side headers and the axis strip.
    FlowBand,
    /// A band key's dot while its side is shown.
    FlowKeyLitDot,
    /// A band key's dot while its side is not shown.
    FlowKeyUnlitDot,
    /// A band key's word while its side is not shown.
    FlowKeyUnlitInk,
    /// The band's settings gear at rest.
    FlowGearInk,
    /// The band's light bulb while the flow pane's lights are on; at rest it
    /// wears the gear's ink.
    FlowBulbLit,
    /// A settings panel's or dock's ground, under its blocks.
    PanelGround,
    /// The full-bleed title plate across a panel's top.
    PanelPlate,
    /// Words on the title plate and on a section's band.
    PanelPlateInk,
    /// A section's block, and a trade's card, on the panel ground.
    PanelBlock,
    /// A block's one-pixel border, and the rule under the filter bar.
    PanelBlockEdge,
    /// A section header's band, and the ledger's date row.
    PanelBand,
    /// A section header's state words on its band.
    PanelBandState,
    /// A subgroup's uppercase caption inside a block.
    PanelCaption,
    /// The one-pixel rule under a subgroup caption.
    PanelCaptionRule,
    /// The full-bleed footer plate: health, reset and totals.
    PanelFooter,
    /// Words on the footer plate, and the ledger's bold session captions.
    PanelStrongInk,
    /// The ledger's full-bleed filter bar.
    PanelFilterBar,
    /// The ledger's Live session row.
    PanelLiveRow,
    /// A losing trade card's left stripe.
    PanelStripeLoss,
}
impl Token {
    pub const ALL: [Self; 150] = [
        Self::FlowSurface,
        Self::FlowDivider,
        Self::FlowInk,
        Self::FlowKeyDown,
        Self::FlowKeyDownShade,
        Self::FlowKeyDownRim,
        Self::FlowKeyDownInk,
        Self::FlowKeyUpTop,
        Self::FlowKeyUpBottom,
        Self::FlowKeyUpLip,
        Self::FlowKeyUpEdge,
        Self::FlowKeyUpShadow,
        Self::FlowKeyUpInk,
        Self::FlowBandRule,
        Self::TradeSelectionHalo,
        Self::CandlePreviewBackground,
        Self::CandlePreviewBid,
        Self::CandlePreviewAsk,
        Self::CandlePreviewBuy,
        Self::CandlePreviewSell,
        Self::RetainedTapeShade,
        Self::TextShadow,
        Self::ReplayError,
        Self::ReplayInset,
        Self::ReplayWell,
        Self::ErrorText,
        Self::ActionInk,
        Self::ModalScrim,
        Self::WidgetOpenBorder,
        Self::WidgetActiveBorder,
        Self::WidgetNoninteractive,
        Self::WidgetShadow,
        Self::WidgetCursor,
        Self::WidgetError,
        Self::WidgetWarn,
        Self::WidgetCode,
        Self::DrawingReadoutPlate,
        Self::DrawingSelectionHalo,
        Self::Canvas,
        Self::Chrome,
        Self::Inset,
        Self::Control,
        Self::Border,
        Self::TextPrimary,
        Self::TextMuted,
        Self::TextFaint,
        Self::Buy,
        Self::Sell,
        Self::Poc,
        Self::Accent,
        Self::Amber,
        Self::PaneMark,
        Self::PaneAction,
        Self::Off,
        Self::CapsuleFill,
        Self::CapsuleBorder,
        Self::CapsuleShadow,
        Self::BareHover,
        Self::SeamLine,
        Self::DayRule,
        Self::SeamLabel,
        Self::Warn,
        Self::Rec,
        Self::GapLine,
        Self::GapLabel,
        Self::TagBg,
        Self::TextSupport,
        Self::ChipInk,
        Self::Casing,
        Self::DrawViolet,
        Self::DrawCyan,
        Self::FloatShadow,
        Self::CandleBullOutline,
        Self::CandleBearOutline,
        Self::CandleWick,
        Self::BuyInk,
        Self::SellInk,
        Self::LoadingTitle,
        Self::LoadingMeta,
        Self::LoadingFaint,
        Self::LoadingBorder,
        Self::FootprintProfile,
        Self::FootprintBevelHighlight,
        Self::FootprintBevelShadow,
        Self::CrosshairInk,
        Self::FootprintTotalInk,
        Self::LoadingLogoLow,
        Self::LoadingLogoHigh,
        Self::FlowBuy,
        Self::FlowSell,
        Self::TapePath,
        Self::TapeLabel,
        Self::QuantityShadow,
        Self::DrawingDefault,
        Self::DrawingBuy,
        Self::DrawingSell,
        Self::DrawingVwap,
        Self::DrawingProfile,
        Self::ChartGrid,
        Self::TabStrip,
        Self::Brand,
        Self::ReplayInk,
        Self::ToolbarSheet,
        Self::TabIdleFill,
        Self::TabIdleOutline,
        Self::TabBand,
        Self::HomeGlow,
        Self::AxisInk,
        Self::PriceAxisRule,
        Self::TimeAxisRule,
        Self::ChartCanvas,
        Self::LastPriceUp,
        Self::LastPriceDown,
        Self::ReplayPlate,
        Self::ReplayPlateInk,
        Self::OrderPlate,
        Self::OrderPlateInk,
        Self::PlateGain,
        Self::PlateLoss,
        Self::HoverFill,
        Self::ControlEdge,
        Self::ConnectionAction,
        Self::LossInk,
        Self::BrightGainInk,
        Self::BrightLossInk,
        Self::LedgerCard,
        Self::LedgerCardEdge,
        Self::LedgerSession,
        Self::LedgerSessionEdge,
        Self::LedgerSelection,
        Self::FlowBand,
        Self::FlowKeyLitDot,
        Self::FlowKeyUnlitDot,
        Self::FlowKeyUnlitInk,
        Self::FlowGearInk,
        Self::FlowBulbLit,
        Self::PanelGround,
        Self::PanelPlate,
        Self::PanelPlateInk,
        Self::PanelBlock,
        Self::PanelBlockEdge,
        Self::PanelBand,
        Self::PanelBandState,
        Self::PanelCaption,
        Self::PanelCaptionRule,
        Self::PanelFooter,
        Self::PanelStrongInk,
        Self::PanelFilterBar,
        Self::PanelLiveRow,
        Self::PanelStripeLoss,
    ];
    pub const fn key(self) -> &'static str {
        match self {
            Self::DrawingDefault => "drawing_default",
            Self::DrawingBuy => "drawing_buy",
            Self::DrawingSell => "drawing_sell",
            Self::DrawingVwap => "drawing_vwap",
            Self::DrawingProfile => "drawing_profile",
            Self::ChartGrid => "chart_grid",
            Self::TabStrip => "tab_strip",
            Self::Brand => "brand",
            Self::ReplayInk => "replay_ink",
            Self::ToolbarSheet => "toolbar_sheet",
            Self::TabIdleFill => "tab_idle_fill",
            Self::TabIdleOutline => "tab_idle_outline",
            Self::TabBand => "tab_band",
            Self::HomeGlow => "home_glow",
            Self::AxisInk => "axis_ink",
            Self::PriceAxisRule => "price_axis_rule",
            Self::TimeAxisRule => "time_axis_rule",
            Self::ChartCanvas => "chart_canvas",
            Self::LastPriceUp => "last_price_up",
            Self::LastPriceDown => "last_price_down",
            Self::ReplayPlate => "replay_plate",
            Self::ReplayPlateInk => "replay_plate_ink",
            Self::OrderPlate => "order_plate",
            Self::OrderPlateInk => "order_plate_ink",
            Self::PlateGain => "plate_gain",
            Self::PlateLoss => "plate_loss",
            Self::HoverFill => "hover_fill",
            Self::ControlEdge => "control_edge",
            Self::ConnectionAction => "connection_action",
            Self::LossInk => "loss_ink",
            Self::BrightGainInk => "bright_gain_ink",
            Self::BrightLossInk => "bright_loss_ink",
            Self::LedgerCard => "ledger_card",
            Self::LedgerCardEdge => "ledger_card_edge",
            Self::LedgerSession => "ledger_session",
            Self::LedgerSessionEdge => "ledger_session_edge",
            Self::LedgerSelection => "ledger_selection",
            Self::FlowBand => "flow_band",
            Self::FlowKeyLitDot => "flow_key_lit_dot",
            Self::FlowKeyUnlitDot => "flow_key_unlit_dot",
            Self::FlowKeyUnlitInk => "flow_key_unlit_ink",
            Self::FlowGearInk => "flow_gear_ink",
            Self::FlowBulbLit => "flow_bulb_lit",
            Self::PanelGround => "panel_ground",
            Self::PanelPlate => "panel_plate",
            Self::PanelPlateInk => "panel_plate_ink",
            Self::PanelBlock => "panel_block",
            Self::PanelBlockEdge => "panel_block_edge",
            Self::PanelBand => "panel_band",
            Self::PanelBandState => "panel_band_state",
            Self::PanelCaption => "panel_caption",
            Self::PanelCaptionRule => "panel_caption_rule",
            Self::PanelFooter => "panel_footer",
            Self::PanelStrongInk => "panel_strong_ink",
            Self::PanelFilterBar => "panel_filter_bar",
            Self::PanelLiveRow => "panel_live_row",
            Self::PanelStripeLoss => "panel_stripe_loss",
            Self::FlowSurface => "flow_surface",
            Self::FlowDivider => "flow_divider",
            Self::FlowInk => "flow_ink",
            Self::FlowKeyDown => "flow_key_down",
            Self::FlowKeyDownShade => "flow_key_down_shade",
            Self::FlowKeyDownRim => "flow_key_down_rim",
            Self::FlowKeyDownInk => "flow_key_down_ink",
            Self::FlowKeyUpTop => "flow_key_up_top",
            Self::FlowKeyUpBottom => "flow_key_up_bottom",
            Self::FlowKeyUpLip => "flow_key_up_lip",
            Self::FlowKeyUpEdge => "flow_key_up_edge",
            Self::FlowKeyUpShadow => "flow_key_up_shadow",
            Self::FlowKeyUpInk => "flow_key_up_ink",
            Self::FlowBandRule => "flow_band_rule",
            Self::DrawingSelectionHalo => "drawing_selection_halo",
            Self::DrawingReadoutPlate => "drawing_readout_plate",
            Self::WidgetCode => "widget_code",
            Self::WidgetWarn => "widget_warn",
            Self::WidgetError => "widget_error",
            Self::WidgetCursor => "widget_cursor",
            Self::WidgetShadow => "widget_shadow",
            Self::WidgetNoninteractive => "widget_noninteractive",
            Self::WidgetActiveBorder => "widget_active_border",
            Self::WidgetOpenBorder => "widget_open_border",
            Self::ModalScrim => "modal_scrim",
            Self::ActionInk => "action_ink",
            Self::ErrorText => "error_text",
            Self::ReplayWell => "replay_well",
            Self::ReplayInset => "replay_inset",
            Self::ReplayError => "replay_error",
            Self::TradeSelectionHalo => "trade_selection_halo",
            Self::CandlePreviewBackground => "candle_preview_background",
            Self::CandlePreviewBid => "candle_preview_bid",
            Self::CandlePreviewAsk => "candle_preview_ask",
            Self::CandlePreviewBuy => "candle_preview_buy",
            Self::CandlePreviewSell => "candle_preview_sell",
            Self::RetainedTapeShade => "retained_tape_shade",
            Self::TextShadow => "text_shadow",
            Self::Canvas => "canvas",
            Self::Chrome => "chrome",
            Self::Inset => "inset",
            Self::Control => "control",
            Self::Border => "border",
            Self::TextPrimary => "text_primary",
            Self::TextMuted => "text_muted",
            Self::TextFaint => "text_faint",
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::Poc => "poc",
            Self::Accent => "accent",
            Self::Amber => "amber",
            Self::PaneMark => "pane_mark",
            Self::PaneAction => "pane_action",
            Self::Off => "off",
            Self::CapsuleFill => "capsule_fill",
            Self::CapsuleBorder => "capsule_border",
            Self::CapsuleShadow => "capsule_shadow",
            Self::BareHover => "bare_hover",
            Self::SeamLine => "seam_line",
            Self::DayRule => "day_rule",
            Self::SeamLabel => "seam_label",
            Self::Warn => "warn",
            Self::Rec => "rec",
            Self::GapLine => "gap_line",
            Self::GapLabel => "gap_label",
            Self::TagBg => "tag_bg",
            Self::TextSupport => "text_support",
            Self::ChipInk => "chip_ink",
            Self::Casing => "casing",
            Self::DrawViolet => "draw_violet",
            Self::DrawCyan => "draw_cyan",
            Self::FloatShadow => "float_shadow",
            Self::CandleBullOutline => "candle_bull_outline",
            Self::CandleBearOutline => "candle_bear_outline",
            Self::CandleWick => "candle_wick",
            Self::BuyInk => "buy_ink",
            Self::SellInk => "sell_ink",
            Self::LoadingTitle => "loading_title",
            Self::LoadingMeta => "loading_meta",
            Self::LoadingFaint => "loading_faint",
            Self::LoadingBorder => "loading_border",
            Self::FootprintProfile => "footprint_profile",
            Self::FootprintBevelHighlight => "footprint_bevel_highlight",
            Self::FootprintBevelShadow => "footprint_bevel_shadow",
            Self::CrosshairInk => "crosshair_ink",
            Self::FootprintTotalInk => "footprint_total_ink",
            Self::LoadingLogoLow => "loading_logo_low",
            Self::LoadingLogoHigh => "loading_logo_high",
            Self::FlowBuy => "flow_buy",
            Self::FlowSell => "flow_sell",
            Self::TapePath => "tape_path",
            Self::TapeLabel => "tape_label",
            Self::QuantityShadow => "quantity_shadow",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|token| token.key() == key)
    }
}
