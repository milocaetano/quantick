//! The aim, seen from the panes it was not made in.
//!
//! One desk serves every pane of a split, so the price a held key aims at in
//! one pane is a level every other pane can show. The pane the aim was made
//! in paints the whole aim (line, label, chip and bracket). Every other pane
//! paints an echo: a faint dashed line at the aimed price on its *own* axis,
//! so the trader reads the level against tick, renko and minute bars at
//! once. The echo is a picture and nothing else. It has no label, no chip
//! and no click, because the click belongs to the pane the aim was made in.

use quantick_engine::Side;
use rust_decimal::prelude::ToPrimitive;

use super::Desk;
use super::cmd::CmdPreview;
use super::geometry::{Bounds, PriceAxis};

/// What one pane paints of this frame's aim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AimPaint<'a> {
    /// The pane the aim was made in: line, label, chip and bracket.
    Full(&'a CmdPreview),
    /// Any other pane: a dashed line at `y`, the aimed price mapped
    /// through that pane's own axis.
    Echo { side: Side, y: f32 },
}

impl Desk {
    /// What a pane paints of the aim this frame, or `None` for nothing.
    ///
    /// `band` is the pane's interactive plot, `aimed_here` whether this pane
    /// ran this frame's paper input and `axis` the pane's own price axis.
    ///
    /// The host runs the input pane before it paints any pane, so the aim
    /// is always this frame's and was always made in the pane that ran the
    /// input. That pane is asked by identity, never by where the pointer is
    /// now: a toast or menu over the band hides the pointer from the pane
    /// without making it someone else's aim. It paints the whole aim, or
    /// nothing when the aim's pointer lies outside `band` (a forced aim
    /// parked past the tape-lane divider), and never an echo of itself.
    #[must_use]
    pub fn aim_paint(
        &self,
        band: Bounds,
        aimed_here: bool,
        axis: &dyn PriceAxis,
    ) -> Option<AimPaint<'_>> {
        let preview = self.gesture.cmd_preview.as_ref()?;
        if aimed_here {
            return band
                .contains(preview.pointer)
                .then_some(AimPaint::Full(preview));
        }
        let y = axis.y(preview.price.to_f64().unwrap_or_default());
        (band.top() <= y && y <= band.bottom()).then_some(AimPaint::Echo {
            side: preview.side,
            y,
        })
    }
}

#[cfg(test)]
mod tests {
    use quantick_engine::{Side, Trade};
    use rust_decimal::Decimal;

    use super::AimPaint;
    use crate::PaperAccount;
    use crate::desk::Desk;
    use crate::desk::cmd::{CmdPreviewForce, HeldKeys};
    use crate::desk::geometry::{Bounds, Point, PriceAxis};
    use crate::desk::gesture::ChartFrame;
    use crate::scratch::ScratchDir;

    /// A price axis over pixels `0..400` from `high` at the top to `low`
    /// at the bottom: two panes of a split, each with its own range.
    struct Axis {
        low: f64,
        high: f64,
    }

    impl PriceAxis for Axis {
        fn y(&self, price: f64) -> f32 {
            ((self.high - price) / (self.high - self.low) * 400.0) as f32
        }

        fn price_at(&self, y: f32) -> f64 {
            self.high - f64::from(y) / 400.0 * (self.high - self.low)
        }

        fn is_inverted(&self) -> bool {
            false
        }
    }

    /// The pane the aim is made in: 80..120 over the left half.
    const AIMED: Axis = Axis {
        low: 80.0,
        high: 120.0,
    };
    /// The other pane: 90..110 over the right half, so 95 sits lower.
    const OTHER: Axis = Axis {
        low: 90.0,
        high: 110.0,
    };

