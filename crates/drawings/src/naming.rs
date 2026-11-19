//! Every mark this store holds answers to a name: the trader's own, or the
//! one it got when placed, pasted or cleared — `zone-1`, `line-2`
//! ([`quantick_workspace::drawing_names`]). The store reads its market's
//! floors from the market store ([`Drawings::reserve_names`]) so a mark on
//! one chart is not named like another chart's.

use crate::{Drawing, Drawings, ToolCatalog};
use quantick_workspace::drawing_names::{self, NameFloors};

impl<T: ToolCatalog> Drawings<T> {
    /// Never name a mark at or below `floors`: the generated numbers its
    /// market's store already uses.
    pub fn reserve_names(&mut self, floors: &NameFloors) {
        for (prefix, floor) in floors {
            let held = self.name_floors.entry(prefix.clone()).or_insert(0);
            *held = (*held).max(*floor);
        }
    }

    /// The next free name for a mark of `tool`, taken.
    pub(crate) fn take_name(&mut self, tool: T) -> String {
        drawing_names::raise_floors(
            &mut self.name_floors,
            (self.items.iter()).map(|mark| (mark.tool.id(), mark.name.as_deref())),
        );
        drawing_names::next_name(&mut self.name_floors, tool.id())
    }

    /// `drawing` with a name: its own, or the next free one.
    pub(crate) fn named(&mut self, mut drawing: Drawing<T>) -> Drawing<T> {
        if drawing.name.is_none() {
            drawing.name = Some(self.take_name(drawing.tool));
        }
        drawing
    }

    /// Rename the object at `index` as one undo step. Whitespace-only input
    /// gives it back a generated name, never none.
    pub fn rename_at(&mut self, index: usize, name: &str) {
        if index >= self.items.len() {
            return;
        }
        let trimmed = name.trim();
        let name = if trimmed.is_empty() {
            self.take_name(self.items[index].tool)
        } else {
            trimmed.to_owned()
        };
        let before = self.snapshot();
        self.items[index].name = Some(name);
        self.record(before);
    }
}

#[cfg(test)]
#[path = "naming_tests.rs"]
mod tests;
