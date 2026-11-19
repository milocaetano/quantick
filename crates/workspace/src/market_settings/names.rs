//! Every mark the store keeps carries a name: the ones a file brought without
//! one are named as the store reads them, per market, in the file's order —
//! the same names on every load and every chart of the market, until a write
//! puts them in the file. See [`crate::drawing_names`].
use super::{MarketKey, MarketSettings};
use crate::drawing_names::{self, NameFloors};

impl MarketSettings {
    /// The highest generated number `market`'s marks use per prefix: the
    /// floor a chart's next mark of that market is named above.
    #[must_use]
    pub fn name_floors(&self, market: &MarketKey) -> NameFloors {
        let mut floors = NameFloors::new();
        let sets = self.drawings.iter().filter(|set| set.is_market(market));
        let marks = sets.flat_map(|set| &set.items);
        drawing_names::raise_floors(&mut floors, marks.filter_map(|mark| mark.name.as_deref()));
        floors
    }

    /// Name every unnamed mark, market by market, above the names its market
    /// already uses.
    pub fn name_unnamed_drawings(&mut self) {
        let markets: Vec<MarketKey> = self.drawings.iter().map(|set| set.key().market).collect();
        let mut floors: std::collections::BTreeMap<MarketKey, NameFloors> = markets
            .into_iter()
            .map(|market| {
                let floors = self.name_floors(&market);
                (market, floors)
            })
            .collect();
        for set in &mut self.drawings {
            let market = set.key().market;
            let Some(floors) = floors.get_mut(&market) else {
                continue;
            };
            for mark in set.items.iter_mut().filter(|mark| mark.name.is_none()) {
                mark.name = Some(drawing_names::next_name(floors, &mark.tool));
            }
        }
    }
}

#[cfg(test)]
#[path = "names_tests.rs"]
mod tests;
