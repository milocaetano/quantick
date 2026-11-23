//! A union of execution sources; overlapping shortcuts never duplicate trades.
use super::SourceFilter;
use quantick_sim::history::SessionSource;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceSelection(u8);

impl From<SourceFilter> for SourceSelection {
    fn from(source: SourceFilter) -> Self {
        Self(match source {
            SourceFilter::Broker => 1,
            SourceFilter::Real => 2,
            SourceFilter::Replay => 4,
            SourceFilter::Simulator => 6,
            SourceFilter::All => 7,
        })
    }
}

impl SourceSelection {
    pub fn contains(self, source: SourceFilter) -> bool {
        let mask = Self::from(source).0;
        self.0 & mask == mask
    }

    pub fn union(self, source: SourceFilter) -> Self {
        Self(self.0 | Self::from(source).0)
    }

    pub fn toggled(self, source: SourceFilter) -> Self {
        if self.contains(source) {
            Self(self.0 & !Self::from(source).0)
        } else {
            self.union(source)
        }
    }

    pub fn admits(self, source: Option<SessionSource>) -> bool {
        [
            SourceFilter::Broker,
            SourceFilter::Real,
            SourceFilter::Replay,
        ]
        .into_iter()
        .any(|filter| self.contains(filter) && filter.admits(source))
    }

    pub fn label(self) -> &'static str {
        match self.0 {
            0 => "No sources selected",
            1 => "Broker",
            2 => "Sim live",
            3 => "Broker + Sim live",
            4 => "Replay",
            5 => "Broker + Replay",
            6 => "Simulator",
            _ => "All sources",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_union_and_toggle_admits_exactly_its_sources_including_legacy() {
        use quantick_trading::TradeMode;
        let atoms = [
            SourceFilter::Broker,
            SourceFilter::Real,
            SourceFilter::Replay,
        ];
        for mask in 0..8 {
            let selected = atoms
                .into_iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .fold(SourceSelection::default(), |s, (_, source)| s.union(source));
            for (source, expected) in [
                (Some(SessionSource::Broker(TradeMode::Demo)), mask & 1 != 0),
                (Some(SessionSource::Broker(TradeMode::Real)), mask & 1 != 0),
                (Some(SessionSource::Live), mask & 2 != 0),
                (None, mask & 2 != 0),
                (Some(SessionSource::Replay), mask & 4 != 0),
            ] {
                assert_eq!(selected.admits(source), expected, "mask {mask}, {source:?}");
            }
            for source in SourceFilter::PILLS {
                assert_eq!(selected.union(source).union(source), selected.union(source));
                assert_eq!(
                    selected.toggled(source).contains(source),
                    !selected.contains(source)
                );
            }
        }
        let simulator = SourceSelection::from(SourceFilter::Simulator);
        assert!(simulator.contains(SourceFilter::Real));
        assert!(simulator.contains(SourceFilter::Replay));
        assert!(!simulator.contains(SourceFilter::Broker));
        assert_eq!(simulator.union(SourceFilter::Real), simulator);
    }
}
