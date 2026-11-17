//! Every word and tone the route shows: the Trading panel's title, its
//! `Account` selector and the list it opens, the toolbar's read-only notice
//! and the status line while the link is down. Decided here, painted by the
//! window, so a second operator reads the very same sentences.

use rust_decimal::Decimal;

use quantick_trading::{AccountFacts, BrokerVenue, LinkState};

use super::desk::{AccountsPort, BrokerDesk, SourceFacts};
use super::{AccountRoute, Disarm, Refusal, Route};
use crate::format::{fmt_decimal, position_word};

/// The colour family a route wears. The window owns the pixels; the two
/// broker tones are fixed here because a trader learns them as meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteTone {
    /// The simulator: the panel's ordinary ink.
    Sim,
    /// The simulator because the source cannot route: muted.
    Locked,
    Demo,
    Real,
    /// A broker account whose link is down.
    Lost,
}

impl RouteTone {
    /// The tone's own colour, `None` for the window's ordinary ink.
    #[must_use]
    pub const fn rgb(self) -> Option<[u8; 3]> {
        match self {
            Self::Sim | Self::Locked => None,
            Self::Demo => Some([0x1F, 0x5F, 0xBF]),
            Self::Real | Self::Lost => Some([0xC6, 0x28, 0x28]),
        }
    }
}

/// The link mark beside an account name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    None,
    /// Filled: the link is up.
    Up,
    /// Hollow: the link is down.
    Hollow,
}

impl Dot {
    /// The mark as text, with the space that follows it.
    #[must_use]
    pub const fn mark(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Up => "● ",
            Self::Hollow => "○ ",
        }
    }
}

/// The toolbar's read-only notice: `orders → XP 51234567 · REAL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteNotice {
    pub lead: &'static str,
    pub text: String,
    pub tone: RouteTone,
    pub dot: Dot,
}

/// How a row reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    /// The route in use.
    Selected,
    Enabled,
    /// Not available now; the sheet's footer or the row says why.
    Off,
}

/// What a press on a row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowGesture {
    /// One click selects the route.
    Click(Route),
    /// Press and hold [`super::HOLD_TO_ARM_MS`] to arm REAL.
    Hold,
    None,
}

/// One row of the account list.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountRow {
    pub title: String,
    pub hint: String,
    pub subs: Vec<String>,
    pub state: RowState,
    pub tone: RouteTone,
    pub gesture: RowGesture,
    /// How far a REAL hold has come, while it is held.
    pub progress: Option<f32>,
}

/// The Trading panel's route block and everything around it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// The panel title: `PAPER TRADING · SIM`, `TRADING · REAL`.
    pub title: String,
    pub tone: RouteTone,
    /// The selector's caption.
    pub selector_label: &'static str,
    /// The selector's face: the same words as the notice.
    pub selector: String,
    /// The source cannot route: the selector stays on the simulator.
    pub locked: bool,
    pub notice: RouteNotice,
    pub rows: Vec<AccountRow>,
    /// Why the broker rows are off, under the list.
    pub footer: Option<String>,
    /// A leave with exposure waits for the trader's choice.
    pub leave_prompt: bool,
    /// The account list is open.
    pub open: bool,
    /// The status line's words while a broker account is shown without a
    /// link: `no link with the broker for 14 s · disarmed`.
    pub status: Option<String>,
}

/// What the REAL row says about the next order: the instrument, what one
/// point of it is worth (`0.20 BRL/pt`) and the size the ticket holds.
#[derive(Debug, Clone, Default)]
struct TicketFacts {
    symbol: String,
    point_value: Option<String>,
    quantity: Option<Decimal>,
}

/// The panel title for the simulator.
pub const SIM_TITLE: &str = "PAPER TRADING · SIM";
/// The notice's lead.
pub const NOTICE_LEAD: &str = "orders →";
const SIMULATED: &str = "Simulated";
/// The broker row's hint while the venue waits for the terminal's account.
const CONNECTING: &str = "connecting…";
/// The broker row while no venue could be built (the port was taken).
const NOT_CONNECTED: &str = "not connected";

