//! One synchronous protocol lifecycle. Effects never advance across an unacknowledged send.
#[cfg(test)]
#[path = "input_boundary_tests.rs"]
mod input_boundary_tests;

use super::super::{
    blocks::{DepthSession, RatesBlock},
    events::{ConnEnd, Mt5Event, Mt5Status},
};
use super::{diagnostic::Diagnostic, effect::*, hello, history::HistoryPage};
use crate::{
    deals::DealSampler,
    latency::LatencyTracker,
    map::{SideMode, TickMapper},
    protocol::{self, BridgeMsg},
    session::SeqTracker,
};
use quantick_engine::Trade;

pub(super) struct Connected {
    pub mapper: TickMapper,
    pub hello: protocol::Hello,
    pub tracker: SeqTracker,
    pub highest_tick_ms: Option<i64>,
    pub latency: LatencyTracker,
    pub deals: DealSampler,
    pub lag_reported: bool,
    pub backfill: Option<Vec<Trade>>,
    pub candles: Option<RatesBlock>,
    pub depth: DepthSession,
    pub history: HistoryPage,
    pub undecodable: u64,
}

pub(super) enum After {
    Wait,
    Connected(protocol::Hello),
    TickDeal {
        tick: protocol::Tick,
        tick_ms: i64,
        advances: bool,
    },
    TickMap(protocol::Tick),
    Sample(SampleTarget),
    Live(Trade),
    HeartbeatOffset(protocol::Heartbeat),
    HeartbeatCapture(protocol::Heartbeat),
    HeartbeatLog(protocol::Heartbeat),
    HeartbeatDone(protocol::Heartbeat),
    BookCapture(protocol::Book),
    Depth,
    HistoryStart {
        count_hint: Option<u64>,
    },
    HistoryEnd {
        page: Option<super::history::CompletedPage>,
        exhausted: bool,
        scanned_to_ms: Option<i64>,
    },
    Unsupported,
    Written,
    ClosePage,
    CloseRates,
    CloseDepth,
    Closed,
}
pub(super) enum SampleTarget {
    Live(Trade),
    Heartbeat(protocol::Heartbeat),
}

#[derive(Clone, Copy)]
pub(super) enum Expect {
    Publish,
    Write,
    Time,
    Capture,
    Pager,
}
impl Expect {
    fn matches(self, ack: &Ack) -> bool {
        matches!(
            (self, ack),
            (Self::Publish, Ack::Published(_))
                | (Self::Write, Ack::Written(_))
                | (Self::Time, Ack::Time(_))
                | (Self::Capture, Ack::Capture { .. })
                | (Self::Pager, Ack::Pager(_))
        )
    }
}

