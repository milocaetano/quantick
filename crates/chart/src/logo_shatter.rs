//! The logo the connection overlay breaks apart and rebuilds while a chart has
//! no bars: the trader's mock, ported. A grid of grey cells sampled from the
//! logo's alpha cracks open from a seeded point near its centre, drifts and
//! falls inside the overlay, dims, and rebuilds bottom-up while the last
//! pieces are still landing, so the canvas is never blank: something is
//! always moving while the software works.
//!
//! Pure: the caller passes the time and its own frame counter, so the same
//! inputs give the same frame. Nothing here reads a clock or a random source;
//! the scatter comes from a seeded `SplitMix64`.

/// The logo's side, as a share of the overlay's shorter side.
const LOGO_SHARE: f32 = 0.42;
/// The logo's centre, as a share of the overlay's height.
const LOGO_CENTRE_Y: f32 = 0.42;
/// A cell is the logo's side over this, down to [`MIN_CELL_PX`].
const CELLS_PER_LOGO: f32 = 40.0;
/// The smallest cell, in points: under this the logo stops reading as pixels.
const MIN_CELL_PX: f32 = 4.0;
/// A cell the logo covers less than this is not drawn.
const MIN_COVERAGE: f32 = 0.25;
/// Every cell's alpha is scaled by this.
const GLOBAL_ALPHA: f32 = 0.7;
/// A cell's grey is `GREY_LOW + GREY_SPAN * shade`, its shade in
/// `SHADE_MIN..1`.
const GREY_LOW: f32 = 150.0;
const GREY_SPAN: f32 = 60.0;
const SHADE_MIN: f32 = 0.7;
/// The whole logo stands this long before it cracks.
const HOLD_S: f32 = 2.0;
/// The crack's start, within this share of the overlay around the logo's centre.
const CRACK_SPREAD: f32 = 0.1;
/// The crack reaches the farthest cell this long after it opens, the nearer
/// ones in proportion, so the flights out are spread and their landings too.
const CRACK_DELAY_S: f32 = 1.0;
const CRACK_JITTER_S: f32 = 0.3;
/// One cell's flight out, in seconds.
const SHATTER_EASE_S: f32 = 2.0;
/// The last flight out lands this long after the crack.
const SHATTER_S: f32 = CRACK_DELAY_S + CRACK_JITTER_S + SHATTER_EASE_S;
/// The rebuild starts this long before the last flight out lands.
const REBUILD_OVERLAP_S: f32 = 1.0;
const REBUILD_AT_S: f32 = SHATTER_S - REBUILD_OVERLAP_S;
/// The rebuild climbs from the bottom row: this much delay over the rows, plus
/// a jitter, then one cell's flight home. A cell still flying out when its row
/// is called starts home as it lands.
const REBUILD_SPREAD_S: f32 = 1.6;
const REBUILD_JITTER_S: f32 = 0.4;
const REBUILD_EASE_S: f32 = 1.8;
/// The rebuild's length: its last call plus one flight home.
const REBUILD_S: f32 = REBUILD_SPREAD_S + REBUILD_JITTER_S + REBUILD_EASE_S;
// Every flight out lands before the rebuild's last call, so the loop's end
// finds every cell home rather than snapping it there.
const _: () = assert!(REBUILD_OVERLAP_S <= REBUILD_SPREAD_S + REBUILD_JITTER_S);
/// One whole loop, in seconds: hold, crack to the rebuild, rebuild.
pub const CYCLE_S: f64 = (HOLD_S + REBUILD_AT_S + REBUILD_S) as f64;
/// How far a cell flies, as shares of the overlay's shorter side.
const FLIGHT_MIN: f32 = 0.25;
const FLIGHT_MAX: f32 = 0.6;
/// The sideways and vertical scatter of a target, as shares of the overlay.
const DRIFT_X: f32 = 0.08;
const DRIFT_UP: f32 = 0.05;
const DRIFT_DOWN: f32 = 0.12;
/// The slow fall under the flight, as a share of the overlay's height.
const FALL_SHARE: f32 = 0.08;
/// A cell turns up to this far, either way, in radians.
const SPIN_RAD: f32 = 1.5;
/// A flown cell's scale.
const SHRUNK: f32 = 0.7;
/// A landed cell's share of its alpha: it lies dim where it fell until its
/// row is called home, so the canvas between the two flights is never blank.
const LANDED_ALPHA: f32 = 0.25;
/// Targets keep this many cells from every edge.
const MARGIN_CELLS: f32 = 2.0;
/// A cell this faint is not drawn.
const INVISIBLE: f32 = 0.01;
/// Mixes the cycle into the seed so every loop scatters differently.
const CYCLE_MIX: u64 = 0xD1B5_4A32_D192_ED03;
/// The grid is cut again only when the overlay's size crosses a step this
/// big, so dragging a window does not re-cut and re-seed it every frame.
const RECUT_STEP_PX: f32 = 8.0;

