//! The builtin registry: every name the dialect knows, with the facts the
//! compile passes need — kind (variable vs function), whether a function
//! call owns per-call-site kernel state, and which rejected namespace a
//! name belongs to.
//!
//! One table, consulted by name resolution (with did-you-mean over its
//! keys), by call-site numbering (stateful calls only) and by the
//! unsupported-construct scan. The dialect reference documents exactly this
//! registry; a doc-drift test keeps the two in lock-step (M2 PR 8).

/// Every accepted builtin, by dialect name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    // ---- series variables (host/bar-provided) ----
    /// `open`
    Open,
    /// `high`
    High,
    /// `low`
    Low,
    /// `close`
    Close,
    /// `volume`
    Volume,
    /// `buy_volume` (flow, ours)
    BuyVolume,
    /// `sell_volume` (flow, ours)
    SellVolume,
    /// `delta` (flow, ours)
    Delta,
    /// `cvd` (flow, ours)
    Cvd,
    /// `trade_count` (flow, ours)
    TradeCount,
    /// `hl2`
    Hl2,
    /// `hlc3`
    Hlc3,
    /// `ohlc4`
    Ohlc4,
    /// `hlcc4`
    Hlcc4,
    /// `bar_index`
    BarIndex,
    /// `last_bar_index`
    LastBarIndex,
    /// `time` (bar open time, epoch ms)
    Time,
    /// `time_close`
    TimeClose,
    /// `barstate.isconfirmed`
    BarstateIsConfirmed,
    /// `barstate.islast`
    BarstateIsLast,

    // ---- stateful ta.* kernels (per-call-site state) ----
    /// `ta.sma`
    TaSma,
    /// `ta.ema`
    TaEma,
    /// `ta.rma`
    TaRma,
    /// `ta.wma`
    TaWma,
    /// `ta.vwma`
    TaVwma,
    /// `ta.rsi`
    TaRsi,
    /// `ta.tr`
    TaTr,
    /// `ta.atr`
    TaAtr,
    /// `ta.stdev`
    TaStdev,
    /// `ta.highest`
    TaHighest,
    /// `ta.lowest`
    TaLowest,
    /// `ta.highestbars`
    TaHighestBars,
    /// `ta.lowestbars`
    TaLowestBars,
    /// `ta.change`
    TaChange,
    /// `ta.mom`
    TaMom,
    /// `ta.cum`
    TaCum,
    /// `ta.crossover`
    TaCrossover,
    /// `ta.crossunder`
    TaCrossunder,
    /// `ta.cross`
    TaCross,
    /// `ta.barssince`
    TaBarsSince,
    /// `ta.valuewhen`
    TaValueWhen,
    /// `ta.pivothigh` (kernel lands in M3; accepted by the registry so the
    /// error is honest until then)
    TaPivotHigh,
    /// `ta.pivotlow` (M3, as above)
    TaPivotLow,
    /// `math.sum` (stateful window sum)
    MathSum,

    // ---- stateless math.* ----
    /// `math.abs`
    MathAbs,
    /// `math.max`
    MathMax,
    /// `math.min`
    MathMin,
    /// `math.floor`
    MathFloor,
    /// `math.ceil`
    MathCeil,
    /// `math.round`
    MathRound,
    /// `math.sign`
    MathSign,
    /// `math.sqrt`
    MathSqrt,
    /// `math.avg`
    MathAvg,
    /// `math.pow` (via fmath/libm)
    MathPow,
    /// `math.exp`
    MathExp,
    /// `math.log`
    MathLog,
    /// `math.log10`
    MathLog10,

    // ---- value helpers ----
    /// `na(x)` as a predicate call; the bare `na` literal is lexed apart.
    NaCall,
    /// `nz(x[, replacement])`
    Nz,
    /// `fixnan(x)`
    Fixnan,

    // ---- color.* ----
    /// `color.new(base, transp)`
    ColorNew,
    /// `color.rgb(r, g, b[, transp])`
    ColorRgb,

    // ---- declaration / output (top-level only) ----
    /// `indicator(...)`
    Indicator,
    /// `plot(...)`
    Plot,
    /// `plotshape(...)`
    PlotShape,
    /// `plotchar(...)`
    PlotChar,
    /// `hline(...)`
    Hline,
    /// `fill(...)`
    Fill,
    /// `bgcolor(...)`
    Bgcolor,
    /// `barcolor(...)`
    Barcolor,
    /// `alertcondition(...)` — parsed, inert, load warning.
    AlertCondition,

    // ---- input.* ----
    /// `input.int`
    InputInt,
    /// `input.float`
    InputFloat,
    /// `input.bool`
    InputBool,
    /// `input.color`
    InputColor,
    /// `input.string`
    InputString,
    /// `input.source`
    InputSource,
}

