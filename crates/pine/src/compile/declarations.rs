//! Pass 1: declaration collection.
//!
//! The `indicator()` header, the fixed top-level `plot*` registry and the
//! `fill()` resolution that pairs two of those plots. This pass runs over the
//! statement list before any name resolves, so everything it reads has to
//! fold to a constant — what does not is reported here and defaulted, never
//! silently dropped.

use quantick_indicators::{
    FillSpec, MarkerLocation, MarkerShape, MarkerSpec, PlotId, PlotSpec, PlotStyle, Rgba8,
};

use crate::ast::{Arg, NodeId, NodeKind};
use crate::builtins::Builtin;
use crate::error::{ErrorCode, PineError};

use super::fold::Const;
use super::{Compiler, PendingPlot, Resolution};

/// Default plot stroke width when a `plot()` gives none.
const DEFAULT_PLOT_WIDTH: f32 = 1.5;
/// Default plot color when a `plot()` gives none (egui-agnostic amber).
const DEFAULT_PLOT_COLOR: Rgba8 = Rgba8::opaque(255, 179, 0);
/// Default fill color when `fill()` gives none: translucent amber.
const DEFAULT_FILL_COLOR: Rgba8 = Rgba8::new(255, 179, 0, 40);

fn style_of_tag(tag: &str) -> PlotStyle {
    match tag {
        "style_stepline" => PlotStyle::StepLine,
        "style_histogram" => PlotStyle::Histogram,
        "style_columns" => PlotStyle::Columns,
        "style_circles" => PlotStyle::Circles,
        "style_cross" => PlotStyle::Cross,
        "style_area" => PlotStyle::Area,
        _ => PlotStyle::Line,
    }
}

