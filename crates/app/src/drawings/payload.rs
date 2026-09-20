//! Tool-owned state: the port a tool's own properties travel through.
//!
//! Kept apart from the object model so a tool that only needs the payload
//! port reads this file and not the whole subsystem envelope.

use std::any::Any;
use std::fmt;

/// Tool-owned state beyond anchors and common style. A property unique to
/// one tool lives in that tool's payload, never in the shared envelope, so
/// the next tool cannot force the model or the central inspector open.
pub trait DrawingPayload: fmt::Debug {
    fn clone_box(&self) -> Box<dyn DrawingPayload>;
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    /// Serialize the payload for a named preset. Coordinates, lock and
    /// visibility never travel with a preset, only the tool-owned config.
    fn export_preset(&self) -> Option<toml::Value> {
        None
    }
    /// Apply a previously exported preset. `false` leaves the payload alone.
    fn import_preset(&mut self, _value: &toml::Value) -> bool {
        false
    }
}

impl Clone for Box<dyn DrawingPayload> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Payload of tools whose whole state is anchors + common style.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NoPayload;

impl DrawingPayload for NoPayload {
    fn clone_box(&self) -> Box<dyn DrawingPayload> {
        Box::new(Self)
    }
    fn eq_dyn(&self, other: &dyn DrawingPayload) -> bool {
        other.as_any().downcast_ref::<Self>().is_some()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