impl BrokerDesk {
    /// The words and tones for this frame. `quantity` is the size the
    /// ticket would send; the instrument is the active chart's.
    #[must_use]
    pub fn sheet(
        &self,
        port: &dyn AccountsPort,
        source: SourceFacts,
        quantity: Option<Decimal>,
        now_ms: i64,
    ) -> Sheet {
        let active = port.account(port.active_tab());
        let ticket = TicketFacts {
            symbol: active
                .map(|account| account.symbol().to_owned())
                .unwrap_or_default(),
            point_value: active
                .and_then(crate::PaperAccount::symbol_money)
                .map(|money| {
                    format!(
                        "{} {}/pt",
                        money_text(money.point_value),
                        money.currency.code()
                    )
                }),
            quantity,
        };
        let route = self.route();
        let facts = self.facts(port, source);
        let venue = self.venue(port);
        let account = venue.and_then(BrokerVenue::account);
        // A parked venue with exposure is still what the chart shows.
        let shown = port
            .account(port.active_tab())
            .is_some_and(crate::PaperAccount::broker_shown);
        let link_up = facts.link_up;
        let locked = source.broker_enabled && !source.source_routes;
        let (tone, face, dot) = match (route.route(), account) {
            (Route::Demo | Route::Real, Some(account)) => (
                if route.route() == Route::Real {
                    RouteTone::Real
                } else {
                    RouteTone::Demo
                },
                format!("{} · {}", account.short_name(), route.route().word()),
                Dot::Up,
            ),
            (Route::Sim, Some(account)) if shown => {
                if link_up {
                    let kind = Route::of_mode(account.trade_mode);
                    (
                        tone_of(kind),
                        format!(
                            "{} · {} · open at broker",
                            account.short_name(),
                            kind.word()
                        ),
                        Dot::Up,
                    )
                } else {
                    (
                        RouteTone::Lost,
                        format!("{} · no link", account.short_name()),
                        Dot::Hollow,
                    )
                }
            }
            _ if locked && source.replay => (
                RouteTone::Locked,
                format!("{SIMULATED} · replay"),
                Dot::None,
            ),
            _ if locked => (RouteTone::Locked, SIMULATED.to_owned(), Dot::None),
            _ => (RouteTone::Sim, SIMULATED.to_owned(), Dot::None),
        };
        let title = match tone {
            RouteTone::Sim | RouteTone::Locked => SIM_TITLE.to_owned(),
            _ => format!(
                "TRADING · {}",
                account
                    .map_or(Route::Real, |facts| Route::of_mode(facts.trade_mode))
                    .word()
            ),
        };
        let status = (tone == RouteTone::Lost)
            .then(|| route.last_disarm())
            .flatten()
            .map(|(reason, at)| lost_status(reason, at, now_ms));
        let (rows, footer) = rows(route, &facts, source, venue, &ticket, now_ms);
        Sheet {
            title,
            tone,
            selector_label: "Account",
            selector: face.clone(),
            locked,
            notice: RouteNotice {
                lead: NOTICE_LEAD,
                text: face,
                tone,
                dot,
            },
            rows,
            footer,
            leave_prompt: route.leaving(),
            open: self.sheet_open || route.leaving(),
            status,
        }
    }
}

fn tone_of(route: Route) -> RouteTone {
    match route {
        Route::Sim => RouteTone::Sim,
        Route::Demo => RouteTone::Demo,
        Route::Real => RouteTone::Real,
    }
}

fn lost_status(reason: Disarm, at_ms: i64, now_ms: i64) -> String {
    let seconds = now_ms.saturating_sub(at_ms).max(0) / 1_000;
    match reason {
        Disarm::LinkLost => format!("no link with the broker for {seconds} s · disarmed"),
        other => other.sentence().to_owned(),
    }
}