impl Compiler {
    /// Scan the whole arena for `:=` targets before anything folds.
    pub(super) fn collect_reassigned_names(&mut self) {
        self.reassigned_names = self
            .ast
            .all()
            .filter_map(|node| match &node.kind {
                NodeKind::Reassign { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
    }

    pub(super) fn collect_declarations(&mut self) {
        let statements = match self.kind(self.root) {
            NodeKind::Script { statements } => statements.clone(),
            _ => return,
        };
        for statement in statements {
            let expr = match self.kind(statement) {
                NodeKind::ExprStmt { expr } => *expr,
                // `a = plot(...)`: the handle a fill() will reference — the
                // plot registers exactly like a bare plot statement.
                NodeKind::Declare { value, .. } => *value,
                _ => continue,
            };
            let NodeKind::Call { callee, args } = self.kind(expr).clone() else {
                continue;
            };
            let Some(name) = self.callee_name(callee) else {
                continue;
            };
            match Builtin::lookup(&name) {
                Some(Builtin::Indicator) => self.indicator_header(expr, &args),
                // Registered after resolution: `plot(close, color=c)` can
                // only fold `c` once names resolve, and folding it here
                // silently substituted the default for every argument that
                // came from a variable. Queued in textual order, which is
                // the plot order the whole system indexes by.
                Some(Builtin::Plot) => {
                    self.pending_plots
                        .push((expr, args.clone(), PendingPlot::Plot));
                }
                Some(Builtin::Hline) => {
                    self.pending_plots
                        .push((expr, args.clone(), PendingPlot::Hline));
                }
                Some(Builtin::PlotShape) => {
                    self.pending_plots.push((
                        expr,
                        args.clone(),
                        PendingPlot::Shape { is_char: false },
                    ));
                }
                Some(Builtin::PlotChar) => {
                    self.pending_plots.push((
                        expr,
                        args.clone(),
                        PendingPlot::Shape { is_char: true },
                    ));
                }
                Some(Builtin::Fill) => self.pending_fills.push((expr, args.clone())),
                // `barcolor` is live: it writes the bar paint channel at eval
                // time and needs nothing registered here. `bgcolor` still has
                // nowhere to land — the canvas behind the bars is not an
                // indicator output — so it stays accepted and inert, and says
                // so rather than looking like it worked.
                Some(Builtin::Bgcolor) => {
                    let span = self.span(expr);
                    self.warnings.push(PineError::new(
                        ErrorCode::PineUnsupported,
                        span,
                        "bgcolor() is accepted but not drawn (barcolor() paints the candles)",
                    ));
                }
                Some(Builtin::AlertCondition) => {
                    let span = self.span(expr);
                    self.warnings.push(PineError::new(
                        ErrorCode::PineUnsupported,
                        span,
                        "alertcondition() is accepted but inert (no alerts here)",
                    ));
                }
                _ => {}
            }
        }
    }

    fn indicator_header(&mut self, call: NodeId, args: &[Arg]) {
        const KNOWN: &[&str] = &["title", "shorttitle", "overlay", "precision"];
        if let Some(arg) = self.arg(args, 0, "title")
            && let Some(Const::Str(title)) = self.fold(arg.value)
        {
            self.title = title;
        }
        if let Some(arg) = self.arg(args, 1, "shorttitle")
            && let Some(Const::Str(short)) = self.fold(arg.value)
        {
            self.short_title = Some(short);
        }
        if let Some(arg) = args.iter().find(|a| a.name.as_deref() == Some("overlay"))
            && let Some(Const::Bool(overlay)) = self.fold(arg.value)
        {
            self.overlay = overlay;
        }
        for arg in args {
            if let Some(name) = arg.name.as_deref()
                && !KNOWN.contains(&name)
            {
                self.warnings.push(PineError::new(
                    ErrorCode::PineUnsupported,
                    arg.span,
                    format!("indicator(... {name}=...) is accepted but ignored"),
                ));
            }
        }
        // `precision` parses and is ignored, with the honest warning.
        if let Some(arg) = args.iter().find(|a| a.name.as_deref() == Some("precision")) {
            self.warnings.push(PineError::new(
                ErrorCode::PineUnsupported,
                arg.span,
                "indicator(precision=...) is accepted but ignored",
            ));
        }
        let _ = call;
    }

    /// Register every queued plot, now that names resolve.
    pub(super) fn register_pending_plots(&mut self) {
        for (call, args, kind) in std::mem::take(&mut self.pending_plots) {
            match kind {
                PendingPlot::Plot => self.plot_call(call, &args, false),
                PendingPlot::Hline => self.plot_call(call, &args, true),
                PendingPlot::Shape { is_char } => self.shape_call(call, &args, is_char),
            }
        }
    }

    /// Say so when an argument the author supplied could not be folded.
    ///
    /// A plot carries one constant look, so `color = up ? green : red` (or
    /// any value that is only known per bar) cannot be honoured. Substituting
    /// the default in silence is the failure mode this warning exists to
    /// prevent — the chart would simply not be what the script says.
    fn warn_unfoldable_plot_arg(&mut self, args: &[Arg], position: usize, name: &str) {
        let Some(arg) = self.arg(args, position, name) else {
            return;
        };
        if self.fold(arg.value).is_some() {
            return;
        }
        let span = self.span(arg.value);
        self.warnings.push(PineError::new(
            ErrorCode::PineUnsupported,
            span,
            format!(
                "`{name}` must be constant at load time; this one is not, so                  the default is used (per-bar colors and widths land with the                  dynamic-color milestone)"
            ),
        ));
    }

    fn plot_call(&mut self, call: NodeId, args: &[Arg], is_hline: bool) {
        let index = self.plots.len();
        for (position, name) in [(1, "title"), (2, "color"), (3, "linewidth"), (4, "style")] {
            if is_hline && position == 4 {
                continue; // hline has no style argument at that position
            }
            self.warn_unfoldable_plot_arg(args, position, name);
        }
        let title = self
            .arg(args, 1, "title")
            .and_then(|a| match self.fold(a.value) {
                Some(Const::Str(s)) => Some(s),
                _ => None,
            })
            .unwrap_or_else(|| format!("plot {index}"));
        let style = if is_hline {
            // hline is a plot column of a constant value; a Line spec keeps
            // one render path for both.
            PlotStyle::Line
        } else {
            self.arg(args, 4, "style")
                .and_then(|a| match self.fold(a.value) {
                    Some(Const::Enum(tag)) => Some(style_of_tag(tag)),
                    _ => None,
                })
                .unwrap_or(PlotStyle::Line)
        };
        let base_color = self
            .arg(args, 2, "color")
            .and_then(|a| match self.fold(a.value) {
                Some(Const::Color(rgba)) => Some(Rgba8::from_u32(rgba)),
                _ => None,
            })
            .unwrap_or(DEFAULT_PLOT_COLOR);
        let width = self
            .arg(args, 3, "linewidth")
            .and_then(|a| self.fold(a.value)?.as_f64())
            .map_or(DEFAULT_PLOT_WIDTH, |w| w as f32);
        self.plots.push(PlotSpec {
            id: PlotId::new(index),
            title,
            style,
            base_color,
            width,
            offset: 0,
            marker: None,
        });
        self.plot_of_call[call.index()] = Some(index);
    }

    /// `plotshape`/`plotchar`: a marker column. The cells are na (nothing)
    /// or a value; for `location.absolute` the value is the y.
    fn shape_call(&mut self, call: NodeId, args: &[Arg], is_char: bool) {
        let index = self.plots.len();
        let title = self
            .arg(args, 1, "title")
            .and_then(|a| match self.fold(a.value) {
                Some(Const::Str(s)) => Some(s),
                _ => None,
            })
            .unwrap_or_else(|| format!("shape {index}"));
        let shape = self
            .arg(args, 2, if is_char { "char" } else { "style" })
            .and_then(|a| match self.fold(a.value) {
                Some(Const::Enum(tag)) => Some(match tag {
                    "triangledown" => MarkerShape::TriangleDown,
                    "circle" => MarkerShape::Circle,
                    "labelup" => MarkerShape::LabelUp,
                    "labeldown" => MarkerShape::LabelDown,
                    "cross" => MarkerShape::Cross,
                    _ => MarkerShape::TriangleUp,
                }),
                // plotchar's char argument is a string; any char renders as
                // a circle with text.
                Some(Const::Str(_)) => Some(MarkerShape::Circle),
                _ => None,
            })
            .unwrap_or(MarkerShape::TriangleUp);
        let location = self
            .arg(args, 3, "location")
            .and_then(|a| match self.fold(a.value) {
                Some(Const::Enum("belowbar")) => Some(MarkerLocation::BelowBar),
                Some(Const::Enum("absolute")) => Some(MarkerLocation::Absolute),
                Some(Const::Enum(_)) => Some(MarkerLocation::AboveBar),
                _ => None,
            })
            .unwrap_or(MarkerLocation::AboveBar);
        let base_color = self
            .arg(args, 4, "color")
            .and_then(|a| match self.fold(a.value) {
                Some(Const::Color(rgba)) => Some(Rgba8::from_u32(rgba)),
                _ => None,
            })
            .unwrap_or(DEFAULT_PLOT_COLOR);
        let text = args
            .iter()
            .find(|a| a.name.as_deref() == Some(if is_char { "char" } else { "text" }))
            .and_then(|a| match self.fold(a.value) {
                Some(Const::Str(s)) if !s.is_empty() => Some(s),
                _ => None,
            });
        self.plots.push(PlotSpec {
            id: PlotId::new(index),
            title,
            style: PlotStyle::Circles,
            base_color,
            width: DEFAULT_PLOT_WIDTH,
            offset: 0,
            marker: Some(MarkerSpec {
                shape,
                location,
                text,
            }),
        });
        self.plot_of_call[call.index()] = Some(index);
    }

    /// `fill(p1, p2, color)`: both arguments must resolve to `plot()`
    /// results at load time — a fill between unknowable columns cannot be
    /// drawn honestly.
    pub(super) fn resolve_fills(&mut self) {
        for (call, args) in std::mem::take(&mut self.pending_fills) {
            let a = args.first().and_then(|arg| self.plot_ref_of(arg.value));
            let b = args.get(1).and_then(|arg| self.plot_ref_of(arg.value));
            let color = self
                .arg(&args, 2, "color")
                .and_then(|arg| match self.fold(arg.value) {
                    Some(Const::Color(rgba)) => Some(Rgba8::from_u32(rgba)),
                    _ => None,
                })
                .unwrap_or(DEFAULT_FILL_COLOR);
            match (a, b) {
                (Some(a), Some(b)) => self.fills.push(FillSpec {
                    a: PlotId::new(a),
                    b: PlotId::new(b),
                    color,
                }),
                _ => {
                    let span = self.span(call);
                    self.errors.push(PineError::new(
                        ErrorCode::PineUnsupported,
                        span,
                        "fill() arguments must be plot() results (assign the plot to a \
                         variable and pass that)",
                    ));
                }
            }
        }
    }

    /// A node that denotes a plot column: a `plot(...)` call itself, or a
    /// name whose initialiser is one.
    fn plot_ref_of(&self, node: NodeId) -> Option<usize> {
        if let Some(index) = self.plot_of_call[node.index()] {
            return Some(index);
        }
        if let NodeKind::Name(_) = self.kind(node)
            && let Resolution::Global(slot) = self.resolutions[node.index()]
            && !self.reassigned.get(slot as usize).copied().unwrap_or(true)
            && let Some(init) = self.global_init.get(slot as usize).copied().flatten()
        {
            return self.plot_of_call[init.index()];
        }
        None
    }
}