impl Builtin {
    /// Whether a *call* of this builtin owns per-call-site kernel state —
    /// the calls that get a `CallSiteId` and are illegal inside loop bodies
    /// (`PINE_STATEFUL_IN_LOOP`).
    #[must_use]
    pub fn is_stateful_call(self) -> bool {
        matches!(
            self,
            Builtin::TaSma
                | Builtin::TaEma
                | Builtin::TaRma
                | Builtin::TaWma
                | Builtin::TaVwma
                | Builtin::TaRsi
                | Builtin::TaTr
                | Builtin::TaAtr
                | Builtin::TaStdev
                | Builtin::TaHighest
                | Builtin::TaLowest
                | Builtin::TaHighestBars
                | Builtin::TaLowestBars
                | Builtin::TaChange
                | Builtin::TaMom
                | Builtin::TaCum
                | Builtin::TaCrossover
                | Builtin::TaCrossunder
                | Builtin::TaCross
                | Builtin::TaBarsSince
                | Builtin::TaValueWhen
                | Builtin::TaPivotHigh
                | Builtin::TaPivotLow
                | Builtin::MathSum
                // fixnan carries the last non-na value across bars: state.
                | Builtin::Fixnan
        )
    }

    /// Whether this builtin may only be called at the top level of the
    /// script (Pine's rule for declaration and `plot*` family).
    #[must_use]
    pub fn is_top_level_only(self) -> bool {
        matches!(
            self,
            Builtin::Indicator
                | Builtin::Plot
                | Builtin::PlotShape
                | Builtin::PlotChar
                | Builtin::Hline
                | Builtin::Fill
                | Builtin::AlertCondition
                | Builtin::InputInt
                | Builtin::InputFloat
                | Builtin::InputBool
                | Builtin::InputColor
                | Builtin::InputString
                | Builtin::InputSource
        )
    }

