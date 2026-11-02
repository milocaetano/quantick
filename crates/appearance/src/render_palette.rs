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
    pub fn canvas(&self) -> T {
        *self.get(Token::Canvas)
    }
    pub fn chrome(&self) -> T {
        *self.get(Token::Chrome)
    }
    pub fn inset(&self) -> T {
        *self.get(Token::Inset)
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
}
