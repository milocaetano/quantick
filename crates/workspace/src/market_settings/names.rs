//! Every mark the store keeps carries a name: the ones a file brought without
//! one are named as the store reads them, per market, in the file's order —
//! the same names on every load and every chart of the market, until a write
//! puts them in the file. See [`crate::drawing_names`].
use super::{MarketKey, MarketSettings};
use crate::drawing_names::{self, NameFloors};
use serde::{Deserialize, Serialize};

/// One market's name high-water, as the file keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MarketNameFloors {
    venue: String,
    asset: String,
    floors: NameFloors,
}

impl MarketSettings {
    /// The highest generated number `market`'s marks use per prefix: the
    /// floor a chart's next mark of that market is named above.
    #[must_use]
    pub fn name_floors(&self, market: &MarketKey) -> NameFloors {
        let mut floors = NameFloors::new();
        let sets = self.drawings.iter().filter(|set| set.is_market(market));
        let marks = sets.flat_map(|set| &set.items);
        drawing_names::raise_floors(&mut floors, marks.filter_map(|mark| mark.name.as_deref()));
        let kept = self.name_floors.iter().find(|kept| kept.is(market));
        for (prefix, floor) in kept.into_iter().flat_map(|kept| &kept.floors) {
            let held = floors.entry(prefix.clone()).or_insert(0);
            *held = (*held).max(*floor);
        }
        floors
    }

    /// Keep `floors` as `market`'s high-water, never lowering one.
    pub fn raise_name_floors(&mut self, market: &MarketKey, floors: &NameFloors) {
        if floors.is_empty() {
            return;
        }
        let index = match self.name_floors.iter().position(|kept| kept.is(market)) {
            Some(index) => index,
            None => {
                self.name_floors.push(MarketNameFloors {
                    venue: market.venue.clone(),
                    asset: market.asset.clone(),
                    floors: NameFloors::new(),
                });
                self.name_floors
                    .sort_by(|a, b| (&a.venue, &a.asset).cmp(&(&b.venue, &b.asset)));
                self.name_floors
                    .iter()
                    .position(|kept| kept.is(market))
                    .unwrap_or(0)
            }
        };
        let kept = &mut self.name_floors[index].floors;
        for (prefix, floor) in floors {
            let held = kept.entry(prefix.clone()).or_insert(0);
            *held = (*held).max(*floor);
        }
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

impl MarketNameFloors {
    fn is(&self, market: &MarketKey) -> bool {
        self.venue == market.venue && self.asset == market.asset
    }
}

#[cfg(test)]
#[path = "names_tests.rs"]
mod tests;