    /// Resolve a dialect name (`close`, `ta.ema`, `barstate.islast`, …).
    #[must_use]
    pub fn lookup(name: &str) -> Option<Builtin> {
        Some(match name {
            "open" => Builtin::Open,
            "high" => Builtin::High,
            "low" => Builtin::Low,
            "close" => Builtin::Close,
            "volume" => Builtin::Volume,
            "buy_volume" => Builtin::BuyVolume,
            "sell_volume" => Builtin::SellVolume,
            "delta" => Builtin::Delta,
            "cvd" => Builtin::Cvd,
            "trade_count" => Builtin::TradeCount,
            "hl2" => Builtin::Hl2,
            "hlc3" => Builtin::Hlc3,
            "ohlc4" => Builtin::Ohlc4,
            "hlcc4" => Builtin::Hlcc4,
            "bar_index" => Builtin::BarIndex,
            "last_bar_index" => Builtin::LastBarIndex,
            "time" => Builtin::Time,
            "time_close" => Builtin::TimeClose,
            "barstate.isconfirmed" => Builtin::BarstateIsConfirmed,
            "barstate.islast" => Builtin::BarstateIsLast,
            "ta.sma" => Builtin::TaSma,
            "ta.ema" => Builtin::TaEma,
            "ta.rma" => Builtin::TaRma,
            "ta.wma" => Builtin::TaWma,
            "ta.vwma" => Builtin::TaVwma,
            "ta.rsi" => Builtin::TaRsi,
            "ta.tr" => Builtin::TaTr,
            "ta.atr" => Builtin::TaAtr,
            "ta.stdev" => Builtin::TaStdev,
            "ta.highest" => Builtin::TaHighest,
            "ta.lowest" => Builtin::TaLowest,
            "ta.highestbars" => Builtin::TaHighestBars,
            "ta.lowestbars" => Builtin::TaLowestBars,
            "ta.change" => Builtin::TaChange,
            "ta.mom" => Builtin::TaMom,
            "ta.cum" => Builtin::TaCum,
            "ta.crossover" => Builtin::TaCrossover,
            "ta.crossunder" => Builtin::TaCrossunder,
            "ta.cross" => Builtin::TaCross,
            "ta.barssince" => Builtin::TaBarsSince,
            "ta.valuewhen" => Builtin::TaValueWhen,
            "ta.pivothigh" => Builtin::TaPivotHigh,
            "ta.pivotlow" => Builtin::TaPivotLow,
            "math.sum" => Builtin::MathSum,
            "math.abs" => Builtin::MathAbs,
            "math.max" => Builtin::MathMax,
            "math.min" => Builtin::MathMin,
            "math.floor" => Builtin::MathFloor,
            "math.ceil" => Builtin::MathCeil,
            "math.round" => Builtin::MathRound,
            "math.sign" => Builtin::MathSign,
            "math.sqrt" => Builtin::MathSqrt,
            "math.avg" => Builtin::MathAvg,
            "math.pow" => Builtin::MathPow,
            "math.exp" => Builtin::MathExp,
            "math.log" => Builtin::MathLog,
            "math.log10" => Builtin::MathLog10,
            "na" => Builtin::NaCall,
            "nz" => Builtin::Nz,
            "fixnan" => Builtin::Fixnan,
            "color.new" => Builtin::ColorNew,
            "color.rgb" => Builtin::ColorRgb,
            "indicator" => Builtin::Indicator,
            "plot" => Builtin::Plot,
            "plotshape" => Builtin::PlotShape,
            "plotchar" => Builtin::PlotChar,
            "hline" => Builtin::Hline,
            "fill" => Builtin::Fill,
            "bgcolor" => Builtin::Bgcolor,
            "barcolor" => Builtin::Barcolor,
            "alertcondition" => Builtin::AlertCondition,
            "input.int" => Builtin::InputInt,
            "input.float" => Builtin::InputFloat,
            "input.bool" => Builtin::InputBool,
            "input.color" => Builtin::InputColor,
            "input.string" => Builtin::InputString,
            "input.source" => Builtin::InputSource,
            _ => return None,
        })
    }

    /// Every accepted dialect name — the did-you-mean vocabulary and the
    /// doc-drift test's source of truth.
    #[must_use]
    pub fn all_names() -> &'static [&'static str] {
        &[
            "open",
            "high",
            "low",
            "close",
            "volume",
            "buy_volume",
            "sell_volume",
            "delta",
            "cvd",
            "trade_count",
            "hl2",
            "hlc3",
            "ohlc4",
            "hlcc4",
            "bar_index",
            "last_bar_index",
            "time",
            "time_close",
            "barstate.isconfirmed",
            "barstate.islast",
            "ta.sma",
            "ta.ema",
            "ta.rma",
            "ta.wma",
            "ta.vwma",
            "ta.rsi",
            "ta.tr",
            "ta.atr",
            "ta.stdev",
            "ta.highest",
            "ta.lowest",
            "ta.highestbars",
            "ta.lowestbars",
            "ta.change",
            "ta.mom",
            "ta.cum",
            "ta.crossover",
            "ta.crossunder",
            "ta.cross",
            "ta.barssince",
            "ta.valuewhen",
            "ta.pivothigh",
            "ta.pivotlow",
            "math.sum",
            "math.abs",
            "math.max",
            "math.min",
            "math.floor",
            "math.ceil",
            "math.round",
            "math.sign",
            "math.sqrt",
            "math.avg",
            "math.pow",
            "math.exp",
            "math.log",
            "math.log10",
            "na",
            "nz",
            "fixnan",
            "color.new",
            "color.rgb",
            "indicator",
            "plot",
            "plotshape",
            "plotchar",
            "hline",
            "fill",
            "bgcolor",
            "barcolor",
            "alertcondition",
            "input.int",
            "input.float",
            "input.bool",
            "input.color",
            "input.string",
            "input.source",
        ]
    }
}