fn rows(
    route: &AccountRoute,
    facts: &super::RouteFacts,
    source: SourceFacts,
    venue: Option<&dyn BrokerVenue>,
    ticket: &TicketFacts,
    now_ms: i64,
) -> (Vec<AccountRow>, Option<String>) {
    let on_sim = route.route() == Route::Sim;
    let mut rows = vec![AccountRow {
        title: SIMULATED.to_owned(),
        hint: if on_sim { "in use" } else { "one click" }.to_owned(),
        subs: vec![if source.replay && !ticket.symbol.is_empty() {
            format!("Replay of {}.", ticket.symbol)
        } else {
            "Fills from the tape. No broker.".to_owned()
        }],
        state: if on_sim {
            RowState::Selected
        } else {
            RowState::Enabled
        },
        tone: RouteTone::Sim,
        gesture: RowGesture::Click(Route::Sim),
        progress: None,
    }];
    if !source.broker_enabled {
        return (rows, None);
    }
    let Some(account) = venue.and_then(BrokerVenue::account) else {
        // The list opening is what builds the venue: until the terminal has
        // said which account it holds, the row is connecting.
        let (hint, waiting) = match venue.map(BrokerVenue::link_state) {
            None => ("", NOT_CONNECTED),
            Some(LinkState::Lost) => ("", "no link with the execution bridge"),
            Some(_) => (
                CONNECTING,
                "connecting to the terminal through the execution bridge",
            ),
        };
        rows.push(AccountRow {
            title: "MetaTrader 5".to_owned(),
            hint: hint.to_owned(),
            subs: vec![waiting.to_owned()],
            state: RowState::Off,
            tone: RouteTone::Locked,
            gesture: RowGesture::None,
            progress: None,
        });
        return (rows, footer(facts, Route::Real));
    };
    let kind = Route::of_mode(account.trade_mode);
    let available = AccountRoute::availability(facts, kind);
    let progress = route.hold_progress(now_ms);
    let selected = route.route() == kind;
    let state = match (selected, available.is_ok()) {
        (true, _) => RowState::Selected,
        (false, true) => RowState::Enabled,
        (false, false) => RowState::Off,
    };
    let hint = match (kind, selected, progress.is_some(), state) {
        (_, true, ..) => "in use",
        (_, _, _, RowState::Off) => "",
        (Route::Real, _, true, _) => "holding…",
        (Route::Real, ..) => "hold",
        _ => "one click",
    };
    let mut subs = vec![balance_line(account, kind)];
    if kind == Route::Real && (selected || progress.is_some()) {
        subs.extend(real_facts(venue, ticket));
    }
    rows.push(AccountRow {
        title: format!(
            "{} · {} · {}",
            account.company.split_whitespace().next().unwrap_or("MT5"),
            account.login,
            kind.word()
        ),
        hint: hint.to_owned(),
        subs,
        state,
        tone: tone_of(kind),
        gesture: match (kind, state) {
            (_, RowState::Off) => RowGesture::None,
            (Route::Real, _) => RowGesture::Hold,
            _ => RowGesture::Click(kind),
        },
        progress,
    });
    (rows, footer(facts, kind))
}

fn balance_line(account: &AccountFacts, kind: Route) -> String {
    let balance = account.balance.map_or_else(
        || "balance unknown".to_owned(),
        |balance| format!("balance {} {}", money_text(balance), account.currency),
    );
    if kind == Route::Real {
        format!("real money · {balance}")
    } else {
        balance
    }
}

/// What the REAL row adds while it is held or in use: the instrument and
/// size the next order takes, who else holds the symbol, and where the
/// protection lives.
fn real_facts(venue: Option<&dyn BrokerVenue>, ticket: &TicketFacts) -> Vec<String> {
    let mut instrument = ticket.symbol.clone();
    if let Some(value) = &ticket.point_value {
        instrument.push_str(&format!(" · {value}"));
    }
    if let Some(quantity) = ticket.quantity {
        instrument.push_str(&format!(" · qty {}", fmt_decimal(quantity)));
    }
    let own = venue.and_then(|venue| venue.position()).map_or_else(
        || "flat".to_owned(),
        |position| {
            format!(
                "{} {}",
                position_word(position.side),
                fmt_decimal(position.quantity)
            )
        },
    );
    let (positions, orders) = venue.map_or((0, 0), BrokerVenue::foreign_counts);
    let others = match (positions, orders) {
        (0, 0) => "none".to_owned(),
        (positions, 0) => plural(positions, "position"),
        (0, orders) => plural(orders, "order"),
        (positions, orders) => format!(
            "{}, {}",
            plural(positions, "position"),
            plural(orders, "order")
        ),
    };
    vec![
        instrument,
        format!("you: {own} · other robots: {others}"),
        "Stop and target rest on the server.".to_owned(),
    ]
}

/// Money as a broker statement writes it: two places, `12340.00`.
fn money_text(amount: Decimal) -> String {
    format!("{:.2}", amount.round_dp(2))
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("1 {word}")
    } else {
        format!("{count} {word}s")
    }
}

/// Why the broker row is off, said once under the list.
fn footer(facts: &super::RouteFacts, kind: Route) -> Option<String> {
    let refusal = AccountRoute::availability(facts, kind).err()?;
    let sentence = match refusal {
        Refusal::SourceCannotRoute => "This source does not route orders.".to_owned(),
        other => {
            let text = other.sentence();
            let mut chars = text.chars();
            chars.next().map_or_else(String::new, |first| {
                format!("{}{}.", first.to_uppercase(), chars.as_str())
            })
        }
    };
    Some(sentence)
}
