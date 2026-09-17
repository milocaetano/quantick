//! Lossless public-output evidence: all Debug fields plus IEEE bit patterns.
//! Float bits also distinguish negative zero and any NaN payload. Decimal
//! Debug is exact decimal text; vectors keep their original element order.
use std::fmt::Write as _;

use quantick_orderflow::{AggressionPrimitive, HeatmapProjection, LiveMarks, SettledProjection};

fn marks_bits(out: &mut String, marks: &[AggressionPrimitive]) {
    for mark in marks {
        writeln!(
            out,
            "mark-bits {:08x} {:08x} {:016x} {:016x} {:08x}",
            mark.buy_share.to_bits(),
            mark.matched_fraction.to_bits(),
            mark.x.to_bits(),
            mark.y.to_bits(),
            mark.size.to_bits()
        )
        .unwrap();
    }
}

pub fn exact(settled: &SettledProjection, live: &LiveMarks, joined: &HeatmapProjection) -> String {
    let mut out = format!("settled={settled:?}\nlive={live:?}\njoined={joined:?}\n");
    for cells in [&settled.cells, &joined.cells] {
        for cell in cells.iter() {
            writeln!(
                out,
                "cell-bits {:016x} {:016x} {:016x} {:016x} {:08x} {:08x}",
                cell.x0.to_bits(),
                cell.x1.to_bits(),
                cell.y0.to_bits(),
                cell.y1.to_bits(),
                cell.intensity.to_bits(),
                cell.alpha.to_bits()
            )
            .unwrap();
        }
    }
    for marks in [&settled.aggressions, &live.aggressions, &joined.aggressions] {
        marks_bits(&mut out, marks);
    }
    for events in [
        &settled.liquidity_events,
        &live.liquidity_events,
        &joined.liquidity_events,
    ] {
        for event in events {
            writeln!(
                out,
                "event-bits {:08x} {:08x} {:016x} {:016x} {:016x}",
                event.fraction.to_bits(),
                event.matched_fraction.to_bits(),
                event.x.to_bits(),
                event.y0.to_bits(),
                event.y1.to_bits()
            )
            .unwrap();
        }
    }
    for event in &settled.live_events {
        writeln!(
            out,
            "live-event-bits {:08x} {:08x}",
            event.fraction.to_bits(),
            event.matched_fraction.to_bits()
        )
        .unwrap();
    }
    for gaps in [&settled.gaps, &joined.gaps] {
        for gap in gaps.iter() {
            writeln!(
                out,
                "gap-bits {:016x} {:016x}",
                gap.x0.to_bits(),
                gap.x1.to_bits()
            )
            .unwrap();
        }
    }
    writeln!(
        out,
        "now-bits {:?} {:?}",
        live.live_now_x.map(f64::to_bits),
        joined.live_now_x.map(f64::to_bits)
    )
    .unwrap();
    out
}

// Compact regression sentinel only; the acceptance parity comparison uses
// complete artifact bytes, not this non-cryptographic digest.
pub fn sentinel(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
