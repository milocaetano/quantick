//! One tab's canvas: the docked tree, which panes are folded or maximized,
//! and which float — over the canvas or in their own OS window — with every
//! transition the pane menu, the add button and a drop can ask for.
//!
//! Saved inside the workspace, so a change here marks it unsaved like any
//! other workspace edit. Transitions answer whether anything changed, so a
//! repeated call is safe and says it did nothing.

use super::drop::{Corner, Edge, park_rect};
use super::presets::{LayoutPreset, MAX_CANVAS_PANES, PaneKind, matching_preset, timeframes_with};
use super::tree::{CanvasRect, PaneSlot, PaneTree, SplitAxis};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// How thick a minimized pane's strip is along its split.
pub const MINIMIZED_STRIP_PX: f32 = 24.0;
/// A new floating pane's size, as a share of the canvas on each axis.
const NEW_FLOAT_SHARE: f32 = 0.5;

/// Where a floating pane is shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "host", rename_all = "snake_case")]
pub enum FloatHost {
    /// Over the canvas, at `rect` in canvas points.
    Canvas { rect: CanvasRect },
    /// In its own OS window on `monitor`, at `rect` in that monitor's points.
    /// Restored on the canvas when the monitor is gone.
    OsWindow { monitor: String, rect: CanvasRect },
}

/// A pane outside the docked tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FloatingPane {
    pub slot: PaneSlot,
    pub kind: PaneKind,
    pub host: FloatHost,
}

/// How a pane is shown, for whoever reads the canvas back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneMode {
    Docked,
    Minimized,
    Maximized,
    Floating,
    OsWindow,
}

/// Why a canvas transition was refused. `code` is stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasRefusal {
    UnknownPane(PaneSlot),
    UnknownPreset(String),
    UnknownWidget(String),
    /// The pane must be in the docked tree for this.
    NotDocked(PaneSlot),
    /// The pane must be floating for this.
    NotFloating(PaneSlot),
    /// The pane must be in its own OS window for this.
    NotInOsWindow(PaneSlot),
    /// The canvas keeps at least one docked pane.
    LastDockedPane,
    /// The canvas keeps at least one docked pane unfolded.
    LastOpenPane,
    /// The canvas already holds [`MAX_CANVAS_PANES`].
    Full,
    /// No seam there can move: it needs two unfolded docked neighbours along
    /// a split, and no chart maximized over the canvas.
    NoSeam,
    /// The two charts beside the seam are narrower than the least each keeps
    /// together: the canvas has no room to move it.
    NoRoom,
}

impl CanvasRefusal {
    /// The code a caller branches on.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownPane(_) => "canvas.unknown_pane",
            Self::UnknownPreset(_) => "canvas.unknown_preset",
            Self::UnknownWidget(_) => "canvas.unknown_widget",
            Self::NotDocked(_) => "canvas.not_docked",
            Self::NotFloating(_) => "canvas.not_floating",
            Self::NotInOsWindow(_) => "canvas.not_in_os_window",
            Self::LastDockedPane => "canvas.last_docked_pane",
            Self::LastOpenPane => "canvas.last_open_pane",
            Self::Full => "canvas.full",
            Self::NoSeam => "canvas.no_seam",
            Self::NoRoom => "canvas.no_room",
        }
    }
}

impl std::fmt::Display for CanvasRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownPane(slot) => write!(f, "the canvas holds no pane {slot}"),
            Self::UnknownPreset(id) => write!(f, "no layout is called {id}"),
            Self::UnknownWidget(id) => write!(f, "no widget is called {id}"),
            Self::NotDocked(slot) => write!(f, "pane {slot} is not docked in the canvas"),
            Self::NotFloating(slot) => write!(f, "pane {slot} is not floating"),
            Self::NotInOsWindow(slot) => write!(f, "pane {slot} is not in its own window"),
            Self::LastDockedPane => f.write_str("the canvas keeps at least one docked chart"),
            Self::LastOpenPane => f.write_str("the canvas keeps at least one chart unfolded"),
            Self::Full => write!(f, "the canvas holds at most {MAX_CANVAS_PANES} charts"),
            Self::NoSeam => f.write_str(
                "no seam there can move: it needs two unfolded docked charts side by side and none maximized",
            ),
            Self::NoRoom => f.write_str(
                "no room to move that seam: the charts beside it are narrower than the least each keeps",
            ),
        }
    }
}

