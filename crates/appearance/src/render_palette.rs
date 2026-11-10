//! A renderer-owned representation of resolved semantic colors.
use crate::Token;

#[derive(Debug, Clone, PartialEq)]
pub struct RenderPalette<T>([T; Token::ALL.len()]);
impl<T> RenderPalette<T> {
    pub fn from_fn(convert: impl FnMut(Token) -> T) -> Self {
        Self(Token::ALL.map(convert))
    }
    pub fn get(&self, token: Token) -> &T {
        &self.0[token as usize]
    }
}
impl<T: Copy> RenderPalette<T> {
    pub fn flow_surface(&self) -> T {
        *self.get(Token::FlowSurface)
    }
    pub fn flow_divider(&self) -> T {
        *self.get(Token::FlowDivider)
    }
    pub fn flow_ink(&self) -> T {
        *self.get(Token::FlowInk)
    }
    pub fn flow_key_down(&self) -> T {
        *self.get(Token::FlowKeyDown)
    }
    pub fn flow_key_down_shade(&self) -> T {
        *self.get(Token::FlowKeyDownShade)
    }
    pub fn flow_key_up_ink(&self) -> T {
        *self.get(Token::FlowKeyUpInk)
    }
    pub fn flow_band_rule(&self) -> T {
        *self.get(Token::FlowBandRule)
    }
    pub fn flow_band(&self) -> T {
        *self.get(Token::FlowBand)
    }
    pub fn flow_key_lit_dot(&self) -> T {
        *self.get(Token::FlowKeyLitDot)
    }
    pub fn flow_key_unlit_dot(&self) -> T {
        *self.get(Token::FlowKeyUnlitDot)
    }
    pub fn flow_key_unlit_ink(&self) -> T {
        *self.get(Token::FlowKeyUnlitInk)
    }
    pub fn flow_gear_ink(&self) -> T {
        *self.get(Token::FlowGearInk)
    }

    pub fn trade_selection_halo(&self) -> T {
        *self.get(Token::TradeSelectionHalo)
    }
    pub fn candle_preview_background(&self) -> T {
        *self.get(Token::CandlePreviewBackground)
    }
    pub fn candle_preview_bid(&self) -> T {
        *self.get(Token::CandlePreviewBid)
    }
    pub fn candle_preview_ask(&self) -> T {
        *self.get(Token::CandlePreviewAsk)
    }
    pub fn candle_preview_buy(&self) -> T {
        *self.get(Token::CandlePreviewBuy)
    }
    pub fn candle_preview_sell(&self) -> T {
        *self.get(Token::CandlePreviewSell)
    }
    pub fn retained_tape_shade(&self) -> T {
        *self.get(Token::RetainedTapeShade)
    }

    pub fn text_shadow(&self) -> T {
        *self.get(Token::TextShadow)
    }
    pub fn replay_error(&self) -> T {
        *self.get(Token::ReplayError)
    }
    pub fn replay_inset(&self) -> T {
        *self.get(Token::ReplayInset)
    }
    pub fn replay_well(&self) -> T {
        *self.get(Token::ReplayWell)
    }
    pub fn error_text(&self) -> T {
        *self.get(Token::ErrorText)
    }
    pub fn action_ink(&self) -> T {
        *self.get(Token::ActionInk)
    }
    pub fn modal_scrim(&self) -> T {
        *self.get(Token::ModalScrim)
    }
    pub fn widget_open_border(&self) -> T {
        *self.get(Token::WidgetOpenBorder)
    }
    pub fn widget_active_border(&self) -> T {
        *self.get(Token::WidgetActiveBorder)
    }
    pub fn widget_noninteractive(&self) -> T {
        *self.get(Token::WidgetNoninteractive)
    }
    pub fn widget_shadow(&self) -> T {
        *self.get(Token::WidgetShadow)
    }
    pub fn widget_cursor(&self) -> T {
        *self.get(Token::WidgetCursor)
    }
    pub fn widget_error(&self) -> T {
        *self.get(Token::WidgetError)
    }
    pub fn widget_warn(&self) -> T {
        *self.get(Token::WidgetWarn)
    }
    pub fn widget_code(&self) -> T {
        *self.get(Token::WidgetCode)
    }
    pub fn drawing_readout_plate(&self) -> T {
        *self.get(Token::DrawingReadoutPlate)
    }
    pub fn drawing_selection_halo(&self) -> T {
        *self.get(Token::DrawingSelectionHalo)
    }
    pub fn side_color_for(&self, buy: bool) -> T {
        *self.get(if buy { Token::Buy } else { Token::Sell })
    }
    pub fn side_ink_for(&self, buy: bool) -> T {
        *self.get(if buy { Token::BuyInk } else { Token::SellInk })
    }

    pub fn drawing_swatches(&self) -> [T; 8] {
        [
            Token::Accent,
            Token::TextPrimary,
            Token::TextMuted,
            Token::Buy,
            Token::Sell,
            Token::Poc,
            Token::DrawViolet,
            Token::DrawCyan,
        ]
        .map(|token| *self.get(token))
    }
    pub fn flow_buy(&self) -> T {
        *self.get(Token::FlowBuy)
    }
    pub fn flow_sell(&self) -> T {
        *self.get(Token::FlowSell)
    }
    pub fn tape_path(&self) -> T {
        *self.get(Token::TapePath)
    }
    pub fn tape_label(&self) -> T {
        *self.get(Token::TapeLabel)
    }
    pub fn quantity_shadow(&self) -> T {
        *self.get(Token::QuantityShadow)
    }

