use super::*;
use crate::{HeatmapConfig, config::theme::OrderflowRenderStyle};
use quantick_engine::Side;
use std::cell::RefCell;
#[derive(Default)]
struct Runner {
    request: Option<FlowRequest>,
    queue: RefCell<Vec<FlowChunk>>,
    output: RefCell<Option<Arc<FlowTapeFrame>>>,
    notifications: usize,
    cache: FlowWorkerCache,
}
impl FlowRunner for Runner {
    fn request(&mut self, request: &FlowRequest, notify: bool) -> bool {
        self.request = Some(request.clone());
        self.notifications += usize::from(notify);
        false
    }
    fn submit(&self, chunk: FlowChunk) -> Result<(), Box<FlowChunk>> {
        let mut queue = self.queue.borrow_mut();
        if queue.len() == 2 {
            Err(Box::new(chunk))
        } else {
            queue.push(chunk);
            Ok(())
        }
    }
    fn finished(&self) -> Option<Arc<FlowTapeFrame>> {
        self.output.borrow_mut().take()
    }
}
impl Runner {
    fn pump(&mut self) {
        let request = self.request.as_ref().unwrap();
        self.cache.select_request(request);
        for chunk in self.queue.get_mut().drain(..) {
            self.cache.append(&chunk);
        }
        *self.output.get_mut() = Some(self.cache.project(request));
    }
}
fn request(epoch: u64, slots: Range<usize>) -> FlowRequest {
    let ordinals = slots.start * 2..slots.end * 2;
    FlowRequest {
        epoch,
        layout_revision: 0,
        source_count: ordinals.end,
        requested: ordinals.clone(),
        keep: FlowKeep {
            slots: slots.clone(),
            ordinals,
        },
        opening_windows: vec![],
        view: FlowTapeView {
            first_slot: slots.start,
            end_slot: slots.end,
            clip_left: slots.start.into(),
            clip_right: slots.end.into(),
            width_px: 200.0,
            height_px: 100.0,
            prices: PriceWindow::new(90.into(), 110.into()).unwrap(),
            reference: FlowReference::VisibleRegions,
            radius_limit: 7.0,
            merge_support_radius: 12.0,
            exclude_opening: false,
        },
    }
}
fn chunk(epoch: u64, range: Range<usize>) -> FlowChunk {
    FlowChunk {
        epoch,
        ticks_per_bar: 2.into(),
        executions: range
            .map(|ordinal| OwnedFlowExecution {
                ordinal,
                slot: ordinal / 2,
                accepted_ordinal: ordinal % 2,
                trade: Trade {
                    agg_id: 1,
                    timestamp_ms: 1000,
                    price: 100.into(),
                    quantity: Decimal::ONE,
                    side: Side::Buy,
                },
            })
            .collect(),
    }
}
fn project(session: &mut FlowSession<Runner>, request: FlowRequest) {
    let epoch = request.epoch;
    session.project(request, |range| chunk(epoch, range));
}
#[test]
fn queue_full_keeps_one_bounded_packet_and_copies_only_new_ordinals() {
    let mut session = FlowSession::<Runner>::default();
    let request = request(1, 0..5000);
    let mut copied = vec![];
    for _ in 0..8 {
        session.project(request.clone(), |range| {
            copied.push(range.clone());
            chunk(1, range)
        });
    }
    assert_eq!(copied, [0..2048, 2048..4096, 4096..6144]);
    assert_eq!(session.submitted.count(0..10000), 4096);
    assert_eq!(session.unsent.as_ref().unwrap().executions.len(), 2048);
    while session.progress().pending {
        session.runner.pump();
        project(&mut session, request.clone());
    }
    assert_eq!(
        session
            .frame()
            .unwrap()
            .dots
            .iter()
            .map(|dot| dot.mark.trade_count)
            .sum::<usize>(),
        10000
    );
    assert_eq!(session.runner.cache.loaded(), 10000);
}
#[test]
fn metadata_only_append_does_not_wake_or_recapture_pinned_history() {
    let mut session = FlowSession::<Runner>::default();
    let mut request = request(1, 0..2);
    project(&mut session, request.clone());
    session.runner.pump();
    project(&mut session, request.clone());
    let notifications = session.runner.notifications;
    request.source_count = 10000;
    session.project(request, |_| panic!("offscreen append copied source"));
    assert_eq!(session.runner.notifications, notifications);
    assert!(!session.progress().pending);
}
#[test]
fn queued_navigation_a_b_a_refills_evicted_facts_without_duplicates() {
    let mut session = FlowSession::<Runner>::default();
    project(&mut session, request(1, 0..2));
    project(&mut session, request(1, 10..12));
    session.runner.pump();
    assert_eq!(session.runner.cache.loaded(), 4);
    project(&mut session, request(1, 0..2));
    session.runner.pump();
    project(&mut session, request(1, 0..2));
    assert!(!session.progress().pending);
    assert_eq!(
        session
            .frame()
            .unwrap()
            .dots
            .iter()
            .map(|dot| dot.mark.trade_count)
            .sum::<usize>(),
        4
    );
    assert!(
        session
            .frame()
            .unwrap()
            .dots
            .iter()
            .flat_map(|dot| dot.members.iter())
            .all(|member| member.ordinal < 4)
    );
}
#[test]
fn sliding_live_windows_release_capacity_and_reload_old_history() {
    let mut session = FlowSession::<Runner>::default();
    for start in (0..200).step_by(2) {
        let request = request(1, start..start + 2);
        project(&mut session, request.clone());
        session.runner.pump();
        project(&mut session, request);
        assert_eq!(session.runner.cache.loaded(), 4);
        assert!(!session.progress().pending);
    }
    project(&mut session, request(1, 0..2));
    session.runner.pump();
    project(&mut session, request(1, 0..2));
    assert_eq!(
        session
            .frame()
            .unwrap()
            .dots
            .iter()
            .map(|dot| dot.mark.trade_count)
            .sum::<usize>(),
        4
    );
    assert!(!session.progress().pending);
}
#[test]
fn changing_epoch_rejects_old_queued_source_and_old_completed_frame() {
    let mut session = FlowSession::<Runner>::default();
    project(&mut session, request(1, 0..2));
    session.runner.pump();
    project(&mut session, request(1, 2..4));
    project(&mut session, request(2, 10..12));
    assert!(session.frame().is_none());
    session.runner.pump();
    project(&mut session, request(2, 10..12));
    let frame = session.frame().unwrap();
    assert_eq!(frame.source_revision, 2);
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|dot| dot.mark.trade_count)
            .sum::<usize>(),
        4
    );
    assert!(!session.progress().pending);
}
fn config() -> HeatmapConfig {
    let mut c = HeatmapConfig::default();
    c.live_lane.native_tape = true;
    c.live_lane.tape_only = false;
    c.volume_dots.enabled = true;
    c.show_aggressions = true;
    c
}
#[test]
fn hidden_tape_keeps_tick_flow_and_suppresses_only_legacy_history() {
    let mut owner = FlowSession::<Runner>::default();
    let mut config = config();
    assert!(!owner.replaces_history(&config));
    project(&mut owner, request(1, 0..1));
    for enabled in [true, false, true] {
        config.live_lane.enabled = enabled;
        assert!(FlowSession::<Runner>::active(&config));
        assert!(owner.replaces_history(&config));
        let mut style = OrderflowRenderStyle::from_config(&config, [20, 24, 30, 255]);
        let lane = style.lane_aggression_layer;
        style.aggression_layer &= !owner.replaces_history(&config);
        assert!(!style.aggression_layer);
        assert_eq!(style.lane_aggression_layer, lane);
    }
    config.show_aggressions = false;
    assert!(!FlowSession::<Runner>::active(&config));
    config.show_aggressions = true;
    config.volume_dots.enabled = false;
    assert!(!FlowSession::<Runner>::active(&config));
    config.volume_dots.enabled = true;
    config.live_lane.native_tape = false;
    assert!(!FlowSession::<Runner>::active(&config));
    config.live_lane.tape_only = true;
    config.live_lane.enabled = true;
    assert!(!FlowSession::<Runner>::active(&config));
    config.live_lane.enabled = false;
    assert!(FlowSession::<Runner>::active(&config));
}

