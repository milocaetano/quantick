//! Every mark this store holds answers to a name: the trader's own, or the
//! one it got when placed, pasted or cleared — `zone-1`, `line-2`
//! ([`quantick_workspace::drawing_names`]). The store reads its market's
//! floors from the market store ([`Drawings::reserve_names`]) so a mark on
//! one chart is not named like another chart's.

use crate::{Drawing, Drawings, ToolCatalog};
use quantick_workspace::drawing_names::{self, NameFloors};
use quantick_workspace::market_settings::MarketKey;

impl<T: ToolCatalog> Drawings<T> {
    /// Never name a mark at or below `floors`: the generated numbers
    /// `market`'s store already uses. Floors rise while the store holds one
    /// market, and start over when it holds another.
    pub fn reserve_names(&mut self, market: &MarketKey, floors: &NameFloors) {
        if self.name_market.as_ref() != Some(market) {
            self.name_floors.clear();
            self.name_market = Some(market.clone());
        }
        for (prefix, floor) in floors {
            let held = self.name_floors.entry(prefix.clone()).or_insert(0);
            *held = (*held).max(*floor);
        }
    }

    /// The next free name for a mark of `tool`, taken.
    pub(crate) fn take_name(&mut self, tool: T) -> String {
        let names = self.items.iter().filter_map(|mark| mark.name.as_deref());
        drawing_names::raise_floors(&mut self.name_floors, names);
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
    /// keeps a generated name and replaces a typed one with the next
    /// generated name, so a mark is never left without one.
    pub fn rename_at(&mut self, index: usize, name: &str) {
        let Some(mark) = self.items.get(index) else {
            return;
        };
        let (tool, held) = (mark.tool, mark.name.clone());
        let trimmed = name.trim();
        let name = match held {
            _ if !trimmed.is_empty() => trimmed.to_owned(),
            Some(held) if drawing_names::is_generated_for(&held, tool.id()) => held,
            _ => self.take_name(tool),
        };
        let before = self.snapshot();
        self.items[index].name = Some(name);
        self.record(before);
    }
}

#[cfg(test)]
#[path = "naming_tests.rs"]
mod tests;