/// One tab's canvas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasState {
    /// The next slot a new pane takes. Only rises, so a slot the saved
    /// workspace holds is never handed to a different pane.
    next_slot: PaneSlot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    maximized: Option<PaneSlot>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    minimized: BTreeSet<PaneSlot>,
    tree: PaneTree,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    floating: Vec<FloatingPane>,
}

impl CanvasState {
    /// A canvas laid out as `preset`, nothing folded or floating.
    #[must_use]
    pub fn from_preset(preset: &LayoutPreset) -> Self {
        Self {
            next_slot: preset.kinds.len() as PaneSlot,
            maximized: None,
            minimized: BTreeSet::new(),
            tree: preset.tree.clone(),
            floating: Vec::new(),
        }
    }

    #[must_use]
    pub fn tree(&self) -> &PaneTree {
        &self.tree
    }

    /// The docked tree, for the seam resize in [`super::seams`].
    pub(super) fn tree_mut(&mut self) -> &mut PaneTree {
        &mut self.tree
    }

    #[must_use]
    pub fn minimized(&self) -> &BTreeSet<PaneSlot> {
        &self.minimized
    }

    #[must_use]
    pub fn maximized(&self) -> Option<PaneSlot> {
        self.maximized
    }

    #[must_use]
    pub fn floating(&self) -> &[FloatingPane] {
        &self.floating
    }

