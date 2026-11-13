#[cfg(test)]
/// The actual paper owner and frame facts needed by a paper panel.
pub(crate) struct PaperPanelEnv<'a> {
    pub paper: &'a mut crate::paper_trading::PaperTrading,
    pub replay: Option<&'a quantick_feed::ReplayLink>,
    pub timezone: crate::timezone::TzOffset,
}
#[cfg(test)]
/// A pass lends one domain facet; models and responses never retain it.
pub(crate) enum BorrowedSurfaceEnv<'a> {
    Paper(PaperPanelEnv<'a>),
    Drawings(&'a super::DrawingEnv<'a>),
}