    pub fn loading_title(&self) -> T {
        *self.get(Token::LoadingTitle)
    }
    pub fn loading_meta(&self) -> T {
        *self.get(Token::LoadingMeta)
    }
    pub fn loading_faint(&self) -> T {
        *self.get(Token::LoadingFaint)
    }
    pub fn loading_border(&self) -> T {
        *self.get(Token::LoadingBorder)
    }
    pub fn footprint_profile(&self) -> T {
        *self.get(Token::FootprintProfile)
    }
    pub fn footprint_bevel_highlight(&self) -> T {
        *self.get(Token::FootprintBevelHighlight)
    }
    pub fn footprint_bevel_shadow(&self) -> T {
        *self.get(Token::FootprintBevelShadow)
    }
    pub fn crosshair_ink(&self) -> T {
        *self.get(Token::CrosshairInk)
    }
    pub fn footprint_total_ink(&self) -> T {
        *self.get(Token::FootprintTotalInk)
    }
    pub fn loading_logo_low(&self) -> T {
        *self.get(Token::LoadingLogoLow)
    }
    pub fn loading_logo_high(&self) -> T {
        *self.get(Token::LoadingLogoHigh)
    }
    pub fn canvas(&self) -> T {
        *self.get(Token::Canvas)
    }
    pub fn chrome(&self) -> T {
        *self.get(Token::Chrome)
    }
    pub fn inset(&self) -> T {
        *self.get(Token::Inset)
    }
    pub fn tab_strip(&self) -> T {
        *self.get(Token::TabStrip)
    }
    pub fn brand(&self) -> T {
        *self.get(Token::Brand)
    }
    pub fn replay_ink(&self) -> T {
        *self.get(Token::ReplayInk)
    }
    pub fn control(&self) -> T {
        *self.get(Token::Control)
    }
    pub fn border(&self) -> T {
        *self.get(Token::Border)
    }
    pub fn text_primary(&self) -> T {
        *self.get(Token::TextPrimary)
    }
    pub fn text_muted(&self) -> T {
        *self.get(Token::TextMuted)
    }
    pub fn text_faint(&self) -> T {
        *self.get(Token::TextFaint)
    }
    pub fn buy(&self) -> T {
        *self.get(Token::Buy)
    }
    pub fn sell(&self) -> T {
        *self.get(Token::Sell)
    }
    pub fn poc(&self) -> T {
        *self.get(Token::Poc)
    }
    pub fn accent(&self) -> T {
        *self.get(Token::Accent)
    }
    pub fn amber(&self) -> T {
        *self.get(Token::Amber)
    }
    pub fn pane_mark(&self) -> T {
        *self.get(Token::PaneMark)
    }
    pub fn pane_action(&self) -> T {
        *self.get(Token::PaneAction)
    }
    pub fn off(&self) -> T {
        *self.get(Token::Off)
    }
    pub fn capsule_fill(&self) -> T {
        *self.get(Token::CapsuleFill)
    }
    pub fn capsule_border(&self) -> T {
        *self.get(Token::CapsuleBorder)
    }
    pub fn capsule_shadow(&self) -> T {
        *self.get(Token::CapsuleShadow)
    }
    pub fn bare_hover(&self) -> T {
        *self.get(Token::BareHover)
    }
    pub fn seam_line(&self) -> T {
        *self.get(Token::SeamLine)
    }
    pub fn day_rule(&self) -> T {
        *self.get(Token::DayRule)
    }
    pub fn seam_label(&self) -> T {
        *self.get(Token::SeamLabel)
    }
    pub fn warn(&self) -> T {
        *self.get(Token::Warn)
    }
    pub fn rec(&self) -> T {
        *self.get(Token::Rec)
    }
    pub fn gap_line(&self) -> T {
        *self.get(Token::GapLine)
    }
    pub fn gap_label(&self) -> T {
        *self.get(Token::GapLabel)
    }
    pub fn tag_bg(&self) -> T {
        *self.get(Token::TagBg)
    }
    pub fn text_support(&self) -> T {
        *self.get(Token::TextSupport)
    }
    pub fn chip_ink(&self) -> T {
        *self.get(Token::ChipInk)
    }
    pub fn casing(&self) -> T {
        *self.get(Token::Casing)
    }
    pub fn draw_violet(&self) -> T {
        *self.get(Token::DrawViolet)
    }
    pub fn draw_cyan(&self) -> T {
        *self.get(Token::DrawCyan)
    }
    pub fn float_shadow(&self) -> T {
        *self.get(Token::FloatShadow)
    }
    pub fn candle_bull_outline(&self) -> T {
        *self.get(Token::CandleBullOutline)
    }
    pub fn candle_bear_outline(&self) -> T {
        *self.get(Token::CandleBearOutline)
    }
    pub fn candle_wick(&self) -> T {
        *self.get(Token::CandleWick)
    }
    pub fn buy_ink(&self) -> T {
        *self.get(Token::BuyInk)
    }
    pub fn sell_ink(&self) -> T {
        *self.get(Token::SellInk)
    }
    pub fn replay_plate(&self) -> T {
        *self.get(Token::ReplayPlate)
    }
    pub fn replay_plate_ink(&self) -> T {
        *self.get(Token::ReplayPlateInk)
    }
    pub fn order_plate(&self) -> T {
        *self.get(Token::OrderPlate)
    }
    pub fn order_plate_ink(&self) -> T {
        *self.get(Token::OrderPlateInk)
    }
    pub fn plate_gain(&self) -> T {
        *self.get(Token::PlateGain)
    }
    pub fn plate_loss(&self) -> T {
        *self.get(Token::PlateLoss)
    }
    pub fn hover_fill(&self) -> T {
        *self.get(Token::HoverFill)
    }
    pub fn control_edge(&self) -> T {
        *self.get(Token::ControlEdge)
    }
    pub fn connection_action(&self) -> T {
        *self.get(Token::ConnectionAction)
    }
}