    fn aimed_band() -> Bounds {
        Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(400.0, 400.0))
    }

    fn other_band() -> Bounds {
        Bounds::from_min_max(Point::new(410.0, 0.0), Point::new(810.0, 400.0))
    }

    const SHIFT: HeldKeys = HeldKeys {
        shift: true,
        command: false,
        alt: false,
    };

    fn marked_account(dir: &ScratchDir) -> PaperAccount {
        let mut account = PaperAccount::with_trades_dir(dir.path().to_path_buf());
        account.set_symbol("ECHOX");
        account.seed(&Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: Decimal::from(100),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        account
    }

    /// One input frame on the aimed pane, the pointer at `pointer`.
    fn frame(pointer: Option<Point>, keys: HeldKeys) -> ChartFrame {
        ChartFrame {
            chart: aimed_band(),
            pointer,
            primary_pressed: false,
            primary_down: false,
            primary_released: false,
            keys,
            canvas_claimed: false,
            scroll_y: 0.0,
            middle_pressed: false,
            layer_visible: true,
        }
    }

    /// Shift over the aimed pane at y 250 aims a buy at 95.
    fn aiming_desk(account: &PaperAccount) -> Desk {
        let mut desk = Desk::default();
        desk.handle_input(
            account,
            &frame(Some(Point::new(200.0, 250.0)), SHIFT),
            Some(&AIMED),
        );
        assert_eq!(
            desk.gesture.cmd_preview.map(|preview| preview.price),
            Some(Decimal::from(95)),
            "Shift over the chart aims at 95"
        );
        desk
    }

    /// The pane the aim was made in paints the whole aim; every other pane
    /// paints the echo at the aimed price on its own axis.
    #[test]
    fn the_other_pane_echoes_the_aim_at_its_own_y() {
        let dir = ScratchDir::new("echo-on");
        let account = marked_account(&dir);
        let desk = aiming_desk(&account);

        assert!(matches!(
            desk.aim_paint(aimed_band(), true, &AIMED),
            Some(AimPaint::Full(preview)) if preview.side == Side::Buy
        ));
        assert_eq!(
            desk.aim_paint(other_band(), false, &OTHER),
            Some(AimPaint::Echo {
                side: Side::Buy,
                y: OTHER.y(95.0),
            }),
            "95 on the other pane's own axis, not the aimed pane's pixel"
        );
        assert_ne!(OTHER.y(95.0), AIMED.y(95.0), "the panes disagree on y");
    }

    /// No aim, no echo; and a price off the other pane's range paints
    /// nothing there rather than pinning a line to its edge.
    #[test]
    fn no_aim_or_an_off_range_price_paints_no_echo() {
        let dir = ScratchDir::new("echo-off");
        let account = marked_account(&dir);
        let mut desk = Desk::default();
        desk.handle_input(
            &account,
            &frame(Some(Point::new(200.0, 250.0)), HeldKeys::default()),
            Some(&AIMED),
        );
        assert_eq!(desk.gesture.cmd_preview, None, "no key, no aim");
        assert_eq!(desk.aim_paint(other_band(), false, &OTHER), None);

        let desk = aiming_desk(&account);
        let far = Axis {
            low: 200.0,
            high: 210.0,
        };
        assert_eq!(
            desk.aim_paint(other_band(), false, &far),
            None,
            "95 is off a 200..210 pane"
        );
    }

    /// The input pane runs first, so the frame the aim ends or moves, for
    /// whatever reason, the echo ends or moves with it.
    #[test]
    fn the_echo_follows_this_frames_aim() {
        let dir = ScratchDir::new("echo-follow");
        let account = marked_account(&dir);
        let mut desk = aiming_desk(&account);

        desk.handle_input(
            &account,
            &frame(Some(Point::new(200.0, 300.0)), SHIFT),
            Some(&AIMED),
        );
        assert_eq!(
            desk.aim_paint(other_band(), false, &OTHER),
            Some(AimPaint::Echo {
                side: Side::Buy,
                y: OTHER.y(90.0),
            }),
            "the aim moved to 90; the echo moved with it"
        );

        let both = HeldKeys {
            shift: true,
            alt: true,
            command: false,
        };
        for (why, keys) in [
            ("the key is up", HeldKeys::default()),
            ("both keys aim nothing", both),
        ] {
            let mut desk = aiming_desk(&account);
            desk.handle_input(
                &account,
                &frame(Some(Point::new(200.0, 250.0)), keys),
                Some(&AIMED),
            );
            assert_eq!(desk.aim_paint(other_band(), false, &OTHER), None, "{why}");
        }

        let mut desk = aiming_desk(&account);
        let mut claimed = frame(Some(Point::new(200.0, 250.0)), SHIFT);
        claimed.canvas_claimed = true;
        desk.handle_input(&account, &claimed, Some(&AIMED));
        assert_eq!(
            desk.aim_paint(other_band(), false, &OTHER),
            None,
            "a drawing handle or the pane chrome took the hand"
        );
    }

    /// The pane the aim was made in never echoes it. A toast or menu over
    /// the band hides the pointer from the pane, but the pane still ran the
    /// input, so it paints the whole aim; and a forced aim parked past the
    /// tape-lane divider paints nothing there rather than an echo.
    #[test]
    fn the_aiming_pane_never_echoes_its_own_aim() {
        let dir = ScratchDir::new("echo-self");
        let account = marked_account(&dir);
        let desk = aiming_desk(&account);
        assert!(
            matches!(
                desk.aim_paint(aimed_band(), true, &AIMED),
                Some(AimPaint::Full(_))
            ),
            "under an overlay, the aiming pane still owns its aim"
        );

        let mut desk = Desk::default();
        desk.gesture.cmd_preview_force = CmdPreviewForce::parse("buy@0.9");
        desk.handle_input(
            &account,
            &frame(Some(Point::new(200.0, 250.0)), HeldKeys::default()),
            Some(&AIMED),
        );
        let preview = desk.gesture.cmd_preview.expect("the hook forces an aim");
        assert_eq!(preview.pointer.x, 360.0);
        let left_of_divider = Bounds::from_min_max(Point::new(0.0, 0.0), Point::new(300.0, 400.0));
        assert_eq!(
            desk.aim_paint(left_of_divider, true, &AIMED),
            None,
            "parked past the divider: no aim here, and no echo of it either"
        );
    }

    /// The capture hook has no hand and no key: the input pane paints the
    /// whole aim, and every other band echoes it.
    #[test]
    fn a_forced_aim_paints_whole_where_made_and_echoes_elsewhere() {
        let dir = ScratchDir::new("echo-forced");
        let account = marked_account(&dir);
        let mut desk = Desk::default();
        desk.gesture.cmd_preview_force = CmdPreviewForce::parse("buy");
        desk.handle_input(
            &account,
            &frame(Some(Point::new(200.0, 250.0)), HeldKeys::default()),
            Some(&AIMED),
        );
        let preview = desk.gesture.cmd_preview.expect("the hook forces an aim");
        assert!(preview.forced);

        assert!(matches!(
            desk.aim_paint(aimed_band(), true, &AIMED),
            Some(AimPaint::Full(_))
        ));
        assert_eq!(
            desk.aim_paint(other_band(), false, &OTHER),
            Some(AimPaint::Echo {
                side: Side::Buy,
                y: OTHER.y(95.0),
            })
        );
    }
}
