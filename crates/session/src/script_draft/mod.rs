//! Compile, preview and prepare alarm-only runtime instances on actual closes.
mod contract;
use crate::script_strategies::ScriptReading;
use crate::strategies::TabStrategies;
pub use contract::{Diagnostic, DraftInput, HistoricalClose, Marker, PreparedAlarm};
use quantick_engine::Side;
use quantick_script_trigger::{ArmRefusal, Script, ScriptFault, ScriptInstance, WARMUP_BARS};

fn diagnostic(fault: ScriptFault) -> Diagnostic {
    Diagnostic {
        line: Some(fault.line),
        code: fault.code.into(),
        message: fault.message,
    }
}

fn instances<D: Copy>(
    input: &DraftInput<'_, D>,
) -> Result<Vec<ScriptInstance<D>>, Vec<Diagnostic>> {
    let script = Script::load(input.id, input.source).map_err(|error| {
        vec![match error {
            ArmRefusal::Compile(fault) => diagnostic(fault),
            other => Diagnostic {
                line: match other {
                    ArmRefusal::ComputedTitle { line } => Some(line),
                    _ => None,
                },
                code: "ALARM_TITLE".into(),
                message: other.to_string(),
            },
        }]
    })?;
    Ok([Side::Buy, Side::Sell]
        .into_iter()
        .filter(|side| script.sides().iter().any(|(_, declared)| declared == side))
        .map(|side| ScriptInstance::arm(0, &script, side, input.placement, &[]))
        .collect())
}

pub fn preview<D: Copy>(input: DraftInput<'_, D>) -> Result<Vec<Marker>, Vec<Diagnostic>> {
    let mut alarms = instances(&input)?;
    let mut markers = Vec::new();
    for sample in input.history {
        let close = &sample.close;
        let placement = input.placement.resolve(|_| sample.region);
        for alarm in &mut alarms {
            let time = close.bar.close_time;
            if alarm
                .on_closed_bar(
                    &close.bar,
                    close.poc,
                    placement.as_ref(),
                    u64::try_from(time).unwrap_or_default(),
                )
                .is_some()
            {
                markers.push(Marker {
                    slot: close.slot,
                    close_time: time,
                    side: alarm.side(),
                });
            }
            if let Some(fault) = alarm.fault() {
                return Err(vec![diagnostic(fault)]);
            }
        }
    }
    Ok(markers)
}

pub fn prepare<D: Copy>(input: DraftInput<'_, D>) -> Result<PreparedAlarm<D>, Vec<Diagnostic>> {
    let mut alarms = instances(&input)?;
    for alarm in &mut alarms {
        for sample in &input.history[input.history.len().saturating_sub(WARMUP_BARS)..] {
            alarm.warm(&sample.close.bar, sample.close.poc);
            if let Some(fault) = alarm.fault() {
                return Err(vec![diagnostic(fault)]);
            }
        }
    }
    Ok(PreparedAlarm(alarms))
}

pub fn commit<D: Copy + PartialEq>(
    tab: &mut TabStrategies<D>,
    pane_id: u64,
    alarms: PreparedAlarm<D>,
) -> Vec<ScriptReading<D>> {
    alarms
        .0
        .into_iter()
        .map(|instance| tab.publish_script(pane_id, instance))
        .collect()
}

/// Read a bounded suffix of the composed closed series, preserving its absolute slots.
pub fn read_history(
    series: &quantick_chart::pane_series::PaneSeriesRead<'_>,
    first_slot: usize,
    limit: usize,
    mut region_at: impl FnMut(usize) -> Option<(quantick_strategy::Region, bool)>,
) -> Vec<HistoricalClose> {
    use crate::strategies::LiveClose;
    let end = series.closed_slots();
    let seam = series.seam_slot();
    let begin = end.saturating_sub(limit).max(first_slot.min(end));
    (begin..end)
        .map(|slot| {
            let (bar, poc) = if slot < seam {
                (series.closed_bar(slot).expect("prefix slot").clone(), None)
            } else {
                let closed = series.footprint_bar(slot - seam);
                (closed.bar, closed.poc)
            };
            let region = region_at(slot);
            HistoricalClose {
                close: LiveClose { bar, poc, slot },
                region,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