#[test]
fn explicit_empty_reset_clears_frame_progress_and_transport_but_keeps_preference() {
    let mut session = FlowSession::<Runner>::default();
    session.set_ignore_opening(true);
    project(&mut session, request(1, 0..2));
    session.runner.pump();
    project(&mut session, request(1, 0..2));
    assert!(session.frame().is_some());
    session.clear();
    assert!(session.frame().is_none());
    assert_eq!(session.progress().requested_executions, 0);
    assert!(!session.progress().pending);
    assert!(session.ignore_opening());
    assert!(session.runner.request.is_none());
    project(&mut session, request(2, 10..12));
    session.runner.pump();
    project(&mut session, request(2, 10..12));
    assert_eq!(session.frame().unwrap().source_revision, 2);
    assert!(!session.progress().pending);
}

fn cold_request(start: usize, end: usize) -> FlowRequest {
    let mut request = request(1, start / 2000..end.div_ceil(2000));
    request.source_count = end;
    request.requested = start..end;
    request.keep.ordinals = start..end;
    request.view.width_px = 1.0;
    request
}
fn cold_chunk(epoch: u64, range: Range<usize>) -> FlowChunk {
    let mut chunk = chunk(epoch, range);
    chunk.ticks_per_bar = 2000.into();
    for item in &mut chunk.executions {
        item.slot = item.ordinal / 2000;
        item.accepted_ordinal = item.ordinal % 2000;
    }
    chunk
}
#[test]
fn publication_large_cold_fill_projects_geometric_partials_and_exact_completion() {
    let mut cache = FlowWorkerCache::default();
    let mut request = cold_request(0, 1_048_576);
    let mut published = Vec::new();
    for start in (0..request.requested.end).step_by(2048) {
        // Live view revisions must not restart the cold-fill publication milestone.
        request.layout_revision += 1;
        cache.select_request(&request);
        cache.append(&cold_chunk(1, start..start + 2048));
        if let Some(frame) = cache.project_if_due(&request, false) {
            assert_eq!(
                frame
                    .dots
                    .iter()
                    .map(|dot| dot.mark.trade_count)
                    .sum::<usize>(),
                start + 2048
            );
            published.push(frame.loaded_executions);
        }
    }
    assert!(published.len() <= 11, "full projections: {published:?}");
    assert_eq!(published.first(), Some(&2048));
    assert_eq!(published.last(), Some(&1_048_576));
    assert!(published.windows(2).all(|pair| pair[1] >= pair[0] * 2));
    assert!(cache.project_if_due(&request, true).is_none());
}
#[test]
fn publication_small_live_appends_and_cached_layout_changes_complete_immediately() {
    let mut cache = FlowWorkerCache::default();
    for end in 1..=20 {
        let request = cold_request(0, end);
        cache.select_request(&request);
        cache.append(&cold_chunk(1, end - 1..end));
        let frame = cache.project_if_due(&request, false).unwrap();
        assert_eq!(frame.loaded_executions, end);
        assert_eq!(frame.omitted_executions, 0);
    }
    let mut zoom = cold_request(0, 20);
    zoom.layout_revision = 1;
    zoom.view.width_px = 100.0;
    cache.select_request(&zoom);
    assert_eq!(
        cache.project_if_due(&zoom, false).unwrap().layout_revision,
        1
    );
    assert!(cache.project_if_due(&zoom, true).is_none());
}
#[test]
fn publication_quiet_partial_flushes_once_and_empty_request_publishes() {
    let mut cache = FlowWorkerCache::default();
    let request = cold_request(0, 20_000);
    cache.select_request(&request);
    cache.append(&cold_chunk(1, 0..2048));
    assert!(cache.project_if_due(&request, false).is_some());
    cache.append(&cold_chunk(1, 2048..3000));
    assert!(cache.project_if_due(&request, false).is_none());
    let partial = cache.project_if_due(&request, true).unwrap();
    assert_eq!(partial.loaded_executions, 3000);
    assert_eq!(partial.omitted_executions, 17_000);
    assert!(cache.project_if_due(&request, true).is_none());
    let empty = cold_request(0, 0);
    cache.select_request(&empty);
    assert!(cache.project_if_due(&empty, false).unwrap().dots.is_empty());
    assert!(cache.project_if_due(&empty, true).is_none());
}
#[test]
fn publication_rebases_after_eviction_and_a_b_a_or_epoch_changes() {
    let mut cache = FlowWorkerCache::default();
    let a = cold_request(0, 40_000);
    cache.select_request(&a);
    cache.append(&cold_chunk(1, 0..16_000));
    assert_eq!(
        cache.project_if_due(&a, false).unwrap().loaded_executions,
        16_000
    );
    // An overlapping request evicts most of the published facts: do not wait for32k.
    let shifted = cold_request(12_000, 52_000);
    cache.select_request(&shifted);
    cache.append(&cold_chunk(1, 16_000..18_000));
    assert_eq!(
        cache
            .project_if_due(&shifted, false)
            .unwrap()
            .loaded_executions,
        6000
    );
    for (epoch, start) in [(1, 80_000), (1, 0), (2, 0)] {
        let mut next = cold_request(start, start + 40_000);
        next.epoch = epoch;
        cache.select_request(&next);
        cache.append(&cold_chunk(
            next.epoch,
            next.requested.start..next.requested.start + 2048,
        ));
        let frame = cache.project_if_due(&next, false).unwrap();
        assert_eq!(frame.loaded_executions, 2048);
        assert_eq!(frame.source_revision, next.epoch);
        assert!(
            frame
                .dots
                .iter()
                .flat_map(|dot| dot.members.iter())
                .all(|member| { next.requested.contains(&member.ordinal) })
        );
    }
}

#[test]
fn publication_terminal_cache_limit_flushes_below_the_next_milestone_once() {
    let request = cold_request(0, MAX_FLOW_EXECUTIONS + 2048);
    let mut publication = FlowPublication::default();
    publication.record(&request, 1_048_576, false);
    assert!(!publication.due(&request, MAX_FLOW_EXECUTIONS, false, false));
    assert!(publication.due(&request, MAX_FLOW_EXECUTIONS, true, false));
    publication.record(&request, MAX_FLOW_EXECUTIONS, true);
    assert!(!publication.dirty(&request, MAX_FLOW_EXECUTIONS, true));
    assert!(!publication.due(&request, MAX_FLOW_EXECUTIONS, true, true));
}
