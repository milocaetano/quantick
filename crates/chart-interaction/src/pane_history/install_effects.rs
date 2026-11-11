//! Installation policy for classified history prefix changes.
use super::{PrefixChange, PrefixInstallEffect};

impl PrefixChange {
    /// Align the pane before rebuilding indicators, without allocating a plan.
    pub fn install_effects(self) -> impl Iterator<Item = PrefixInstallEffect> {
        use PrefixInstallEffect::{
            RebuildIndicators, ShiftDrawings, ShiftIndicatorRows, ShiftViewport,
        };
        let effects = match self {
            Self::Unchanged => [None, None, None, None],
            Self::LeadOnly => [None, None, None, Some(RebuildIndicators)],
            Self::Replace { delta } => [
                Some(ShiftViewport(delta)),
                Some(ShiftDrawings(delta)),
                // Rows cannot shift backwards; shrinking waits for the rebuild.
                // Zero retains the executor's existing no-op alignment.
                usize::try_from(delta).ok().map(ShiftIndicatorRows),
                Some(RebuildIndicators),
            ],
        };
        effects.into_iter().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PrefixInstallEffect::*;

    #[test]
    fn prefix_install_effects_cover_signed_changes() {
        let cases: [(PrefixChange, &[PrefixInstallEffect]); 5] = [
            (PrefixChange::Unchanged, &[]),
            (PrefixChange::LeadOnly, &[RebuildIndicators]),
            (
                PrefixChange::Replace { delta: -3 },
                &[ShiftViewport(-3), ShiftDrawings(-3), RebuildIndicators],
            ),
            (
                PrefixChange::Replace { delta: 0 },
                &[
                    ShiftViewport(0),
                    ShiftDrawings(0),
                    ShiftIndicatorRows(0),
                    RebuildIndicators,
                ],
            ),
            (
                PrefixChange::Replace { delta: 3 },
                &[
                    ShiftViewport(3),
                    ShiftDrawings(3),
                    ShiftIndicatorRows(3),
                    RebuildIndicators,
                ],
            ),
        ];
        for (change, expected) in cases {
            assert!(
                change.install_effects().eq(expected.iter().copied()),
                "{change:?}"
            );
        }
    }

    /// A rebuild reads the state visible at that instant, not the final state.
    fn rebuild_snapshot(
        effects: impl IntoIterator<Item = PrefixInstallEffect>,
    ) -> Vec<(isize, isize, usize)> {
        let mut state = (12, 8, 10);
        let mut snapshots = Vec::new();
        for effect in effects {
            match effect {
                ShiftViewport(delta) => state.0 += delta,
                ShiftDrawings(delta) => state.1 += delta,
                ShiftIndicatorRows(added) => state.2 += added,
                RebuildIndicators => snapshots.push(state),
            }
        }
        snapshots
    }

    #[test]
    fn prefix_install_rebuild_observes_aligned_state() {
        assert!(rebuild_snapshot(PrefixChange::Unchanged.install_effects()).is_empty());
        assert_eq!(
            rebuild_snapshot(PrefixChange::LeadOnly.install_effects()),
            [(12, 8, 10)]
        );
        for (delta, expected) in [(-3, (9, 5, 10)), (0, (12, 8, 10)), (3, (15, 11, 13))] {
            assert_eq!(
                rebuild_snapshot(PrefixChange::Replace { delta }.install_effects()),
                [expected]
            );
        }
        let reordered = [
            RebuildIndicators,
            ShiftViewport(3),
            ShiftDrawings(3),
            ShiftIndicatorRows(3),
        ];
        assert_ne!(
            rebuild_snapshot(reordered),
            [(15, 11, 13)],
            "the oracle must reject a rebuild before alignment"
        );
    }
}
