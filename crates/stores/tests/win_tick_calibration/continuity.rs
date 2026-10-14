//! Complete recorded identities across source packets and camera changes.
use super::*;
use quantick_orderflow::projection::flow_tape::{FlowOpeningSelection, FlowTapeSource};

type Identity = (Vec<usize>, Decimal, Decimal, Decimal, Decimal, u32);
fn identities(frame: &FlowTapeFrame) -> Vec<Identity> {
    let mut groups: Vec<_> = frame
        .dots
        .iter()
        .map(|dot| {
            let mut members: Vec<_> = dot.members.iter().map(|member| member.ordinal).collect();
            members.sort_unstable();
            (
                members,
                dot.mark.quantity,
                dot.mark.buy_quantity,
                dot.mark.price,
                dot.candle_position,
                dot.radius.to_bits(),
            )
        })
        .collect();
    groups.sort();
    groups
}

#[test]
#[ignore = "requires the private recording used by the detail screenshots"]
fn recorded_detail_prefixes_keep_the_same_inspected_groups() {
    let path = std::env::var_os("WIN_TICK_FIXTURE").expect("private canonical recording");
    let all = parse_trades(&std::fs::read_to_string(path).unwrap()).unwrap();
    for (sample, prefix, left, right) in [
        (&all[..40_000], 20_000, 0, 8),
        (&all[all.len() - 200_000..], 72_000, 15, 26),
        (&all[all.len() - 200_000..], 148_000, 60, 67),
    ] {
        let select = |frame: &FlowTapeFrame| {
            identities(frame)
                .into_iter()
                .filter(|group| group.4 >= left.into() && group.4 <= right.into())
                .collect::<Vec<_>>()
        };
        let overview = select(&project(sample, 840.0, 100));
        let detail = select(&project(&sample[..prefix], 323.7, 100));
        assert!(!overview.is_empty());
        assert_eq!(
            detail, overview,
            "prefix={prefix} inspected={left}..{right}"
        );
        println!(
            "WIN_DETAIL_IDENTITY records={} prefix={prefix} slots={left}..{right} groups={} members={} exact_members_sides_price_anchor_radius=true",
            sample.len(),
            overview.len(),
            overview.iter().map(|group| group.0.len()).sum::<usize>()
        );
    }
}

#[test]
#[ignore = "requires the private canonical WIN recording"]
fn recorded_cold_packets_restart_and_camera_keep_every_group_identity() {
    let path = std::env::var_os("WIN_TICK_FIXTURE").expect("private canonical recording");
    let all = parse_trades(&std::fs::read_to_string(path).unwrap()).unwrap();
    for sample in [&all[..40_000], &all[all.len() - 200_000..]] {
        let cold = project(sample, 840.0, 100);
        let expected = identities(&cold);
        for packet in [127, 2_048, 16_384] {
            if sample.len() > 40_000 && packet == 127 {
                continue;
            }
            for partial in [false, true] {
                let mut source = FlowTapeSource::default();
                let mut publish_at = packet;
                for start in (0..sample.len()).step_by(packet) {
                    let end = (start + packet).min(sample.len());
                    source.append((start..end).map(|ordinal| FlowExecution {
                        ordinal,
                        slot: ordinal / 2_000,
                        accepted_ordinal: ordinal % 2_000,
                        ticks_per_bar: 2_000.into(),
                        trade: &sample[ordinal],
                        opening: false,
                    }));
                    // A cold worker publishes useful milestones, not every tiny packet.
                    if partial && end < publish_at && end < sample.len() {
                        continue;
                    }
                    publish_at = end.saturating_mul(2);
                    let mut view = cold.view;
                    if end < sample.len() {
                        view.tape_bursts.as_mut().unwrap().settled_through_ms =
                            sample[end - 1].timestamp_ms.div_euclid(100) * 100;
                    }
                    let requested_end = if partial { sample.len() } else { end };
                    let frame = source.project(
                        1,
                        0,
                        requested_end,
                        0..requested_end,
                        view,
                        FlowOpeningSelection::default(),
                    );
                    if !partial {
                        reconcile(&frame, &sample[..end]);
                    }
                }
                for (width, left, right) in [
                    (840.0, 0, cold.view.end_slot),
                    (323.7, 3, 12),
                    (840.0, 0, cold.view.end_slot),
                ] {
                    let mut view = cold.view;
                    view.width_px = width;
                    view.clip_left = left.into();
                    view.clip_right = right.into();
                    let frame = source.project(
                        1,
                        1,
                        sample.len(),
                        0..sample.len(),
                        view,
                        FlowOpeningSelection::default(),
                    );
                    let got = identities(&frame);
                    let want: Vec<_> = expected
                        .iter()
                        .filter(|identity| {
                            identity.4 >= view.clip_left && identity.4 <= view.clip_right
                        })
                        .cloned()
                        .collect();
                    assert_eq!(got, want, "packet={packet} partial={partial} width={width}");
                }
                println!(
                    "WIN_CONTINUITY records={} groups={} packet={packet} partial={partial} exact_members_sides_price_anchor_radius=true camera_restore=true",
                    sample.len(),
                    expected.len()
                );
            }
        }
        assert_eq!(
            identities(&project(sample, 840.0, 100)),
            expected,
            "cold restart"
        );
    }
}
