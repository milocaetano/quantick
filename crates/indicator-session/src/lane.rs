use crate::{IndicatorSession, LaneSample, SlotId, session::SlotMirror};
use quantick_engine::forming_run::FormingRun;
use quantick_indicators::IndicatorHost;
use std::collections::BTreeMap;
/// Maximum full indicator evaluations for one forming-bar ladder.
pub const MAX_LANE_RUNGS: usize = 64;
/// Retained forming storage and cumulative fold work for the session.
/// Each cut adds the retired run's count once, without traversing trades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneDiagnostics {
    pub len: usize,
    pub capacity: usize,
    pub folds: u64,
}
impl IndicatorSession {
    pub fn lane_diagnostics(&self) -> LaneDiagnostics {
        LaneDiagnostics {
            len: self.lane_run.len(),
            capacity: self.lane_run.capacity(),
            folds: self.retired_folds + self.lane_run.folds(),
        }
    }
    pub(crate) fn cut_lane(&mut self) {
        self.retired_folds += self.lane_run.folds();
        self.lane_run = FormingRun::default();
    }
}
/// Sample every slot's plots across the forming bar's run, oldest rung first.
///
/// Returns an empty map when there is no lane, no run, or nothing forming —
/// the three ways a chart says "no curve on the tape", all of which must
/// clear whatever the lane was drawing rather than leave it frozen.
pub(crate) fn walk_lane(
    host: &mut IndicatorHost,
    slots: &BTreeMap<SlotId, SlotMirror>,
    run: &FormingRun,
    rungs: usize,
) -> BTreeMap<SlotId, Vec<LaneSample>> {
    let mut lane: BTreeMap<SlotId, Vec<LaneSample>> = BTreeMap::new();
    let prefixes = run.prefixes(rungs.min(MAX_LANE_RUNGS));
    if prefixes.is_empty() {
        return lane;
    }
    for (&slot, mirror) in slots {
        if mirror.host_id.is_some() {
            lane.insert(slot, Vec::with_capacity(prefixes.len()));
        }
    }
    host.walk_partial_prefixes(&prefixes, |prefix, previews| {
        for (&slot, mirror) in slots {
            let Some(host_id) = mirror.host_id else {
                continue;
            };
            // A slot in its error state previews nothing, and nothing is
            // exactly what the lane should show for it.
            let Some(frame) = previews.get(host_id) else {
                continue;
            };
            if let Some(samples) = lane.get_mut(&slot) {
                samples.push(LaneSample {
                    close_time: prefix.close_time,
                    values: frame.values.clone(),
                });
            }
        }
    });
    lane
}
