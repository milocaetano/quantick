use crate::{ChartLayer, LayerBlock, LayerRegistry, LayerScope, LayerSource, Requirement, blocks};

/// Facts needed by policy, supplied by the caller; no clock or feature state is owned here.
#[derive(Debug, Default, Clone, Copy)]
pub struct LayerFacts {
    pub flow_pane: bool,
    pub tape_on: bool,
    pub book_capture: bool,
    pub traded_volume: bool,
    pub capture_enabled: bool,
    pub depth_visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerEffect {
    pub source: LayerSource,
    pub visible: bool,
    pub write: VisibilityWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityWrite {
    Change,
    /// Establish an opening baseline, not an undoable user operation.
    Opening,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerError {
    UnknownLayer,
}

/// Only otherwise unowned requested switches live here. Delegated switches
/// return effects and are read from the authoritative feature on every query.
#[derive(Debug, Clone)]
pub struct LayerState {
    registry: LayerRegistry,
    requested: u32,
}

impl Default for LayerState {
    fn default() -> Self {
        Self::new(LayerRegistry::default())
    }
}

impl LayerState {
    pub fn new(registry: LayerRegistry) -> Self {
        let requested = registry
            .layers()
            .iter()
            .enumerate()
            .filter(|(_, layer)| layer.0.source == LayerSource::Local && layer.0.default_on)
            .fold(0, |mask, (slot, _)| mask | (1u32 << slot));
        Self {
            registry,
            requested,
        }
    }
    pub fn registry(&self) -> &LayerRegistry {
        &self.registry
    }
    /// Supported introspection for diagnostics and independent consumer tests.
    /// Bits describe only locally owned requested switches, in this state's
    /// registry order; they are not stable persisted IDs and cannot be compared
    /// across differently ordered registries. External owners are not mirrored.
    pub fn local_mask(&self) -> u32 {
        self.requested
    }
    pub fn requested(&self, layer: ChartLayer) -> bool {
        self.registry
            .bit(layer)
            .is_some_and(|bit| self.requested & bit != 0)
    }
    pub fn set(
        &mut self,
        layer: ChartLayer,
        visible: bool,
    ) -> Result<Option<LayerEffect>, LayerError> {
        let Some(bit) = self.registry.bit(layer) else {
            return Err(LayerError::UnknownLayer);
        };
        if layer.0.source != LayerSource::Local {
            return Ok(Some(LayerEffect {
                source: layer.0.source,
                visible,
                write: VisibilityWrite::Change,
            }));
        }
        if visible {
            self.requested |= bit;
        } else {
            self.requested &= !bit;
        }
        Ok(None)
    }
    /// Inherit a new pane's opening view without taking ownership of preset
    /// state, window-wide state, or the external owner's undo history.
    pub fn initialize(
        &mut self,
        layer: ChartLayer,
        visible: bool,
        facts: LayerFacts,
    ) -> Result<Option<LayerEffect>, LayerError> {
        if !self.registry.contains(layer) {
            return Err(LayerError::UnknownLayer);
        }
        if !layer.persisted() || !Self::restorable(layer, facts) {
            return Ok(None);
        }
        Ok(self.set(layer, visible)?.map(|mut effect| {
            effect.write = VisibilityWrite::Opening;
            effect
        }))
    }
    pub fn draws(layer: ChartLayer, facts: LayerFacts) -> bool {
        layer.0.scope != LayerScope::FlowPane || facts.flow_pane
    }
    /// Existing switch readback: tape switches retain their answer when the
    /// tape is off; only depth visibility also reflects capture being enabled.
    pub fn visible(layer: ChartLayer, requested: bool, facts: LayerFacts) -> bool {
        requested
            && Self::draws(layer, facts)
            && (!layer.0.capture_gates_visibility || facts.capture_enabled)
    }
    pub fn effective(layer: ChartLayer, requested: bool, facts: LayerFacts) -> bool {
        Self::visible(layer, requested, facts) && Self::blocked(layer, facts).is_none()
    }
    pub fn blocked(layer: ChartLayer, facts: LayerFacts) -> Option<LayerBlock> {
        if !Self::draws(layer, facts) {
            return Some(blocks::WRONG_PANE);
        }
        if layer.0.needs_tape && !facts.tape_on {
            return Some(blocks::TAPE_OFF);
        }
        let missing = match layer.0.requirement {
            Requirement::None => None,
            Requirement::Book => (!facts.book_capture).then_some(blocks::NO_BOOK),
            Requirement::Volume => (!facts.traded_volume).then_some(blocks::NO_TRADED_VOLUME),
            Requirement::BookOrVolume => (!facts.book_capture && !facts.traded_volume)
                .then_some(blocks::NO_BOOK_AND_NO_VOLUME),
        };
        missing.or_else(|| {
            (layer.0.needs_depth && !facts.depth_visible).then_some(blocks::DEPTH_MAP_HIDDEN)
        })
    }
    pub fn restorable(layer: ChartLayer, facts: LayerFacts) -> bool {
        layer.0.scope != LayerScope::Window && Self::draws(layer, facts)
    }
    pub fn requested_mask(&self, mut read: impl FnMut(ChartLayer) -> bool) -> u32 {
        self.registry
            .layers()
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, layer)| layer.persisted() && read(*layer))
            .fold(0, |mask, (slot, _)| mask | (1u32 << slot))
    }
    pub fn projection_demand(&self, mut read: impl FnMut(ChartLayer) -> bool) -> bool {
        self.registry
            .layers()
            .iter()
            .copied()
            .any(|layer| layer.0.projection_demand && read(layer))
    }
}
