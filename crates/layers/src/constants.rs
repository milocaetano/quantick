//! Registration budgets of the layer catalog. One place for later config
//! wiring; `descriptor.rs` re-exports them under their public names.

// Layer registration (`descriptor.rs`).

/// The supported compact state budget. Registration rejects overflow and aliasing.
pub const MAX_LAYERS: usize = u32::BITS as usize;

/// Longest layer ID a registration accepts, in UTF-8 bytes.
pub const MAX_LAYER_ID_BYTES: usize = 64;

/// Longest layer label a registration accepts, in UTF-8 bytes.
pub const MAX_LAYER_LABEL_BYTES: usize = 128;