/// Where a moment of the loop stands. Cells overlap the last two: the rebuild
/// calls the bottom rows home while the far cells are still landing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Hold,
    Shatter,
    Rebuild,
}

/// The loop `t_s` falls in, its phase, and the seconds into the loop.
#[must_use]
pub fn phase_at(t_s: f64) -> (u64, Phase, f32) {
    let t = t_s.max(0.0);
    let cycle = (t / CYCLE_S).floor();
    let local = (t - cycle * CYCLE_S) as f32;
    let phase = if local < HOLD_S {
        Phase::Hold
    } else if local < HOLD_S + REBUILD_AT_S {
        Phase::Shatter
    } else {
        Phase::Rebuild
    };
    (cycle as u64, phase, local)
}

/// A seed from a symbol: FNV-1a, so the same symbol always breaks the same way.
#[must_use]
pub fn symbol_seed(symbol: &str) -> u64 {
    symbol.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3)
    })
}

/// `SplitMix64`: small, seeded, and the same on every machine.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `low..high`.
    fn range(&mut self, low: f32, high: f32) -> f32 {
        let unit = (self.next() >> 40) as f32 / (1_u64 << 24) as f32;
        low + unit * (high - low)
    }
}

/// One cell of the grid: its home, its looks, and this loop's scatter.
#[derive(Debug, Clone, Copy, Default)]
struct Cell {
    home: [f32; 2],
    coverage: f32,
    /// Its row over the grid's rows, which orders the rebuild.
    row_share: f32,
    grey: u8,
    spin: f32,
    target: [f32; 2],
    delay: f32,
    rebuild_delay: f32,
}

/// One cell as drawn this frame, in the overlay's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellPose {
    pub center: [f32; 2],
    pub side: f32,
    pub rotation: f32,
    pub grey: u8,
    /// Final alpha, in `0.0..=1.0`.
    pub alpha: f32,
}

impl CellPose {
    /// The turned square's corners, clockwise from its top-left.
    #[must_use]
    pub fn corners(&self) -> [[f32; 2]; 4] {
        let (sin, cos) = self.rotation.sin_cos();
        let half = self.side / 2.0;
        [[-half, -half], [half, -half], [half, half], [-half, half]].map(|[x, y]| {
            [
                self.center[0] + x * cos - y * sin,
                self.center[1] + x * sin + y * cos,
            ]
        })
    }
}

/// The grid for one overlay size and seed, re-seeded at each loop's start.
#[derive(Debug, Clone)]
pub struct Shatter {
    width: f32,
    height: f32,
    cell: f32,
    seed: u64,
    cycle: Option<u64>,
    cells: Vec<Cell>,
}

impl Shatter {
    /// The logo's `side` by `side` alpha image laid on a `width` by `height`
    /// overlay, cut into cells, every speck left out.
    #[must_use]
    pub fn new(width: f32, height: f32, alpha: &[u8], side: usize, seed: u64) -> Self {
        let size = width.min(height) * LOGO_SHARE;
        let cell = (size / CELLS_PER_LOGO).round().max(MIN_CELL_PX);
        let left = (width / 2.0 - size / 2.0).round();
        let top = (height * LOGO_CENTRE_Y - size / 2.0).round();
        let rows = if size > 0.0 {
            (size / cell).ceil() as usize
        } else {
            0
        };
        let mut looks = SplitMix64(seed);
        let mut cells = Vec::new();
        for row in 0..rows {
            for column in 0..rows {
                let coverage = coverage(alpha, side, side as f32 / size, cell, column, row);
                if coverage < MIN_COVERAGE {
                    continue;
                }
                let shade = looks.range(SHADE_MIN, 1.0);
                cells.push(Cell {
                    home: [left + column as f32 * cell, top + row as f32 * cell],
                    coverage,
                    row_share: row as f32 / rows as f32,
                    grey: (GREY_LOW + GREY_SPAN * shade).round() as u8,
                    spin: looks.range(-SPIN_RAD, SPIN_RAD),
                    ..Cell::default()
                });
            }
        }
        Self {
            width,
            height,
            cell,
            seed,
            cycle: None,
            cells,
        }
    }

