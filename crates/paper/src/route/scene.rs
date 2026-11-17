//! A staged route: what a capture or a demo asks the route to show, decided
//! here so the window's harness only reads three launch values and builds a
//! venue over a fake bridge.

use super::desk::{AccountsPort, BrokerDesk, SourceFacts};
use super::{Actor, HOLD_TO_ARM_MS, Route};

/// The route a run asked to be staged at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scene {
    /// The account list open.
    pub list: bool,
    /// The REAL row held at this fraction of its hold, standing still.
    pub hold: Option<f32>,
    /// The kind of account the staged terminal reports.
    pub account: Option<Route>,
    /// Arm that account, as the trader would.
    pub arm: bool,
    /// Once armed, drop the link.
    pub lost: bool,
    /// The chart's tape stands in for a live MetaTrader one.
    pub live: bool,
}

impl Scene {
    /// Read `demo|real|lost|sim|idle`, `1` and `<0..1>`; `None` when nothing
    /// is asked.
    #[must_use]
    pub fn parse(route: Option<&str>, list: Option<&str>, hold: Option<&str>) -> Option<Self> {
        let hold = hold.and_then(|text| text.trim().parse::<f32>().ok());
        let account = match route {
            Some("demo") => Some(Route::Demo),
            Some("real" | "lost" | "sim" | "idle") => Some(Route::Real),
            _ => hold.map(|_| Route::Real),
        };
        let scene = Self {
            list: list == Some("1"),
            hold,
            account,
            arm: matches!(route, Some("demo" | "real" | "lost")),
            lost: route == Some("lost"),
            live: route.is_some_and(|route| route != "idle") || hold.is_some(),
        };
        (scene.list || scene.account.is_some()).then_some(scene)
    }

    /// Whether the scene needs a broker venue staged behind it.
    #[must_use]
    pub fn needs_broker(&self) -> bool {
        self.account.is_some()
    }

    /// Whether the chart's tape stands in for a live MetaTrader one, so a
    /// capture over a recording can show a route it could arm at all. An
    /// `idle` scene does not: it shows what the chart's own source allows.
    #[must_use]
    pub fn stands_in_live(&self) -> bool {
        self.live
    }

    /// Whether the staged terminal reports a demo account.
    #[must_use]
    pub fn demo(&self) -> bool {
        self.account == Some(Route::Demo)
    }

    /// What a run's three launch values stage: the scene, and for a broker
    /// scene the connector `connect(demo, entry)` builds over a fake bridge
    /// whose position opens at the chart's `mark`, whole points. `None`
    /// until a broker scene has a mark to open at.
    pub fn stage(
        [route, list, hold]: [Option<&str>; 3],
        mark: Option<rust_decimal::Decimal>,
        connect: impl FnOnce(bool, rust_decimal::Decimal) -> Box<dyn quantick_trading::BrokerConnector>,
    ) -> Option<super::Staged> {
        let scene = Self::parse(route, list, hold);
        let Some(scene) = scene.filter(Self::needs_broker) else {
            return Some((scene, None));
        };
        let connector = connect(scene.demo(), mark?.round_dp(0));
        Some((Some(scene), Some(connector)))
    }
}

impl BrokerDesk {
    /// One frame of a staged scene. Answers `true` the frame its link should
    /// drop.
    pub fn stage(
        &mut self,
        port: &mut dyn AccountsPort,
        source: SourceFacts,
        scene: &Scene,
        now_ms: i64,
    ) -> bool {
        let drop_link = self.stage_route(port, source, scene, now_ms);
        // After arming, which closes the list as a completed hold does.
        if scene.list {
            self.sheet_open = true;
        }
        drop_link
    }

    fn stage_route(
        &mut self,
        port: &mut dyn AccountsPort,
        source: SourceFacts,
        scene: &Scene,
        now_ms: i64,
    ) -> bool {
        if let Some(progress) = scene.hold {
            // Restarted every frame at the same age: the sweep stands still
            // and the hold never completes.
            let held = (f64::from(progress.clamp(0.0, 0.99)) * HOLD_TO_ARM_MS as f64) as i64;
            let _ = self.hold_start(port, source, now_ms.saturating_sub(held));
            return false;
        }
        let Some(route) = scene.account.filter(|_| scene.arm) else {
            return false;
        };
        if self.route().route().is_broker() {
            return scene.lost;
        }
        if self.route().last_disarm().is_some() {
            return false;
        }
        if route == Route::Real {
            if self
                .hold_start(port, source, now_ms.saturating_sub(HOLD_TO_ARM_MS))
                .is_ok()
            {
                self.hold(port, source, now_ms, false);
            }
        } else {
            let _ = self.request(port, source, (route, Actor::Trader, None));
        }
        false
    }
}
