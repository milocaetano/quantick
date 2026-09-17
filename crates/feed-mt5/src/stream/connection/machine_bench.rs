//! Manual paired, predecoded control; setup/printing are outside allocation and timing windows.
use super::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    time::Instant,
};

thread_local! {
    static MEASURING: Cell<bool> = const {Cell::new(false)};
    static ALLOCATIONS: Cell<u64> = const {Cell::new(0)};
    static DEALLOCATIONS: Cell<u64> = const {Cell::new(0)};
}
struct CountingAllocator;
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
fn allocated() {
    if MEASURING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
    }
}
// SAFETY: all allocation operations are forwarded unchanged to System. The
// thread-local counters have const initialization and allocate no storage.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocated();
        // SAFETY: caller supplies the valid allocator layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocated();
        // SAFETY: caller supplies the valid allocator layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if MEASURING.try_with(Cell::get).unwrap_or(false) {
            let _ = DEALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        }
        // SAFETY: pointer and layout came from the forwarded System allocation.
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocated();
        // SAFETY: preserve the allocator caller's pointer/layout/size contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[derive(Default, Debug, PartialEq, Eq)]
struct Counts {
    live: u64,
    latency: u64,
    ids: u64,
    buys: u64,
    samples: u64,
}
fn count_trade(counts: &mut Counts, trade: quantick_engine::Trade) {
    counts.live += 1;
    counts.ids += trade.agg_id;
    counts.buys += u64::from(trade.side == Side::Buy);
    std::hint::black_box(trade);
}
fn measure(run: impl FnOnce() -> Counts) -> (Counts, u128, u64, u64) {
    ALLOCATIONS.set(0);
    DEALLOCATIONS.set(0);
    MEASURING.set(true);
    let started = Instant::now();
    let counts = run();
    let elapsed = started.elapsed().as_nanos();
    MEASURING.set(false);
    (counts, elapsed, ALLOCATIONS.get(), DEALLOCATIONS.get())
}
fn machine_run(machine: &mut SessionMachine<'_>, ticks: Vec<protocol::Tick>) -> Counts {
    let mut counts = Counts::default();
    for input in ticks
        .into_iter()
        .map(|tick| Input::Message(BridgeMsg::Tick(tick)))
        .chain([Input::Message(BridgeMsg::Heartbeat(protocol::Heartbeat {
            seq_last: 200000,
            time_ms: 210000,
            ticks_sent: 200000,
            server_utc_offset_s: None,
        }))])
    {
        let mut effect = machine.input(input).unwrap();
        loop {
            let ack = match effect {
                Effect::Wait(_) => break,
                Effect::Live(trade) => {
                    count_trade(&mut counts, trade);
                    Ack::Published(true)
                }
                Effect::Publish => {
                    assert!(matches!(
                        machine.take_publication().unwrap(),
                        Mt5Event::Latency(_)
                    ));
                    counts.latency += 1;
                    Ack::Published(true)
                }
                Effect::SampleTime => {
                    counts.samples += 1;
                    Ack::Time(210000)
                }
                Effect::ReadCapture => Ack::Capture {
                    enabled: false,
                    base_generation: 0,
                },
                Effect::Diagnostic => {
                    let _ = machine.take_diagnostic().unwrap();
                    Ack::Diagnostic
                }
                _ => panic!("unexpected pure benchmark effect"),
            };
            effect = machine.resume(ack).unwrap();
        }
    }
    counts
}
fn control_run(ticks: Vec<protocol::Tick>) -> Counts {
    let mut mapper = crate::map::TickMapper::new(SideMode::TickRule, 2);
    let mut tracker = crate::session::SeqTracker::new();
    let mut deals = crate::deals::DealSampler::new(2);
    let mut latency = crate::latency::LatencyTracker::new();
    let mut counts = Counts::default();
    for tick in ticks {
        assert!(tracker.observe(tick.seq).is_none());
        assert!(deals.observe(&tick).is_none());
        if let crate::map::MapOutcome::Trade { trade, .. } = mapper.map(&tick) {
            latency.observe_live(tick.time_ms, tick.sent_ms);
            if latency.due() {
                counts.samples += 1;
                counts.latency += u64::from(
                    latency
                        .sample(210000, mapper.server_utc_offset_ms())
                        .is_some(),
                );
            }
            count_trade(&mut counts, trade);
        }
    }
    assert!(deals.finish().is_none());
    counts.samples += 1;
    counts.latency += u64::from(
        latency
            .sample(210000, mapper.server_utc_offset_ms())
            .is_some(),
    );
    counts
}

#[test]
#[ignore = "manual quiet-host predecoded control; run with --ignored --nocapture --test-threads=1"]
fn benchmark_synchronous_session_control() {
    let inputs: Vec<_> = (1..=200000)
        .map(|seq| {
            let Input::Message(BridgeMsg::Tick(tick)) =
                tick(seq, 10000 + seq as i64, 100 + seq % 2, None)
            else {
                unreachable!()
            };
            tick
        })
        .collect();
    let expected = Counts {
        live: 199999,
        latency: 3125,
        ids: 20000099999,
        buys: 99999,
        samples: 3125,
    };
    for round in 0..18 {
        for side in if round % 2 == 0 {
            ["control", "machine"]
        } else {
            ["machine", "control"]
        } {
            let ticks = inputs.clone(); // fixture ownership prepared before either measurement
            let (mut machine, _) = admitted();
            let (counts, elapsed_ns, allocations, deallocations) = measure(|| {
                if side == "machine" {
                    machine_run(&mut machine, ticks)
                } else {
                    control_run(ticks)
                }
            });
            assert_eq!(counts, expected);
            assert_eq!(allocations, 0, "predecoded processing must not allocate");
            println!(
                "{{\"side\":\"{side}\",\"round\":{round},\"warmup\":{},\"ticks\":200000,\"elapsed_ns\":{elapsed_ns},\"allocations\":{allocations},\"deallocations\":{deallocations},\"live\":{},\"latency\":{},\"sample_requests\":{},\"id_sum\":{},\"buys\":{}}}",
                round < 3,
                counts.live,
                counts.latency,
                counts.samples,
                counts.ids,
                counts.buys
            );
        }
    }
}
