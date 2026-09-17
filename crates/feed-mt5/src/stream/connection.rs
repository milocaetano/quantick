//! Socket/framing, scheduling and effect execution for a synchronous protocol owner.
mod diagnostic;
mod effect;
mod hello;
mod history;
#[cfg(test)]
mod machine_tests;
#[cfg(test)]
#[path = "connection_owner_tests.rs"]
mod owner_tests;
mod session;
mod transitions;
#[cfg(test)]
mod wait_tests;

use super::events::{ConnEnd, Mt5Event};
use super::reader::{BoundedLine, BoundedLineReader};
use super::{ServerConfig, snippet};
use crate::protocol::{self, FeedMsg};
use effect::{Ack, Effect, Input, PagerOp, Phase};
use session::SessionMachine;
use tokio::{io::AsyncWriteExt as _, net::TcpStream, sync::mpsc};

/// One reader and one pinned pager wait survive every processing continuation.
/// The timeout wraps only waiting; effects and consumer backpressure are outside it.
pub(super) async fn serve_connection(
    stream: TcpStream,
    config: &ServerConfig,
    tx: &mpsc::Sender<Mt5Event>,
    generation_offset: &mut u64,
) -> ConnEnd {
    let (incoming, mut outgoing) = stream.into_split();
    let mut lines = BoundedLineReader::new(incoming);
    let mut machine = SessionMachine::new(
        &config.symbol,
        config.side_mode,
        *generation_offset,
        config.hello_timeout.as_secs(),
        config.read_timeout.as_secs(),
    );
    let pending_request = config.history_pager.take_request();
    tokio::pin!(pending_request);
    let mut backpressure_reported = false;
    let mut effect = Effect::Wait(Phase::AwaitHello);
    loop {
        effect = match effect {
            Effect::Wait(phase) => {
                let input = if phase == Phase::AwaitHello {
                    match tokio::time::timeout(config.hello_timeout, lines.next_line()).await {
                        Ok(line) => decode(line),
                        Err(_) => Input::Timeout,
                    }
                } else {
                    match tokio::time::timeout(
                        config.read_timeout,
                        select_input(&mut lines, pending_request.as_mut()),
                    )
                    .await
                    {
                        Ok(Wake::Request {
                            count,
                            before_utc_ms,
                        }) => {
                            pending_request.set(config.history_pager.take_request());
                            Input::Request {
                                count,
                                before_utc_ms,
                            }
                        }
                        Ok(Wake::Line(line)) => decode(line),
                        Err(_) => Input::Timeout,
                    }
                };
                machine
                    .input(input)
                    .expect("driver input only follows Wait")
            }
            Effect::Publish => {
                let event = machine.take_publication().expect("issued publication");
                let success = tx.send(event).await.is_ok();
                machine
                    .resume(Ack::Published(success))
                    .expect("publication acknowledgement")
            }
            Effect::Live(trade) => {
                let success = super::publish::send_live(
                    tx,
                    trade,
                    &config.symbol,
                    &mut backpressure_reported,
                )
                .await
                .is_ok();
                machine
                    .resume(Ack::Published(success))
                    .expect("live acknowledgement")
            }
            Effect::Write(message) => {
                let result = write_message(&mut outgoing, &message)
                    .await
                    .map_err(|error| error.to_string());
                machine
                    .resume(Ack::Written(result))
                    .expect("write acknowledgement")
            }
            Effect::SampleTime => machine
                .resume(Ack::Time(super::publish::wall_clock_ms()))
                .expect("clock acknowledgement"),
            Effect::ReadCapture => {
                let (enabled, base_generation) = config.book_capture.state();
                machine
                    .resume(Ack::Capture {
                        enabled,
                        base_generation,
                    })
                    .expect("capture acknowledgement")
            }
            Effect::Pager(operation) => {
                let result = match operation {
                    PagerOp::IsInFlight => config.history_pager.is_in_flight(),
                    PagerOp::SettleOwed => config.history_pager.settle_owed(),
                    PagerOp::Abandon => config.history_pager.abandon(),
                };
                machine
                    .resume(Ack::Pager(result))
                    .expect("pager acknowledgement")
            }
            Effect::Diagnostic => {
                let fact = machine.take_diagnostic().expect("issued diagnostic");
                diagnostic::log(fact, &config.symbol);
                machine
                    .resume(Ack::Diagnostic)
                    .expect("diagnostic acknowledgement")
            }
            Effect::Finished => {
                *generation_offset = machine.generation_offset;
                return machine.take_end();
            }
        };
    }
}

enum Wake {
    Line(std::io::Result<BoundedLine>),
    Request { count: u64, before_utc_ms: i64 },
}

/// The same biased wait is exercised with controlled readers and pager futures.
async fn select_input<R, P>(
    lines: &mut BoundedLineReader<R>,
    pending_request: std::pin::Pin<&mut P>,
) -> Wake
where
    R: tokio::io::AsyncRead + Unpin,
    P: std::future::Future<Output = (u64, i64)>,
{
    tokio::select! {
        biased;
        (count,before_utc_ms)=pending_request=>Wake::Request {count,before_utc_ms},
        line=lines.next_line()=>Wake::Line(line),
    }
}

async fn write_message(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    message: &FeedMsg,
) -> std::io::Result<()> {
    writer
        .write_all(protocol::encode_line(message).as_bytes())
        .await
}

fn decode(line: std::io::Result<BoundedLine>) -> Input {
    match line {
        Err(error) => Input::ReadError(error.to_string()),
        Ok(BoundedLine::Eof) => Input::Eof,
        Ok(BoundedLine::TooLong) => Input::Oversized,
        Ok(BoundedLine::NotUtf8 { len }) => Input::NonUtf8 { len },
        Ok(BoundedLine::Line(line)) => {
            if line.trim().is_empty() {
                return Input::Blank {
                    error: protocol::parse_line(&line).unwrap_err(),
                    snippet: snippet(&line).to_string(),
                };
            }
            match protocol::parse_line(&line) {
                Ok(message) => Input::Message(message),
                Err(error) => Input::Undecodable {
                    error,
                    snippet: snippet(&line).to_string(),
                },
            }
        }
    }
}