    /// How many cells the logo is.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The side of one cell, in points.
    #[must_use]
    pub fn cell_px(&self) -> f32 {
        self.cell
    }

    /// Every visible cell `t_s` seconds after the overlay came up. At `0.0`
    /// this is the whole logo at rest, which is also what reduced motion draws.
    pub fn poses(&mut self, t_s: f64, out: &mut Vec<CellPose>) {
        let (cycle, phase, local) = phase_at(t_s);
        self.seed_cycle(cycle);
        out.clear();
        let since_crack = local - HOLD_S;
        let fall = self.height * FALL_SHARE;
        for cell in &self.cells {
            let [home_x, home_y] = cell.home;
            let [target_x, target_y] = cell.target;
            let landed = cell.delay + SHATTER_EASE_S;
            let (at, alpha, scale, rotation) = if phase == Phase::Hold {
                (cell.home, cell.coverage, 1.0, 0.0)
            } else if since_crack < landed {
                let p = progress(since_crack - cell.delay, SHATTER_EASE_S);
                let eased = 1.0 - (1.0 - p).powi(3);
                let x = home_x + (target_x - home_x) * eased;
                let y = home_y + (target_y - home_y) * eased + p * p * fall;
                let scale = 1.0 - p * (1.0 - SHRUNK);
                let alpha = 1.0 - p * p * (1.0 - LANDED_ALPHA);
                ([x, y], cell.coverage * alpha, scale, cell.spin * p)
            } else {
                let called = landed.max(REBUILD_AT_S + cell.rebuild_delay);
                let p = progress(since_crack - called, REBUILD_EASE_S);
                let eased = if p < 0.5 {
                    2.0 * p * p
                } else {
                    1.0 - (-2.0 * p + 2.0).powi(2) / 2.0
                };
                let fallen = target_y + fall;
                let x = target_x + (home_x - target_x) * eased;
                let y = fallen + (home_y - fallen) * eased;
                let scale = SHRUNK + (1.0 - SHRUNK) * p;
                let alpha = LANDED_ALPHA + (1.0 - LANDED_ALPHA) * p;
                ([x, y], cell.coverage * alpha, scale, cell.spin * (1.0 - p))
            };
            if alpha > INVISIBLE {
                out.push(self.pose(cell, at, alpha, scale, rotation));
            }
        }
    }

    fn pose(&self, cell: &Cell, at: [f32; 2], alpha: f32, scale: f32, rotation: f32) -> CellPose {
        let half = self.cell / 2.0;
        CellPose {
            center: [at[0] + half, at[1] + half],
            side: self.cell * scale - 1.0,
            rotation,
            grey: cell.grey,
            alpha: (alpha * GLOBAL_ALPHA).min(1.0),
        }
    }

    /// This loop's crack, delays and targets, drawn once per loop.
    fn seed_cycle(&mut self, cycle: u64) {
        if self.cycle == Some(cycle) {
            return;
        }
        self.cycle = Some(cycle);
        let mut rng = SplitMix64(self.seed ^ cycle.wrapping_add(1).wrapping_mul(CYCLE_MIX));
        let (width, height) = (self.width, self.height);
        let short = width.min(height);
        let crack_x = width / 2.0 + rng.range(-width * CRACK_SPREAD, width * CRACK_SPREAD);
        let crack_y =
            height * LOGO_CENTRE_Y + rng.range(-height * CRACK_SPREAD, height * CRACK_SPREAD);
        let farthest = self.cells.iter().fold(1.0_f32, |far, cell| {
            far.max((cell.home[0] - crack_x).hypot(cell.home[1] - crack_y))
        });
        let margin = self.cell * MARGIN_CELLS;
        let fall = height * FALL_SHARE;
        let right = width - margin - self.cell;
        let bottom = height - margin - self.cell - fall;
        for cell in &mut self.cells {
            let (dx, dy) = (cell.home[0] - crack_x, cell.home[1] - crack_y);
            let distance = match dx.hypot(dy) {
                d if d > 0.0 => d,
                _ => 1.0,
            };
            cell.delay = distance / farthest * CRACK_DELAY_S + rng.range(0.0, CRACK_JITTER_S);
            let flight = rng.range(short * FLIGHT_MIN, short * FLIGHT_MAX);
            let x = cell.home[0] + dx / distance * flight;
            let x = x + rng.range(-width * DRIFT_X, width * DRIFT_X);
            let y = cell.home[1] + dy / distance * flight;
            let y = y + rng.range(-height * DRIFT_UP, height * DRIFT_DOWN);
            cell.target = [x.min(right).max(margin), y.min(bottom).max(margin)];
            cell.rebuild_delay =
                (1.0 - cell.row_share) * REBUILD_SPREAD_S + rng.range(0.0, REBUILD_JITTER_S);
        }
    }
}

