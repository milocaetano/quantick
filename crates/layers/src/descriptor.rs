use crate::builtins;

/// The supported compact state budget. Registration rejects overflow and aliasing.
pub const MAX_LAYERS: usize = u32::BITS as usize;
pub const MAX_LAYER_ID_BYTES: usize = 64;
pub const MAX_LAYER_LABEL_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrderflowSwitch {
    Tape,
    TapeDepth,
    TapeBubbles,
    Depth,
    Bubbles,
    Marks,
    Legend,
    Status,
    Gaps,
}

/// The authority that already owns a requested visibility value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayerSource {
    Local,
    Orderflow(OrderflowSwitch),
    Footprint,
    Grid,
    Drawings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayerScope {
    Pane,
    FlowPane,
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Persistence {
    Layers,
    OrderflowPreset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Requirement {
    None,
    Book,
    Volume,
    BookOrVolume,
}

/// Data shared by menu, state transitions, persistence and control discovery.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LayerDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub source: LayerSource,
    pub scope: LayerScope,
    pub persistence: Persistence,
    pub requirement: Requirement,
    pub on_tape: bool,
    pub needs_tape: bool,
    pub needs_depth: bool,
    pub capture_gates_visibility: bool,
    pub default_on: bool,
    pub projection_demand: bool,
}

/// A registered identity, including extension identities; never a closed enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChartLayer(pub &'static LayerDescriptor);

impl ChartLayer {
    pub const ALL: [Self; 21] = builtins::ALL;
    pub const fn id(self) -> &'static str {
        self.0.id
    }
    pub const fn label(self) -> &'static str {
        self.0.label
    }
    pub const fn hint(self) -> &'static str {
        self.0.hint
    }
    pub const fn on_tape(self) -> bool {
        self.0.on_tape
    }
    pub const fn persisted(self) -> bool {
        matches!(self.0.persistence, Persistence::Layers)
    }
    pub fn from_id(id: &str) -> Option<Self> {
        LayerRegistry::default().resolve(id)
    }
}

/// Registration assigns compact positions internally; extensions supply stable IDs only.
#[derive(Debug, Clone)]
pub struct LayerRegistry {
    layers: std::sync::Arc<[ChartLayer]>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationError {
    TooManyLayers,
    DuplicateId,
    InvalidId,
    InvalidLabel,
}
impl Default for LayerRegistry {
    fn default() -> Self {
        static BUILTINS: std::sync::OnceLock<LayerRegistry> = std::sync::OnceLock::new();
        BUILTINS
            .get_or_init(|| Self::new(builtins::ALL).expect("valid built-in layer catalog"))
            .clone()
    }
}
impl LayerRegistry {
    pub fn new(layers: impl IntoIterator<Item = ChartLayer>) -> Result<Self, RegistrationError> {
        let layers: Vec<_> = layers.into_iter().collect();
        if layers.len() > MAX_LAYERS {
            return Err(RegistrationError::TooManyLayers);
        }
        for (index, layer) in layers.iter().enumerate() {
            if layer.id().is_empty()
                || layer.id().len() > MAX_LAYER_ID_BYTES
                || !layer
                    .id()
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            {
                return Err(RegistrationError::InvalidId);
            }
            if layer.label().trim().is_empty()
                || layer.label().len() > MAX_LAYER_LABEL_BYTES
                || layer.label().chars().any(char::is_control)
            {
                return Err(RegistrationError::InvalidLabel);
            }
            if layers[..index].iter().any(|other| other.id() == layer.id()) {
                return Err(RegistrationError::DuplicateId);
            }
        }
        Ok(Self {
            layers: layers.into(),
        })
    }
    pub fn layers(&self) -> &[ChartLayer] {
        &self.layers
    }
    pub fn resolve(&self, id: &str) -> Option<ChartLayer> {
        self.layers.iter().copied().find(|layer| layer.id() == id)
    }
    pub fn bit(&self, layer: ChartLayer) -> Option<u32> {
        self.layers
            .iter()
            .position(|registered| *registered == layer)
            .map(|index| 1u32 << index)
    }
    pub fn contains(&self, layer: ChartLayer) -> bool {
        self.bit(layer).is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerBlock {
    /// The stable wire code. Never the sentence below: a client made to parse
    /// prose breaks the day it is reworded, and translating the interface
    /// would break every such client at once.
    pub code: &'static str,
    /// What the disabled control tells a human.
    pub explanation: &'static str,
}

impl LayerBlock {
    pub const fn new(code: &'static str, explanation: &'static str) -> Self {
        Self { code, explanation }
    }
}

/// Stable availability reasons, named
/// once so UI and control consumers cannot assign different codes.
pub mod blocks {
    use super::LayerBlock;

    pub const WRONG_PANE: LayerBlock = LayerBlock::new(
        "the_order_flow_layers_are_drawn_on_the_flow_pane",
        "the order-flow layers are drawn on the flow pane",
    );
    pub const TAPE_OFF: LayerBlock = LayerBlock::new(
        "the_tape_is_off",
        "the tape is off — the switch in the canvas's top-right corner puts it back, \
         and this layer is waiting exactly as it was left",
    );
    pub const NO_BOOK: LayerBlock = LayerBlock::new(
        "source_captures_no_order_book",
        "order-book capture is not available for this source",
    );
    pub const NO_TRADED_VOLUME: LayerBlock = LayerBlock::new(
        "source_prints_no_traded_volume",
        "this source quotes prices but prints no traded volume",
    );
    pub const NO_BOOK_AND_NO_VOLUME: LayerBlock = LayerBlock::new(
        "source_publishes_neither_book_nor_traded_volume",
        "this source publishes neither an order book nor traded volume, \
         so the strip would have nothing to draw",
    );
    pub const DEPTH_MAP_HIDDEN: LayerBlock = LayerBlock::new(
        "the_depth_map_it_reports_on_is_hidden",
        "the badge reports on the depth map, which is hidden",
    );
}
