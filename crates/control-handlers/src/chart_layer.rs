//! One pane's layer as its menu reads it: what `layers.visibility.set` asks
//! the window for ([`crate::chart::ChartPort::chart_layer`]).
use quantick_control_schema::layers::LayerSnapshot;

/// One layer of one pane, as its menu reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRead {
    pub layer: LayerSnapshot,
    /// Why the menu greys the switch out, when it does.
    pub blocked: Option<LayerBlocked>,
}

/// Why a layer cannot be switched on this pane right now.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LayerBlocked {
    /// The stable wire code a client branches on.
    pub code: &'static str,
    /// What the greyed-out switch tells a human.
    pub explanation: &'static str,
}