/// The logo across one overlay from frame to frame: cut again when the
/// overlay's size crosses a step or its seed changes, started whole again
/// when the caller's frame counter shows it was not drawn last frame.
#[derive(Debug, Clone, Default)]
pub struct LogoRun {
    shatter: Option<(Shatter, (u32, u32, u64))>,
    started_ms: i64,
    last_pass: Option<u64>,
    /// The loop whose hold is already in `poses`: a hold never changes.
    held: Option<u64>,
    poses: Vec<CellPose>,
}

impl LogoRun {
    /// This frame's cells over a `size` overlay at `now_ms`, the logo given as
    /// its alpha image and side, `pass` the caller's frame counter. `still`
    /// holds the whole logo, for reduced motion. Answers the cells and the
    /// seconds until they next change: the rest of a hold, `0.0` while they
    /// move, `None` when they never will.
    pub fn frame(
        &mut self,
        size: [f32; 2],
        seed: u64,
        logo: (&[u8], usize),
        now_ms: i64,
        pass: u64,
        still: bool,
    ) -> (&[CellPose], Option<f64>) {
        let key = (
            (size[0] / RECUT_STEP_PX) as u32,
            (size[1] / RECUT_STEP_PX) as u32,
            seed,
        );
        if self.shatter.as_ref().is_none_or(|(_, at)| *at != key) {
            let shatter = Shatter::new(size[0], size[1], logo.0, logo.1, seed);
            (self.shatter, self.held) = (Some((shatter, key)), None);
        }
        if self
            .last_pass
            .is_none_or(|last| pass != last && pass != last.wrapping_add(1))
        {
            (self.started_ms, self.held) = (now_ms, None);
        }
        self.last_pass = Some(pass);
        let Some((shatter, _)) = &mut self.shatter else {
            return (&[], None);
        };
        let seconds = if still {
            0.0
        } else {
            now_ms.saturating_sub(self.started_ms) as f64 / 1_000.0
        };
        let (cycle, phase, local) = phase_at(seconds);
        let hold = phase == Phase::Hold;
        if !(hold && self.held == Some(cycle)) {
            shatter.poses(seconds, &mut self.poses);
        }
        self.held = hold.then_some(cycle);
        let next = match (still, hold) {
            (true, _) => None,
            (false, true) => Some(f64::from(HOLD_S - local)),
            (false, false) => Some(0.0),
        };
        (&self.poses, next)
    }
}

/// `elapsed` over `span`, held to `0.0..=1.0`.
fn progress(elapsed: f32, span: f32) -> f32 {
    (elapsed / span).clamp(0.0, 1.0)
}

