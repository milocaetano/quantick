use crate::{IndicatorEvent, LaneSample, SessionEffects, SlotId, session::SlotMirror};
use quantick_indicators::{IndicatorHost, Rgba8};
use std::collections::BTreeMap;

/// Emit exactly the difference between what the UI has and what the host now
/// holds: full snapshots after a rebuild, appended rows otherwise, error
/// transitions once, and the current preview frame for every healthy slot.
pub(crate) fn publish_deltas(
    host: &IndicatorHost,
    slots: &mut BTreeMap<SlotId, SlotMirror>,
    effects: &mut impl SessionEffects,
    rebuilt: bool,
    lane: &mut BTreeMap<SlotId, Vec<LaneSample>>,
) {
    for (&slot, mirror) in slots.iter_mut() {
        let Some(host_id) = mirror.host_id else {
            continue;
        };
        let Some(plots) = host.plots(host_id) else {
            continue;
        };
        let descriptor = host
            .descriptor(host_id)
            .expect("instance with plots has a descriptor");
        let rows = plots.len();

        if rebuilt || !mirror.synced {
            let columns: Vec<Vec<f64>> = descriptor
                .plots
                .iter()
                .map(|spec| plots.column(spec.id).to_vec())
                .collect();
            // Left empty for the indicators that never paint, which is all of
            // them until a script calls `barcolor`: a rebuild of a 100k-bar
            // chart must not carry a column of `None`s per indicator.
            let bar_paint: Vec<Option<Rgba8>> = if plots.paints_any() {
                (0..rows).map(|row| plots.bar_paint(row)).collect()
            } else {
                Vec::new()
            };
            effects.event(IndicatorEvent::Rebuilt {
                slot,
                descriptor: descriptor.clone(),
                columns,
                bar_paint,
                rows,
                stale: mirror.stale.clone(),
                inputs: mirror.values.clone(),
            });
            mirror.known_rows = rows;
            mirror.synced = true;
            mirror.error_reported = false;
        } else {
            for row_index in mirror.known_rows..rows {
                let row: Vec<f64> = descriptor
                    .plots
                    .iter()
                    .map(|spec| plots.value(spec.id, row_index))
                    .collect();
                effects.event(IndicatorEvent::Appended {
                    slot,
                    row,
                    paint: plots.bar_paint(row_index),
                });
            }
            mirror.known_rows = rows;
        }

        match host.error(host_id) {
            Some(error) if !mirror.error_reported => {
                effects.event(IndicatorEvent::Error {
                    slot,
                    error: error.clone(),
                });
                mirror.error_reported = true;
            }
            Some(_) => {}
            None => mirror.error_reported = false,
        }

        effects.event(IndicatorEvent::Preview {
            slot,
            frame: host.preview(host_id).cloned(),
        });

        // Sent on the same cadence as the preview, empty vector included: the
        // lane's curve is as transient as the forming bar it describes, and a
        // slot that has stopped producing rungs must stop drawing them.
        // Taken, not cloned: each slot is visited once, and the rungs are
        // already a fresh allocation from this batch's walk.
        effects.event(IndicatorEvent::Lane {
            slot,
            samples: lane.remove(&slot).unwrap_or_default(),
        });

        if let Some(revision) = host.objects_revision(host_id)
            && revision != mirror.objects_revision
        {
            effects.event(IndicatorEvent::Objects {
                slot,
                objects: host.objects_snapshot(host_id).unwrap_or_default(),
            });
            mirror.objects_revision = revision;
        }
    }
}
