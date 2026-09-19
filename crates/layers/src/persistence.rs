/// Persistence tracks the active chart's requested switches, not its capabilities.
/// A tab switch rebases without attributing another chart's settings to a user edit.
#[derive(Debug, Default)]
pub struct SavedLayers {
    mask: u32,
    tab: u64,
    pub actions: LayerActions,
}

impl SavedLayers {
    /// Seed a baseline for independent consumers and integration tests.
    /// `mask` must use the same registry ordering as subsequent observations;
    /// this tracker compares bits, not stable layer IDs. Persist IDs through
    /// `LayerDocument` instead of storing this registry-relative mask.
    pub fn new(tab: u64, mask: u32) -> Self {
        Self {
            tab,
            mask,
            actions: LayerActions::default(),
        }
    }
    /// Supported diagnostic/test readback of the last recorded baseline.
    /// Interpret these bits only with the registry ordering that produced them.
    pub fn mask(&self) -> u32 {
        self.mask
    }
    /// Supported diagnostic/test readback of the baseline's caller-supplied tab
    /// identity, so consumers can verify rebasing without accessing storage.
    pub fn tab(&self) -> u64 {
        self.tab
    }
    pub fn rebaseline(&mut self, tab: u64, mask: u32) {
        self.tab = tab;
        self.mask = mask;
    }
    pub fn record(&mut self, mask: u32) {
        self.mask = mask;
    }
    /// Returns the switches to persist, or no write for an unchanged/new chart.
    pub fn observe(&mut self, tab: u64, mask: u32) -> Option<u32> {
        if tab != self.tab {
            self.rebaseline(tab, mask);
            return None;
        }
        let flipped = self.mask ^ mask;
        (flipped != 0).then_some(flipped)
    }
}

/// Effects requested by pane-local controls and executed by the window shell.
/// Indicator and footprint data stay with their existing authoritative owners.
#[derive(Debug, Default)]
pub struct LayerActions {
    pub grid: Option<bool>,
    pub indicators_changed: bool,
    pub footprint_changed: bool,
    pub open_footprint_settings: bool,
}