/// The mean alpha under one cell, where `scale` is image pixels per point.
/// What lies past the image counts as clear, as the mock's canvas did.
fn coverage(alpha: &[u8], side: usize, scale: f32, cell: f32, column: usize, row: usize) -> f32 {
    if side == 0 || alpha.len() < side * side || !scale.is_finite() {
        return 0.0;
    }
    let span = |index: usize| {
        let start = (index as f32 * cell * scale) as usize;
        let end = (((index + 1) as f32 * cell * scale) as usize).max(start + 1);
        start..end
    };
    let (columns, rows) = (span(column), span(row));
    let area = (columns.len() * rows.len()) as f32;
    let mut sum = 0_u64;
    for y in rows.start..rows.end.min(side) {
        for x in columns.start..columns.end.min(side) {
            sum += u64::from(alpha[y * side + x]);
        }
    }
    sum as f32 / (area * 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A disc on a clear square: a logo with no plate.
    fn disc(side: usize) -> Vec<u8> {
        let centre = side as f32 / 2.0;
        (0..side * side)
            .map(|index| {
                let (x, y) = ((index % side) as f32 + 0.5, (index / side) as f32 + 0.5);
                if (x - centre).hypot(y - centre) < centre * 0.8 {
                    255
                } else {
                    0
                }
            })
            .collect()
    }

    fn grid(width: f32, height: f32) -> Shatter {
        Shatter::new(width, height, &disc(64), 64, symbol_seed("DOGEUSDT"))
    }

    fn at_rest(shatter: &mut Shatter) -> Vec<CellPose> {
        let mut poses = Vec::new();
        shatter.poses(0.0, &mut poses);
        poses
    }

    #[test]
    fn the_cell_count_is_stable_for_a_fixed_grid() {
        let mut one = grid(900.0, 500.0);
        let two = grid(900.0, 500.0);
        assert_eq!(one.len(), two.len());
        // 500 * 0.42 = 210 points, a 5-point cell, a 42 by 42 grid: the disc
        // covers about half of it and no plate fills the corners.
        assert_eq!(one.cell_px(), 5.0);
        assert!((600..1_400).contains(&one.len()), "{}", one.len());
        let mut poses = Vec::new();
        for cycle in 0..4 {
            one.poses(f64::from(cycle) * CYCLE_S, &mut poses);
            assert_eq!(poses.len(), two.len(), "every loop starts whole");
        }
        assert_eq!(grid(40.0, 30.0).cell_px(), MIN_CELL_PX);
    }

    /// Each beat is as long as its own delays and flights: 2.0 s hold, the
    /// crack's 3.3 s less a 1.0 s overlap, then a 3.8 s rebuild.
    #[test]
    fn phase_boundaries_follow_the_loop() {
        assert!((CYCLE_S - 8.1).abs() < 1e-5, "{CYCLE_S}");
        let at = |t: f64| {
            let (cycle, phase, _) = phase_at(t);
            (cycle, phase)
        };
        assert_eq!(at(0.0), (0, Phase::Hold));
        assert_eq!(at(1.99), (0, Phase::Hold));
        assert_eq!(at(2.01), (0, Phase::Shatter));
        assert_eq!(at(4.29), (0, Phase::Shatter));
        assert_eq!(at(4.31), (0, Phase::Rebuild));
        assert_eq!(at(8.09), (0, Phase::Rebuild));
        assert_eq!(at(8.11), (1, Phase::Hold));
        assert_eq!(at(-5.0), (0, Phase::Hold), "before the start is the start");
    }

    /// The least share of the resting logo's ink a frame may show.
    const MIN_INK: f32 = 0.2;

    /// No blank beat: from the crack to the loop's end a good part of the
    /// logo's ink is always on the canvas, flying out or flying home.
    #[test]
    fn the_canvas_is_never_blank() {
        for (width, height) in [(900.0, 500.0), (270.0, 600.0), (1_600.0, 260.0)] {
            let mut shatter = grid(width, height);
            let ink = |poses: &[CellPose]| poses.iter().map(|pose| pose.alpha).sum::<f32>();
            let whole = ink(&at_rest(&mut shatter));
            let mut poses = Vec::new();
            for step in 0..(2.0 * CYCLE_S / 0.02) as usize {
                let t = step as f64 * 0.02;
                shatter.poses(t, &mut poses);
                let share = ink(&poses) / whole;
                assert!(share > MIN_INK, "{width}x{height} at {t}: {share}");
            }
        }
    }

    /// The loop's end finds every cell home, so the next hold does not snap
    /// anything: no flight is cut off by a boundary.
    #[test]
    fn the_loop_ends_with_every_cell_home() {
        let mut shatter = grid(900.0, 500.0);
        let rest = at_rest(&mut shatter);
        for cycle in 0..3 {
            let mut end = Vec::new();
            shatter.poses((f64::from(cycle) + 1.0) * CYCLE_S - 1e-4, &mut end);
            assert_eq!(end.len(), rest.len(), "loop {cycle}");
            for (end, rest) in end.iter().zip(&rest) {
                assert!((end.center[0] - rest.center[0]).abs() < 0.5);
                assert!((end.center[1] - rest.center[1]).abs() < 0.5);
                assert!((end.alpha - rest.alpha).abs() < 0.01);
                assert!(end.rotation.abs() < 0.01);
            }
        }
    }

    /// At rest, which reduced motion draws: every cell home, square, unturned.
    #[test]
    fn reduced_motion_is_the_home_grid() {
        let mut shatter = grid(900.0, 500.0);
        let home = at_rest(&mut shatter);
        let mut held = Vec::new();
        shatter.poses(1.0, &mut held);
        assert_eq!(home, held);
        assert_eq!(home.len(), shatter.len());
        let cell = shatter.cell_px();
        for pose in &home {
            assert_eq!(pose.rotation, 0.0);
            assert_eq!(pose.side, cell - 1.0);
            assert!((150..=210).contains(&pose.grey));
            assert!(pose.alpha > 0.0 && pose.alpha <= GLOBAL_ALPHA);
        }
    }

    /// Every corner of every cell stays inside the overlay at every sampled
    /// moment of two loops, wide and narrow.
    #[test]
    fn every_cell_stays_inside_the_canvas() {
        for (width, height) in [(900.0, 500.0), (270.0, 600.0), (1_600.0, 260.0)] {
            let mut shatter = grid(width, height);
            let mut poses = Vec::new();
            for step in 0..(2.0 * CYCLE_S / 0.05) as usize {
                shatter.poses(step as f64 * 0.05, &mut poses);
                for pose in &poses {
                    for [x, y] in pose.corners() {
                        assert!(
                            (0.0..=width).contains(&x) && (0.0..=height).contains(&y),
                            "{width}x{height} at {}: {x},{y}",
                            step as f64 * 0.05
                        );
                    }
                }
            }
        }
    }

    /// Same seed, same frame; another symbol breaks another way; each loop
    /// re-seeds.
    #[test]
    fn the_scatter_is_seeded() {
        let frame = |seed: u64, t: f64| {
            let mut shatter = Shatter::new(900.0, 500.0, &disc(64), 64, seed);
            let mut poses = Vec::new();
            shatter.poses(t, &mut poses);
            poses
        };
        let doge = symbol_seed("DOGEUSDT");
        assert_eq!(frame(doge, 3.5), frame(doge, 3.5));
        assert_ne!(frame(doge, 3.5), frame(symbol_seed("BTCUSDT"), 3.5));
        assert_ne!(frame(doge, 3.5), frame(doge, 3.5 + CYCLE_S));
    }

    #[test]
    fn mid_shatter_cells_fly_turn_shrink_and_fade() {
        let mut shatter = grid(900.0, 500.0);
        let home = at_rest(&mut shatter);
        let mut flying = Vec::new();
        shatter.poses(f64::from(HOLD_S) + 1.5, &mut flying);
        assert!(!flying.is_empty() && flying.len() <= home.len());
        assert!(flying.iter().any(|pose| pose.rotation != 0.0));
        assert!(
            flying
                .iter()
                .any(|pose| pose.side < shatter.cell_px() - 1.0)
        );
        let faded = flying.iter().map(|pose| pose.alpha).sum::<f32>();
        assert!(faded < home.iter().map(|pose| pose.alpha).sum::<f32>());
    }

    /// The run keeps its clock while drawn on consecutive frames, restarts
    /// whole when a frame went by without it, asks for a frame only when the
    /// picture changes, and re-cuts on a resize past a step.
    #[test]
    fn a_run_restarts_whole_and_repaints_only_on_change() {
        let logo = disc(64);
        let mut run = LogoRun::default();
        let size = [900.0, 500.0];
        let (cells, next) = run.frame(size, 7, (&logo, 64), 10_000, 1, false);
        let whole = cells.len();
        assert_eq!(next, Some(2.0), "the hold's whole length");
        let (_, next) = run.frame(size, 7, (&logo, 64), 11_500, 2, false);
        assert_eq!(next, Some(0.5), "the hold's rest");
        let (_, next) = run.frame(size, 7, (&logo, 64), 13_000, 3, false);
        assert_eq!(next, Some(0.0), "moving: every frame");
        let mut still = run.clone();
        let (cells, next) = still.frame(size, 7, (&logo, 64), 13_016, 4, true);
        assert_eq!((cells.len(), next), (whole, None));
        // A frame without the overlay: it comes back whole, at the hold.
        let (cells, next) = run.frame(size, 7, (&logo, 64), 13_032, 9, false);
        assert_eq!((cells.len(), next), (whole, Some(2.0)));
        // A drag inside one step keeps the grid; past it, it is cut again.
        let (_, next) = run.frame([903.0, 501.0], 7, (&logo, 64), 13_048, 10, false);
        assert!(next.is_some_and(|left| left < 2.0), "same run, same clock");
        let narrow = run.frame([270.0, 600.0], 7, (&logo, 64), 13_064, 11, false);
        assert!(narrow.0.len() < whole, "a smaller logo is fewer cells");
    }

    #[test]
    fn a_short_image_is_no_logo_rather_than_a_panic() {
        assert!(Shatter::new(900.0, 500.0, &[0; 3], 4, 1).is_empty());
        assert!(Shatter::new(900.0, 500.0, &[], 0, 1).is_empty());
        assert!(Shatter::new(0.0, 0.0, &disc(8), 8, 1).is_empty());
    }
}