pub(super) struct SessionMachine<'a> {
    pub phase: Phase,
    pub generation_offset: u64,
    pub connected: Option<Connected>,
    pub symbol: &'a str,
    side_mode: SideMode,
    hello_timeout_s: u64,
    read_timeout_s: u64,
    end: Option<ConnEnd>,
    pending: Option<(Expect, After)>,
    // Diagnostics are bounded facts, never a per-message heap effect list.
    diagnostics: [Option<Diagnostic>; 4],
    diagnostic_count: u8,
    diagnostic_taken: bool,
    publication: Option<Mt5Event>,
    deferred: Option<Effect>,
}
impl<'a> SessionMachine<'a> {
    pub fn new(
        symbol: &'a str,
        side_mode: SideMode,
        generation_offset: u64,
        hello_timeout_s: u64,
        read_timeout_s: u64,
    ) -> Self {
        Self {
            phase: Phase::AwaitHello,
            generation_offset,
            connected: None,
            symbol,
            side_mode,
            hello_timeout_s,
            read_timeout_s,
            end: None,
            pending: None,
            diagnostics: std::array::from_fn(|_| None),
            diagnostic_count: 0,
            diagnostic_taken: false,
            publication: None,
            deferred: None,
        }
    }
    pub fn input(&mut self, input: Input) -> Result<Effect, InvalidInput> {
        if self.pending.is_some()
            || self.deferred.is_some()
            || matches!(self.phase, Phase::Closing | Phase::Closed)
        {
            return Err(InvalidInput);
        }
        let effect = match input {
            Input::Message(BridgeMsg::Tick(tick)) if self.phase == Phase::Connected => {
                self.tick(tick)
            }
            other => self.dispatch_other_input(other)?,
        };
        Ok(self.expose(effect))
    }
    #[inline(never)]
    fn dispatch_other_input(&mut self, input: Input) -> Result<Effect, InvalidInput> {
        let effect = match input {
            Input::Message(message) if self.phase == Phase::AwaitHello => self.admit(message),
            Input::Message(message) => self.message(message),
            Input::Request {
                count,
                before_utc_ms,
            } if self.phase == Phase::Connected => self.request(count, before_utc_ms),
            Input::Request { .. } => return Err(InvalidInput),
            other => self.line_fault(other),
        };
        Ok(effect)
    }
    #[inline]
    pub fn resume(&mut self, ack: Ack) -> Result<Effect, InvalidInput> {
        if self.deferred.is_none()
            && self.diagnostic_count == 0
            && self.publication.is_none()
            && self.phase == Phase::Connected
            && matches!(ack, Ack::Published(true))
            && matches!(self.pending, Some((Expect::Publish, After::Wait)))
        {
            self.pending = None;
            return Ok(self.wait());
        }
        self.resume_pending(ack)
    }
    fn resume_pending(&mut self, ack: Ack) -> Result<Effect, InvalidInput> {
        if self.deferred.is_some() {
            if !matches!(ack, Ack::Diagnostic) || !self.diagnostic_taken {
                return Err(InvalidInput);
            }
            self.diagnostic_taken = false;
            if self.diagnostic_count != 0 {
                return Ok(Effect::Diagnostic);
            }
            return Ok(self.deferred.take().expect("deferred effect"));
        }
        let Some((expected, _)) = self.pending.as_ref() else {
            return Err(InvalidInput);
        };
        if !expected.matches(&ack) || self.publication.is_some() {
            return Err(InvalidInput);
        }
        let (_, after) = self.pending.take().expect("validated continuation");
        let effect = if matches!(ack, Ack::Published(false)) && self.phase != Phase::Closing {
            self.terminate(ConnEnd::UiGone)
        } else {
            self.continue_after(after, ack)
        };
        Ok(self.expose(effect))
    }
    pub fn take_end(&mut self) -> ConnEnd {
        self.end.take().expect("finished connection")
    }
    /// Move only the issued publication; taking its payload never acknowledges it.
    pub fn take_publication(&mut self) -> Result<Mt5Event, InvalidInput> {
        if self.deferred.is_some() || !matches!(self.pending, Some((Expect::Publish, _))) {
            return Err(InvalidInput);
        }
        self.publication.take().ok_or(InvalidInput)
    }
    pub(super) fn state(&mut self) -> &mut Connected {
        self.connected.as_mut().expect("connected phase")
    }
    pub(super) fn log(&mut self, diagnostic: Diagnostic) {
        let slot = self
            .diagnostics
            .iter_mut()
            .find(|slot| slot.is_none())
            .expect("bounded diagnostic budget");
        *slot = Some(diagnostic);
        self.diagnostic_count += 1;
    }
    /// Move one issued fact while retaining its deferred effect until acknowledgement.
    pub fn take_diagnostic(&mut self) -> Result<Diagnostic, InvalidInput> {
        if self.deferred.is_none() || self.diagnostic_taken || self.diagnostic_count == 0 {
            return Err(InvalidInput);
        }
        self.diagnostic_count -= 1;
        self.diagnostic_taken = true;
        Ok(self
            .diagnostics
            .iter_mut()
            .find(|slot| slot.is_some())
            .and_then(Option::take)
            .expect("diagnostic count matches stored facts"))
    }
    #[inline]
    fn expose(&mut self, effect: Effect) -> Effect {
        if self.diagnostic_count != 0 {
            self.deferred = Some(effect);
            Effect::Diagnostic
        } else {
            effect
        }
    }
    #[inline]
    pub(super) fn yield_to(&mut self, effect: Effect, expect: Expect, after: After) -> Effect {
        self.pending = Some((expect, after));
        effect
    }
    pub(super) fn publish(&mut self, event: Mt5Event, after: After) -> Effect {
        assert!(
            self.publication.is_none(),
            "publication must be consumed before continuing"
        );
        self.publication = Some(event);
        self.yield_to(Effect::Publish, Expect::Publish, after)
    }
    pub(super) fn wait(&self) -> Effect {
        Effect::Wait(self.phase)
    }
    pub(super) fn terminate(&mut self, end: ConnEnd) -> Effect {
        self.end = Some(end);
        if self.phase == Phase::AwaitHello {
            self.phase = Phase::Closed;
            Effect::Finished
        } else {
            self.phase = Phase::Closing;
            if self.state().backfill.is_some() {
                self.log(Diagnostic::BackfillDiscard);
            }
            self.yield_to(
                Effect::Pager(PagerOp::Abandon),
                Expect::Pager,
                After::ClosePage,
            )
        }
    }
    fn admit(&mut self, message: BridgeMsg) -> Effect {
        let BridgeMsg::Hello(hello) = message else {
            self.log(Diagnostic::FirstMessage(message));
            return self.terminate(ConnEnd::BridgeGone("no hello".into()));
        };
        if let Some((reason, diagnostic)) = hello::refusal(&hello, self.symbol) {
            self.log(diagnostic);
            return self.terminate(ConnEnd::BridgeGone(reason));
        }
        self.log(Diagnostic::Hello(hello.clone()));
        let event = Mt5Event::Status(Mt5Status::Connected {
            symbol: hello.symbol.clone(),
            broker_symbol: hello.broker_symbol.clone(),
            tape: hello.tape,
            book_levels: hello.book_levels,
            rates: hello.rates.unwrap_or(false),
            history_paging: hello.history_paging.unwrap_or(false),
            deal_counter: hello.deal_counter.unwrap_or(false),
        });
        self.publish(event, After::Connected(hello))
    }
    fn connect(&mut self, hello: protocol::Hello) -> Effect {
        self.log(Diagnostic::Capabilities {
            bridge: hello.bridge.clone(),
            version: hello.bridge_version.clone(),
            paging: hello.history_paging.unwrap_or(false),
            depth: hello.book_levels.is_some(),
        });
        self.connected = Some(Connected {
            mapper: TickMapper::new(self.side_mode, hello.server_utc_offset_s)
                .with_tape(hello.tape),
            tracker: SeqTracker::new(),
            highest_tick_ms: None,
            latency: LatencyTracker::new(),
            deals: DealSampler::new(hello.server_utc_offset_s),
            lag_reported: false,
            backfill: None,
            candles: None,
            depth: DepthSession::new(&hello, self.symbol.to_string()),
            history: HistoryPage::default(),
            undecodable: 0,
            hello,
        });
        self.phase = Phase::Connected;
        self.wait()
    }
    fn line_fault(&mut self, input: Input) -> Effect {
        let hello = self.phase == Phase::AwaitHello;
        let reason = match input {
            Input::Blank { .. } if !hello => return self.wait(),
            Input::Blank { error, snippet } => {
                self.log(Diagnostic::Undecodable {
                    hello,
                    error,
                    snippet,
                    total: 0,
                });
                "undecodable hello".to_string()
            }
            Input::Eof => {
                self.log(Diagnostic::Eof { hello });
                if hello { "closed before hello" } else { "eof" }.into()
            }
            Input::Oversized => {
                self.log(Diagnostic::Oversized { hello });
                if hello {
                    "oversized hello"
                } else {
                    "oversized line"
                }
                .into()
            }
            Input::Timeout => {
                self.log(Diagnostic::Timeout {
                    hello,
                    seconds: if hello {
                        self.hello_timeout_s
                    } else {
                        self.read_timeout_s
                    },
                });
                if hello { "hello timeout" } else { "silent" }.into()
            }
            Input::ReadError(error) => {
                let reason = if hello {
                    format!("socket error before hello: {error}")
                } else {
                    format!("socket error: {error}")
                };
                self.log(Diagnostic::Socket { hello, error });
                reason
            }
            Input::NonUtf8 { len } => {
                let total = if hello {
                    0
                } else {
                    self.state().undecodable += 1;
                    self.state().undecodable
                };
                self.log(Diagnostic::Utf8 { hello, len, total });
                if !hello {
                    return self.wait();
                }
                "undecodable hello".into()
            }
            Input::Undecodable { error, snippet } => {
                let total = if hello {
                    0
                } else {
                    self.state().undecodable += 1;
                    self.state().undecodable
                };
                self.log(Diagnostic::Undecodable {
                    hello,
                    error,
                    snippet,
                    total,
                });
                if !hello {
                    return self.wait();
                }
                "undecodable hello".into()
            }
            _ => unreachable!("messages and requests are dispatched separately"),
        };
        self.terminate(ConnEnd::BridgeGone(reason))
    }
    fn message(&mut self, message: BridgeMsg) -> Effect {
        match message {
            BridgeMsg::Tick(tick) => self.tick(tick),
            BridgeMsg::Heartbeat(heartbeat) => self.heartbeat(heartbeat),
            BridgeMsg::Book(image) => {
                if !self.state().depth.declared() {
                    return self.wait();
                }
                self.yield_to(
                    Effect::ReadCapture,
                    Expect::Capture,
                    After::BookCapture(image),
                )
            }
            BridgeMsg::HistoryStart {
                count_hint,
                opening,
            } => {
                if opening {
                    self.history_start(count_hint, true, true)
                } else {
                    self.yield_to(
                        Effect::Pager(PagerOp::IsInFlight),
                        Expect::Pager,
                        After::HistoryStart { count_hint },
                    )
                }
            }
            BridgeMsg::HistoryEnd {
                exhausted,
                scanned_to_ms,
                remaining,
            } => self.history_end(exhausted, scanned_to_ms, remaining),
            BridgeMsg::BackfillStart { count_hint } => {
                self.log(Diagnostic::BackfillStart(count_hint));
                self.state().backfill = Some(Vec::new());
                self.wait()
            }
            BridgeMsg::BackfillEnd {} => {
                let batch = self.state().backfill.take().unwrap_or_default();
                self.log(Diagnostic::BackfillEnd(batch.len()));
                self.publish(Mt5Event::Backfilled(batch), After::Wait)
            }
            BridgeMsg::RatesStart {
                interval_ms,
                count_hint,
            } => self.rates_start(interval_ms, count_hint),
            BridgeMsg::Rate(chunk) => self.rate(chunk),
            BridgeMsg::RatesEnd { partial } => self.rates_end(partial),
            BridgeMsg::Bye { reason } => {
                let end = ConnEnd::BridgeGone(format!("bye: {reason}"));
                self.log(Diagnostic::Bye(reason));
                self.terminate(end)
            }
            BridgeMsg::Hello(_) => {
                self.log(Diagnostic::Violation {
                    message: "second hello mid-session; ignoring it",
                    action: "ignore",
                    bars: 0,
                });
                self.wait()
            }
        }
    }
    fn continue_after(&mut self, after: After, ack: Ack) -> Effect {
        match after {
            After::Wait => self.wait(),
            After::Connected(hello) => self.connect(hello),
            After::TickDeal {
                tick,
                tick_ms,
                advances,
            } => self.tick_deal(tick, tick_ms, advances),
            After::TickMap(tick) => self.tick_map(tick),
            After::Sample(target) => {
                let Ack::Time(now) = ack else { unreachable!() };
                self.sample(now, target)
            }
            After::Live(trade) => self.yield_to(Effect::Live(trade), Expect::Publish, After::Wait),
            After::HeartbeatOffset(heartbeat) => self.heartbeat_offset(heartbeat),
            After::HeartbeatCapture(heartbeat) => self.yield_to(
                Effect::ReadCapture,
                Expect::Capture,
                After::HeartbeatLog(heartbeat),
            ),
            After::HeartbeatLog(heartbeat) => {
                let Ack::Capture {
                    enabled,
                    base_generation,
                } = ack
                else {
                    unreachable!()
                };
                self.heartbeat_depth(heartbeat, enabled, base_generation)
            }
            After::HeartbeatDone(heartbeat) => self.heartbeat_done(heartbeat),
            After::BookCapture(image) => {
                let Ack::Capture {
                    enabled,
                    base_generation,
                } = ack
                else {
                    unreachable!()
                };
                let state = self.connected.as_mut().expect("connected");
                let step = state.depth.observe(
                    image,
                    enabled,
                    base_generation,
                    &mut self.generation_offset,
                );
                self.depth_step(step)
            }
            After::Depth => {
                let state = self.connected.as_mut().expect("connected");
                let step = state.depth.acknowledged(&mut self.generation_offset);
                self.depth_step(step)
            }
            After::HistoryStart { count_hint } => {
                let Ack::Pager(in_flight) = ack else {
                    unreachable!()
                };
                self.history_start(count_hint, false, in_flight)
            }
            After::HistoryEnd {
                page,
                exhausted,
                scanned_to_ms,
            } => {
                let Ack::Pager(owed) = ack else {
                    unreachable!()
                };
                self.history_settled(page, exhausted, scanned_to_ms, owed)
            }
            After::Unsupported => self.publish(empty_page(), After::Wait),
            After::Written => {
                let Ack::Written(result) = ack else {
                    unreachable!()
                };
                match result {
                    Ok(()) => self.wait(),
                    Err(error) => self.terminate(ConnEnd::BridgeGone(format!(
                        "socket error on write: {error}"
                    ))),
                }
            }
            After::ClosePage => {
                let Ack::Pager(owed) = ack else {
                    unreachable!()
                };
                self.close_page(owed)
            }
            After::CloseRates => self.close_rates(),
            After::CloseDepth => self.close_depth(),
            After::Closed => {
                self.phase = Phase::Closed;
                Effect::Finished
            }
        }
    }
}
pub(super) fn empty_page() -> Mt5Event {
    Mt5Event::HistoryPage {
        trades: Vec::new(),
        exhausted: false,
        scanned_to_utc_ms: None,
    }
}
