//! Each built-in package owns its descriptor and builder factory.
// A package docks here with one line; the module, export and catalog stay in sync.
macro_rules! register {
    ($($module:ident => $definition:ident),+ $(,)?) => {
        $(mod $module; pub use $module::$definition;)+
        pub const DEFINITIONS: &[&super::BarDefinition] = &[$(&$definition),+];
    };
}
register! {
    tick => TICK,
    volume => VOLUME,
    dollar => DOLLAR,
    time => TIME,
    imbalance => IMBALANCE,
    trades => TRADES,
}

use super::{InputRequirements, NumberEditor};
pub const PRINTS: InputRequirements = InputRequirements {
    traded_volume: false,
    deal_counter: false,
};
pub const VOLUME_INPUT: InputRequirements = InputRequirements {
    traded_volume: true,
    deal_counter: false,
};
const COUNT_EDITOR: NumberEditor = NumberEditor {
    label: "N ticks",
    min: 1.0,
    max: 5000.0,
    step: 1.0,
    decimals: None,
    hover: "",
    presets: &[],
};
