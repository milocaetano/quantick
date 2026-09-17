//! Ordered data transitions of the synchronous session.
use super::super::{
    LAG_REPORT_MS,
    blocks::{DepthStep, RatesBlock},
    events::Mt5Event,
};
use super::{
    diagnostic::Diagnostic,
    effect::*,
    history::CompletedPage,
    session::{After, Expect, SampleTarget, SessionMachine, empty_page},
};
use crate::{
    map::MapOutcome,
    protocol::{self, FeedMsg},
};

impl SessionMachine<'_> {
    pub(super) fn tick(&mut self, tick: protocol::Tick) -> Effect {
        let state = self.state();
        let tick_ms = state.mapper.to_utc_ms(tick.time_ms);
        let advances = state
            .tracker
            .highest()
            .is_none_or(|highest| tick.seq > highest);
        let anomaly = state.tracker.observe(tick.seq);
        if let (Some(anomaly), Some(from_ms)) = (anomaly, state.highest_tick_ms) {
            return self.publish(
                Mt5Event::SequenceAnomaly {
                    anomaly,
                    from_ms,
                    to_ms: tick_ms,
                },
                After::TickDeal {
                    tick,
                    tick_ms,
                    advances,
                },
            );
        }
        self.tick_deal(tick, tick_ms, advances)
    }
    pub(super) fn tick_deal(
        &mut self,
        tick: protocol::Tick,
        tick_ms: i64,
        advances: bool,
    ) -> Effect {
        let state = self.state();
        if advances {
            state.highest_tick_ms = Some(tick_ms);
        }
        if let Some(sample) = state.deals.observe(&tick) {
            return self.publish(Mt5Event::DealCounter(sample), After::TickMap(tick));
        }
        self.tick_map(tick)
    }
    pub(super) fn tick_map(&mut self, tick: protocol::Tick) -> Effect {
        let state = self.state();
        if let MapOutcome::Trade { trade, .. } = state.mapper.map(&tick)
            && let Some(trade) = state.history.collect(trade)
        {
            if let Some(backfill) = state.backfill.as_mut() {
                backfill.push(trade);
            } else {
                state.latency.observe_live(tick.time_ms, tick.sent_ms);
                if state.latency.due() {
                    return self.yield_to(
                        Effect::SampleTime,
                        Expect::Time,
                        After::Sample(SampleTarget::Live(trade)),
                    );
                }
                return self.yield_to(Effect::Live(trade), Expect::Publish, After::Wait);
            }
        }
        self.wait()
    }
    pub(super) fn sample(&mut self, now: i64, target: SampleTarget) -> Effect {
        let state = self.state();
        let sample = state
            .latency
            .sample(now, state.mapper.server_utc_offset_ms());
        if let Some(sample) = sample {
            let late = sample.arrival_lag_ms >= LAG_REPORT_MS;
            if late != state.lag_reported {
                state.lag_reported = late;
                self.log(Diagnostic::Lag { late, sample });
            }
            let after = match target {
                SampleTarget::Live(trade) => After::Live(trade),
                SampleTarget::Heartbeat(hb) => After::HeartbeatCapture(hb),
            };
            return self.publish(Mt5Event::Latency(sample), after);
        }
        match target {
            SampleTarget::Live(trade) => {
                self.yield_to(Effect::Live(trade), Expect::Publish, After::Wait)
            }
            SampleTarget::Heartbeat(hb) => self.yield_to(
                Effect::ReadCapture,
                Expect::Capture,
                After::HeartbeatLog(hb),
            ),
        }
    }
    pub(super) fn heartbeat(&mut self, hb: protocol::Heartbeat) -> Effect {
        if let Some(sample) = self.state().deals.finish() {
            return self.publish(Mt5Event::DealCounter(sample), After::HeartbeatOffset(hb));
        }
        self.heartbeat_offset(hb)
    }
    pub(super) fn heartbeat_offset(&mut self, hb: protocol::Heartbeat) -> Effect {
        if let Some(offset) = hb.server_utc_offset_s {
            let state = self.state();
            state.mapper.set_server_utc_offset_s(offset);
            state.depth.set_server_utc_offset_s(offset);
            state.deals.set_server_utc_offset_s(offset);
        }
        self.yield_to(
            Effect::SampleTime,
            Expect::Time,
            After::Sample(SampleTarget::Heartbeat(hb)),
        )
    }
    pub(super) fn heartbeat_depth(
        &mut self,
        hb: protocol::Heartbeat,
        enabled: bool,
        base: u64,
    ) -> Effect {
        if let Some(event) = self.state().depth.missing(enabled, base) {
            self.log(Diagnostic::MissingDepth);
            return self.publish(Mt5Event::Depth(event), After::HeartbeatDone(hb));
        }
        self.heartbeat_done(hb)
    }
    pub(super) fn heartbeat_done(&mut self, hb: protocol::Heartbeat) -> Effect {
        self.log(Diagnostic::Heartbeat {
            seq_last: hb.seq_last,
            ticks_sent: hb.ticks_sent,
        });
        self.wait()
    }
    pub(super) fn depth_step(&mut self, step: DepthStep) -> Effect {
        match step {
            DepthStep::Done => self.wait(),
            DepthStep::Publish(event) => self.publish(Mt5Event::Depth(event), After::Depth),
            DepthStep::Mapped(mapped) => {
                for diagnostic in mapped.diagnostics.into_iter().flatten() {
                    self.log(Diagnostic::Book(diagnostic));
                }
                match mapped.event {
                    Some(event) => self.publish(Mt5Event::Depth(event), After::Depth),
                    None => self.wait(),
                }
            }
        }
    }
    pub(super) fn request(&mut self, count: u64, before_utc_ms: i64) -> Effect {
        if !self.state().hello.history_paging.unwrap_or(false) {
            self.log(Diagnostic::RequestUnsupported(count));
            return self.yield_to(
                Effect::Pager(PagerOp::SettleOwed),
                Expect::Pager,
                After::Unsupported,
            );
        }
        let before_ms = self.state().mapper.to_server_ms(before_utc_ms);
        self.log(Diagnostic::Request {
            count,
            before_ms,
            before_utc_ms,
        });
        self.yield_to(
            Effect::Write(FeedMsg::LoadOlder { count, before_ms }),
            Expect::Write,
            After::Written,
        )
    }
    pub(super) fn history_start(
        &mut self,
        count_hint: Option<u64>,
        opening: bool,
        in_flight: bool,
    ) -> Effect {
        self.log(Diagnostic::HistoryStart {
            count_hint,
            unsolicited: !opening && !in_flight,
        });
        let state = self.state();
        state.history.start(&mut state.mapper, opening);
        self.wait()
    }
    pub(super) fn history_end(
        &mut self,
        exhausted: bool,
        scanned_to_ms: Option<i64>,
        remaining: Option<u64>,
    ) -> Effect {
        let state = self.state();
        let page = state.history.end(&mut state.mapper);
        if let Some(page) = page {
            if page.over_cap > 0 {
                self.log(Diagnostic::HistoryTruncated(page.over_cap));
            }
            if page.opening {
                self.log(Diagnostic::Opening {
                    trades: page.trades.len(),
                    remaining,
                });
                return self.publish(
                    Mt5Event::OpeningPage {
                        trades: page.trades,
                        remaining,
                    },
                    After::Wait,
                );
            }
            return self.yield_to(
                Effect::Pager(PagerOp::SettleOwed),
                Expect::Pager,
                After::HistoryEnd {
                    page: Some(page),
                    exhausted,
                    scanned_to_ms,
                },
            );
        }
        self.log(Diagnostic::Violation {
            message: "history_end without a history_start; its ticks went out as live",
            action: "settle_and_answer_empty",
            bars: 0,
        });
        self.yield_to(
            Effect::Pager(PagerOp::SettleOwed),
            Expect::Pager,
            After::HistoryEnd {
                page: None,
                exhausted,
                scanned_to_ms,
            },
        )
    }
    pub(super) fn history_settled(
        &mut self,
        page: Option<CompletedPage>,
        exhausted: bool,
        scanned_to_ms: Option<i64>,
        owed: bool,
    ) -> Effect {
        let Some(page) = page else {
            return if owed {
                self.publish(empty_page(), After::Wait)
            } else {
                self.wait()
            };
        };
        if !owed {
            self.log(Diagnostic::HistoryDiscard(page.trades.len()));
            return self.wait();
        }
        self.log(Diagnostic::HistoryEnd {
            trades: page.trades.len(),
            exhausted,
            scanned_to_ms,
        });
        let scanned_to_utc_ms = scanned_to_ms.map(|ms| self.state().mapper.to_utc_ms(ms));
        self.publish(
            Mt5Event::HistoryPage {
                trades: page.trades,
                exhausted,
                scanned_to_utc_ms,
            },
            After::Wait,
        )
    }
    pub(super) fn rates_start(&mut self, interval_ms: i64, count_hint: Option<u64>) -> Effect {
        self.log(Diagnostic::RatesStart {
            interval_ms,
            count_hint,
        });
        if let Some(open) = self.state().candles.take() {
            self.log(Diagnostic::Violation {
                message: "a second rates_start arrived inside an open block; discarding the first",
                bars: open.len(),
                action: "discard_open_block",
            });
        }
        let offset = self.state().hello.server_utc_offset_s;
        self.state().candles = Some(RatesBlock::new(interval_ms, offset));
        self.wait()
    }
    pub(super) fn rate(&mut self, chunk: protocol::RateChunk) -> Effect {
        match self.state().candles.as_mut() {
            Some(block) => {
                if block.absorb(&chunk) {
                    self.log(Diagnostic::RatesTruncated);
                }
            }
            None => self.log(Diagnostic::Violation {
                message: "candles arrived outside a rates block; dropping them",
                bars: chunk.bars.len(),
                action: "drop_chunk",
            }),
        }
        self.wait()
    }
    pub(super) fn rates_end(&mut self, bridge: bool) -> Effect {
        if let Some(block) = self.state().candles.take() {
            let (stats, interval_ms) = block.stats();
            self.log(Diagnostic::RatesSummary { stats, interval_ms });
            let (interval_ms, bars, clipped) = block.finish();
            let partial = bridge || clipped;
            if partial {
                self.log(Diagnostic::RatesPartial {
                    bars: bars.len(),
                    bridge,
                    clipped,
                });
            }
            return self.publish(
                Mt5Event::Rates {
                    interval_ms,
                    bars,
                    partial,
                },
                After::Wait,
            );
        }
        self.log(Diagnostic::Violation {
            message: "rates_end without a rates_start; ignoring it",
            bars: 0,
            action: "ignore",
        });
        self.wait()
    }
    pub(super) fn close_page(&mut self, owed: bool) -> Effect {
        if owed {
            let partial = self.state().history.len();
            self.log(Diagnostic::HistoryUnanswered(partial));
            return self.publish(empty_page(), After::CloseRates);
        }
        self.close_rates()
    }
    pub(super) fn close_rates(&mut self) -> Effect {
        if let Some(block) = self.state().candles.take() {
            self.log(Diagnostic::RatesDiscard(block.len()));
        }
        let stats = self.state().mapper.stats;
        self.log(Diagnostic::MapSummary(stats));
        self.close_deals()
    }
    pub(super) fn close_deals(&mut self) -> Effect {
        if let Some(sample) = self.state().deals.finish() {
            return self.publish(Mt5Event::DealCounter(sample), After::CloseDepth);
        }
        self.close_depth()
    }
    pub(super) fn close_depth(&mut self) -> Effect {
        let (stats, event) = self.state().depth.close();
        if let Some(stats) = stats {
            self.log(Diagnostic::BookSummary(stats));
        }
        if let Some(event) = event {
            return self.publish(Mt5Event::Depth(event), After::Closed);
        }
        self.phase = Phase::Closed;
        Effect::Finished
    }
}