/// Why a *rejected* namespace is rejected — the honest message per family
/// (§3.1 reject list). `None` means the name is simply unknown.
#[must_use]
pub fn rejection_of(name: &str) -> Option<(crate::error::ErrorCode, &'static str)> {
    use crate::error::ErrorCode;
    let family = name.split('.').next().unwrap_or(name);
    Some(match family {
        "request" => (
            ErrorCode::PineNoSecurity,
            "request.* is not supported: activity-sampled charts (tick/volume/dollar bars) \
             have no timeframes to request another series from",
        ),
        "timeframe" => (
            ErrorCode::PineNoTimeframe,
            "timeframe.* is not supported: alternative bars are not on a clock",
        ),
        "strategy" => (
            ErrorCode::PineNoStrategy,
            "strategy.* is not supported: this dialect scripts indicators; backtesting \
             consumes their plots through the host instead",
        ),
        "array" | "matrix" | "map" | "table" => (
            ErrorCode::PineNoCollections,
            "collection namespaces (array/matrix/map/table) are not in the v1 dialect",
        ),
        "year" | "month" | "dayofmonth" | "dayofweek" | "hour" | "minute" | "second" => (
            ErrorCode::PineNoCalendar,
            "calendar builtins are not supported: activity-sampled bars are not on a clock",
        ),
        "plotcandle" | "plotbar" => (
            ErrorCode::PineUnsupported,
            "plotcandle/plotbar are not supported: the chart already draws the bars",
        ),
        "max_bars_back" => (
            ErrorCode::PineUnsupported,
            "max_bars_back() is capped by configuration; deeper history reads yield na",
        ),
        _ => return None,
    })
}

/// The classic did-you-mean: the accepted name closest to `unknown` within
/// an edit distance budget, so typos get a hint and garbage does not.
#[must_use]
pub fn did_you_mean(unknown: &str) -> Option<&'static str> {
    let mut best: Option<(&'static str, usize)> = None;
    for candidate in Builtin::all_names() {
        let distance = edit_distance(unknown, candidate);
        if best.is_none_or(|(_, d)| distance < d) {
            best = Some((candidate, distance));
        }
    }
    // Two edits covers the common typo classes (transposition = two plain
    // Levenshtein edits); very short names only get one so garbage cannot
    // pattern-match into `na`.
    let budget = if unknown.chars().count() >= 5 { 2 } else { 1 };
    best.filter(|&(_, d)| d <= budget)
        .map(|(candidate, _)| candidate)
}

/// Plain Levenshtein, two-row rolling. Both operands are short (builtin
/// names), so O(n·m) at load time is nothing.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != cb);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_name_resolves_and_back() {
        for name in Builtin::all_names() {
            assert!(
                Builtin::lookup(name).is_some(),
                "`{name}` is listed but does not resolve"
            );
        }
    }

    #[test]
    fn stateful_calls_are_exactly_the_kernel_family() {
        assert!(Builtin::TaEma.is_stateful_call());
        assert!(Builtin::TaValueWhen.is_stateful_call());
        assert!(Builtin::MathSum.is_stateful_call());
        assert!(!Builtin::MathAbs.is_stateful_call());
        assert!(!Builtin::Plot.is_stateful_call());
        assert!(!Builtin::Close.is_stateful_call());
    }

    #[test]
    fn rejections_name_their_reason() {
        let (code, message) = rejection_of("request.security").expect("rejected");
        assert_eq!(code, crate::error::ErrorCode::PineNoSecurity);
        assert!(message.contains("activity-sampled"));
        assert!(rejection_of("strategy.entry").is_some());
        assert!(rejection_of("array.new_float").is_some());
        assert!(rejection_of("dayofweek").is_some());
        assert!(rejection_of("close").is_none());
    }

    #[test]
    fn did_you_mean_catches_typos_not_garbage() {
        assert_eq!(did_you_mean("ta.emma"), Some("ta.ema"));
        assert_eq!(did_you_mean("colse"), Some("close"));
        assert_eq!(did_you_mean("qwertyuiop"), None);
    }
}