    /// The preset the docked tree is, or `None` for a custom arrangement.
    #[must_use]
    pub fn preset(&self) -> Option<&'static LayoutPreset> {
        matching_preset(&self.tree)
    }

    /// Whether `slot` is on this canvas, docked or floating.
    #[must_use]
    pub fn holds(&self, slot: PaneSlot) -> bool {
        self.tree.holds(slot) || self.floating_index(slot).is_some()
    }

    /// How `slot` is shown, if the canvas holds it.
    #[must_use]
    pub fn mode(&self, slot: PaneSlot) -> Option<PaneMode> {
        if self.tree.holds(slot) {
            return Some(if self.maximized == Some(slot) {
                PaneMode::Maximized
            } else if self.minimized.contains(&slot) {
                PaneMode::Minimized
            } else {
                PaneMode::Docked
            });
        }
        let pane = &self.floating[self.floating_index(slot)?];
        Some(match pane.host {
            FloatHost::Canvas { .. } => PaneMode::Floating,
            FloatHost::OsWindow { .. } => PaneMode::OsWindow,
        })
    }

    /// Every pane with its kind and mode, in slot order.
    #[must_use]
    pub fn panes(&self) -> Vec<(PaneSlot, PaneKind, PaneMode)> {
        let docked = self.tree.panes().into_iter();
        let floating = self.floating.iter().map(|pane| (pane.slot, pane.kind));
        let mut out: Vec<_> = docked
            .chain(floating)
            .filter_map(|(slot, kind)| Some((slot, kind, self.mode(slot)?)))
            .collect();
        out.sort_by_key(|(slot, _, _)| *slot);
        out
    }

    /// The docked panes' rectangles in `area`, in slot order: the maximized
    /// pane alone over all of it, or the tree with each minimized pane held
    /// to a [`MINIMIZED_STRIP_PX`] strip.
    #[must_use]
    pub fn rects(&self, area: CanvasRect) -> Vec<(PaneSlot, CanvasRect)> {
        if let Some(slot) = self.maximized {
            return vec![(slot, area)];
        }
        let minimized = &self.minimized;
        self.tree.rects_with(area, &|slot| {
            minimized.contains(&slot).then_some(MINIMIZED_STRIP_PX)
        })
    }

    /// Lay the docked panes out as `preset`. Panes already docked keep their
    /// slots where the preset has a place of their kind, in reading order; a
    /// place left over opens a new pane. Folds and the maximized pane are
    /// cleared; floating panes stay.
    pub fn apply_preset(&mut self, preset: &LayoutPreset) -> bool {
        let mut existing = self.tree.panes();
        let mut next = self.next_slot;
        let slots: Vec<PaneSlot> = preset
            .kinds
            .iter()
            .map(
                |kind| match existing.iter().position(|(_, held)| held == kind) {
                    Some(index) => existing.remove(index).0,
                    None => {
                        next += 1;
                        next - 1
                    }
                },
            )
            .collect();
        let tree = preset.tree.relabelled(&|slot| slots[slot as usize]);
        let changed = tree != self.tree || self.maximized.is_some() || !self.minimized.is_empty();
        self.tree = tree;
        self.next_slot = next;
        self.maximized = None;
        self.minimized.clear();
        changed
    }

    /// Fold a docked pane to its strip; the others take the space.
    ///
    /// # Errors
    /// Not docked, or the last unfolded pane.
    pub fn minimize(&mut self, slot: PaneSlot) -> Result<bool, CanvasRefusal> {
        self.docked(slot)?;
        if self.minimized.contains(&slot) {
            return Ok(false);
        }
        if self.tree.len() - self.minimized.len() <= 1 {
            return Err(CanvasRefusal::LastOpenPane);
        }
        if self.maximized == Some(slot) {
            self.maximized = None;
        }
        self.minimized.insert(slot);
        Ok(true)
    }

    /// Fill the canvas with a docked pane until it is restored.
    ///
    /// # Errors
    /// Not docked.
    pub fn maximize(&mut self, slot: PaneSlot) -> Result<bool, CanvasRefusal> {
        self.docked(slot)?;
        if self.maximized == Some(slot) {
            return Ok(false);
        }
        self.minimized.remove(&slot);
        self.maximized = Some(slot);
        Ok(true)
    }

    /// Unfold a minimized pane, or bring a maximized one back to its place.
    ///
    /// # Errors
    /// The canvas holds no such pane.
    pub fn restore(&mut self, slot: PaneSlot) -> Result<bool, CanvasRefusal> {
        if !self.holds(slot) {
            return Err(CanvasRefusal::UnknownPane(slot));
        }
        if self.minimized.remove(&slot) {
            return Ok(true);
        }
        if self.maximized == Some(slot) {
            self.maximized = None;
            return Ok(true);
        }
        Ok(false)
    }

    /// Take a pane off the canvas. Removing the last docked flow pane lays
    /// the remaining charts out as the TIMEFRAMES preset of their count.
    ///
    /// # Errors
    /// No such pane, or the last docked one.
    pub fn remove(&mut self, slot: PaneSlot) -> Result<bool, CanvasRefusal> {
        if let Some(index) = self.floating_index(slot) {
            self.floating.remove(index);
            return Ok(true);
        }
        self.take_docked(slot)?;
        let docked = self.tree.panes();
        if !docked.iter().any(|(_, kind)| *kind == PaneKind::Flow)
            && let Some(preset) = timeframes_with(docked.len())
            && matching_preset(&self.tree).is_none()
        {
            self.tree = preset.tree.relabelled(&|place| docked[place as usize].0);
        }
        Ok(true)
    }

    /// Open a new floating pane of `kind`, centred over `canvas`. Answers its
    /// slot.
    ///
    /// # Errors
    /// The canvas is full.
    pub fn add(&mut self, kind: PaneKind, canvas: CanvasRect) -> Result<PaneSlot, CanvasRefusal> {
        if self.tree.len() + self.floating.len() >= MAX_CANVAS_PANES {
            return Err(CanvasRefusal::Full);
        }
        let slot = self.next_slot;
        self.next_slot += 1;
        let size = CanvasRect::new(
            0.0,
            0.0,
            canvas.w * NEW_FLOAT_SHARE,
            canvas.h * NEW_FLOAT_SHARE,
        );
        self.floating.push(FloatingPane {
            slot,
            kind,
            host: FloatHost::Canvas {
                rect: size.centred_in(canvas),
            },
        });
        Ok(slot)
    }

    /// Float a pane over the canvas at `rect`: a docked one leaves the tree, a
    /// floating one moves, one in its own window comes back to the canvas.
    ///
    /// # Errors
    /// No such pane, or the last docked one.
    pub fn float(&mut self, slot: PaneSlot, rect: CanvasRect) -> Result<bool, CanvasRefusal> {
        self.set_host(slot, FloatHost::Canvas { rect })
    }

    /// Insert a floating pane into the docked tree at `edge`.
    ///
    /// # Errors
    /// The pane is not floating.
    pub fn dock(&mut self, slot: PaneSlot, edge: Edge) -> Result<bool, CanvasRefusal> {
        let index = self
            .floating_index(slot)
            .ok_or_else(|| self.missing_or(slot, CanvasRefusal::NotFloating(slot)))?;
        let pane = self.floating.remove(index);
        let (axis, at_start) = match edge {
            Edge::Left => (SplitAxis::Row, true),
            Edge::Right => (SplitAxis::Row, false),
            Edge::Top => (SplitAxis::Column, true),
            Edge::Bottom => (SplitAxis::Column, false),
        };
        self.tree = self
            .tree
            .docked(PaneTree::pane(pane.slot, pane.kind), axis, at_start);
        self.maximized = None;
        Ok(true)
    }

    /// Snap a pane floating over the canvas into `corner` of `canvas`,
    /// keeping its size.
    ///
    /// # Errors
    /// The pane is not floating over the canvas.
    pub fn park(
        &mut self,
        slot: PaneSlot,
        corner: Corner,
        canvas: CanvasRect,
    ) -> Result<bool, CanvasRefusal> {
        let not_floating = self.missing_or(slot, CanvasRefusal::NotFloating(slot));
        let index = self.floating_index(slot).ok_or(not_floating.clone())?;
        let FloatHost::Canvas { rect } = self.floating[index].host else {
            return Err(not_floating);
        };
        let parked = park_rect(canvas, corner, (rect.w, rect.h));
        self.floating[index].host = FloatHost::Canvas { rect: parked };
        Ok(parked != rect)
    }

    /// Put a pane in its own OS window on `monitor` at `rect`.
    ///
    /// # Errors
    /// No such pane, or the last docked one.
    pub fn pop_out(
        &mut self,
        slot: PaneSlot,
        monitor: &str,
        rect: CanvasRect,
    ) -> Result<bool, CanvasRefusal> {
        self.set_host(
            slot,
            FloatHost::OsWindow {
                monitor: monitor.to_owned(),
                rect,
            },
        )
    }

    /// Bring a pane from its own window back over the canvas, centred, its
    /// size kept as far as the canvas allows.
    ///
    /// # Errors
    /// The pane is not in its own window.
    pub fn return_to_canvas(
        &mut self,
        slot: PaneSlot,
        canvas: CanvasRect,
    ) -> Result<bool, CanvasRefusal> {
        let index = self
            .floating_index(slot)
            .filter(|index| matches!(self.floating[*index].host, FloatHost::OsWindow { .. }))
            .ok_or_else(|| self.missing_or(slot, CanvasRefusal::NotInOsWindow(slot)))?;
        let FloatHost::OsWindow { rect, .. } = self.floating[index].host else {
            unreachable!("filtered to an OS window above");
        };
        let size = CanvasRect::new(0.0, 0.0, rect.w.min(canvas.w), rect.h.min(canvas.h));
        self.floating[index].host = FloatHost::Canvas {
            rect: size.centred_in(canvas),
        };
        Ok(true)
    }

    /// Bring every pane whose OS window sat on a monitor not in `monitors`
    /// back over `canvas`, centred, its size kept as far as the canvas
    /// allows. Answers the slots it brought back, in slot order.
    pub fn settle_monitors(&mut self, monitors: &[String], canvas: CanvasRect) -> Vec<PaneSlot> {
        let gone: Vec<PaneSlot> = (self.floating.iter())
            .filter(|pane| match &pane.host {
                FloatHost::OsWindow { monitor, .. } => !monitors.contains(monitor),
                FloatHost::Canvas { .. } => false,
            })
            .map(|pane| pane.slot)
            .collect();
        for slot in &gone {
            let _ = self.return_to_canvas(*slot, canvas);
        }
        gone
    }

    /// Whether this canvas is `preset` as the preset itself numbers it: what a
    /// save may leave out and rebuild from the preset's name alone.
    #[must_use]
    pub fn numbered_as(&self, preset: &LayoutPreset) -> bool {
        *self == Self::from_preset(preset)
    }

    /// Where a pane floats or stands in its own window, if it does.
    #[must_use]
    pub fn host(&self, slot: PaneSlot) -> Option<&FloatHost> {
        Some(&self.floating[self.floating_index(slot)?].host)
    }

    fn set_host(&mut self, slot: PaneSlot, host: FloatHost) -> Result<bool, CanvasRefusal> {
        if let Some(index) = self.floating_index(slot) {
            let changed = self.floating[index].host != host;
            self.floating[index].host = host;
            return Ok(changed);
        }
        let kind = self.take_docked(slot)?;
        self.floating.push(FloatingPane { slot, kind, host });
        Ok(true)
    }

    /// Take a docked pane out of the tree, dropping its fold.
    fn take_docked(&mut self, slot: PaneSlot) -> Result<PaneKind, CanvasRefusal> {
        let kind = self.docked(slot)?;
        let tree = self
            .tree
            .without(slot)
            .ok_or(CanvasRefusal::LastDockedPane)?;
        self.tree = tree;
        self.minimized.remove(&slot);
        if self.maximized == Some(slot) {
            self.maximized = None;
        }
        if self.minimized.len() >= self.tree.len() {
            self.minimized.clear();
        }
        Ok(kind)
    }

    pub(super) fn docked(&self, slot: PaneSlot) -> Result<PaneKind, CanvasRefusal> {
        self.tree
            .panes()
            .into_iter()
            .find(|(held, _)| *held == slot)
            .map(|(_, kind)| kind)
            .ok_or_else(|| self.missing_or(slot, CanvasRefusal::NotDocked(slot)))
    }

    fn floating_index(&self, slot: PaneSlot) -> Option<usize> {
        self.floating.iter().position(|pane| pane.slot == slot)
    }

    /// `refusal` for a pane the canvas holds in the wrong place, or
    /// [`CanvasRefusal::UnknownPane`] for one it does not hold at all.
    fn missing_or(&self, slot: PaneSlot, refusal: CanvasRefusal) -> CanvasRefusal {
        if self.holds(slot) {
            refusal
        } else {
            CanvasRefusal::UnknownPane(slot)
        }
    }
}

/// Whether removing `live`'s `slot` should ask first: only when `saved`, the
/// workspace as last saved, holds that pane. A pane added since goes quietly.
///
/// Compared by slot, the pane's identity for its life: a save keeps the
/// canvas whenever it is not [`CanvasState::numbered_as`] its preset, so a
/// saved tab and the live one always number their panes alike, and a slot
/// added since the save is one the save never held.
#[must_use]
pub fn remove_needs_confirm(saved: &CanvasState, live: &CanvasState, slot: PaneSlot) -> bool {
    saved.holds(slot) && live.holds(slot)
}
