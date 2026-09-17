//! Anchored-average lifecycle over the existing indicator kernel.
use quantick_engine::Bar;
use quantick_indicators::native::{
    AVWAP_BAND_PAIRS, AVWAP_PLOT_COUNT as AVWAP_ROW_WIDTH, AnchoredVwap,
};
use quantick_indicators::{Ctx, Indicator as _, IndicatorBar, PlotId, SourceId};
pub struct AverageInputs<'a> {
    pub closed: &'a [Bar],
    pub partial: Option<&'a Bar>,
    pub prefix: &'a [Bar],
    pub timeline_revision: u64,
}
pub struct AverageRequest {
    pub anchor_bar: f32,
    pub source: SourceId,
    pub bands: [AvwapBand; AVWAP_BAND_PAIRS],
}
/// One σ-band pair's configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AvwapBand {
    pub on: bool,
    pub mult: f64,
}

/// Everything one refresh computed for one object. Derived state: excluded
/// from equality and presets (the range-profile cache rule), so a
/// recompute can never read as a user edit to the undo history.
#[derive(Debug, Clone)]
pub struct AnchoredAverage {
    /// The **closed-state** inputs the rows were computed from;
    /// `AnchoredAverage::refresh` replays only when this misses. The forming bar has
    /// its own signature ([`AnchoredAverage::partial`]). If this key stays equal,
    /// only that row is re-evaluated. The app advances the timeline revision
    /// on ordinary live input, which instead takes the full replay path.
    key: AvwapCacheKey,
    /// The forming bar the last row describes, when one is on the tape.
    partial: Option<AvwapPartialSig>,
    /// The slot the first row belongs to — the anchor's own bar.
    first_slot: usize,
    /// One row per bar from the anchor to the newest — the forming bar's
    /// row last, when `partial` is set: `[vwap, u1, l1, u2, l2, u3, l3]`,
    /// `NaN` where the kernel answered `na` or the pair is off.
    rows: Vec<[f64; AVWAP_ROW_WIDTH]>,
    /// The kernel as of the last **closed** bar — the accumulators the next
    /// bar close extends and the next forming-bar tick previews against,
    /// when their respective key comparisons permit reuse.
    kernel: quantick_indicators::native::AnchoredVwap,
}

/// Read-only result projection; borrowed data stays in its computation owner.
#[derive(Debug, Clone, Copy)]
pub struct AverageOutput<'a> {
    pub key: AvwapCacheKey,
    pub partial: Option<AvwapPartialSig>,
    pub first_slot: usize,
    pub rows: &'a [[f64; AVWAP_ROW_WIDTH]],
}

/// The forming bar's exact signature — its last-trade instant and trade
/// count — so the live row recomputes when (and only when) it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvwapPartialSig {
    pub close_time: i64,
    pub trades: u64,
}

/// Everything the **closed-bar** replay depends on. An anchor drag changes
/// the slot, a bar close bumps `closed_len`, a config edit changes
/// source/bands, live input or a rebuild bumps the revision, a backfill page grows the
/// prefix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AvwapCacheKey {
    pub anchor_slot: usize,
    pub timeline_revision: u64,
    pub closed_len: usize,
    pub prefix_len: usize,
    pub source: SourceId,
    pub bands: [AvwapBand; AVWAP_BAND_PAIRS],
}

fn bar_at<'a>(inputs: &'a AverageInputs<'_>, slot: usize) -> Option<&'a Bar> {
    if slot < inputs.prefix.len() {
        return inputs.prefix.get(slot);
    }
    let state_slot = slot - inputs.prefix.len();
    let closed = inputs.closed;
    if state_slot < closed.len() {
        return closed.get(state_slot);
    }
    (state_slot == closed.len())
        .then_some(inputs.partial)
        .flatten()
}

/// One committed plot row, read back as the cache's array shape.
fn plot_row(kernel: &AnchoredVwap, row: usize) -> [f64; AVWAP_ROW_WIDTH] {
    let plots = kernel.plots();
    let mut values = [f64::NAN; AVWAP_ROW_WIDTH];
    for (column, value) in values.iter_mut().enumerate() {
        *value = plots.value(PlotId::new(column), row);
    }
    values
}

/// The forming bar's row, previewed against the committed accumulators —
/// the kernel's own rollback contract, so a live tick costs one preview and
/// never a replay.
fn live_row(kernel: &mut AnchoredVwap, partial: &Bar) -> Option<[f64; AVWAP_ROW_WIDTH]> {
    let mut ctx = Ctx {
        bar_index: kernel.plots().len(),
        cvd: &[],
    };
    let frame = kernel
        .preview(&IndicatorBar::from(partial), &mut ctx)
        .ok()?;
    let mut values = [f64::NAN; AVWAP_ROW_WIDTH];
    for (column, value) in values.iter_mut().enumerate() {
        *value = frame.values.get(column).copied().unwrap_or(f64::NAN);
    }
    Some(values)
}

impl AnchoredAverage {
    /// Borrow current results without cloning rows or traversing history.
    #[must_use]
    pub fn output(&self) -> AverageOutput<'_> {
        AverageOutput {
            key: self.key,
            partial: self.partial,
            first_slot: self.first_slot,
            rows: &self.rows,
        }
    }

    /// Number of rows actually committed to the retained indicator kernel.
    #[must_use]
    pub fn committed_rows(&self) -> usize {
        self.kernel.plots().len()
    }

    /// A changed timeline revision replays the span, including live input.
    pub fn refresh(
        current: &mut Option<Self>,
        request: AverageRequest,
        inputs: &AverageInputs<'_>,
    ) {
        let anchor_bar = request.anchor_bar;
        let closed_len = inputs.closed.len();
        let closed_total = inputs.prefix.len() + closed_len;
        let partial = inputs.partial;
        let slots = closed_total + usize::from(partial.is_some());
        let Some(last_slot) = slots.checked_sub(1) else {
            *current = None;
            return;
        };

        // The candle under the anchor, the frvp rounding rule: a slot's centre is
        // its integer coordinate. An anchor past the newest bar seeds on the
        // newest — the average must start on a bar that exists.
        let anchor_slot = if anchor_bar.is_finite() {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let slot = anchor_bar.round().max(0.0) as usize;
            slot.min(last_slot)
        } else {
            *current = None;
            return;
        };

        let key = AvwapCacheKey {
            anchor_slot,
            timeline_revision: inputs.timeline_revision,
            closed_len,
            prefix_len: inputs.prefix.len(),
            source: request.source,
            bands: request.bands,
        };
        let partial_sig = partial.map(|bar| AvwapPartialSig {
            close_time: bar.close_time,
            trades: bar.trade_count,
        });

        // An equal key permits one forming-row preview against the retained accumulators — O(1),
        // whatever the anchored span.
        if let Some(cache) = current.as_mut().filter(|cache| cache.key == key) {
            cache.refresh_partial(partial, partial_sig);
            return;
        }

        // A bar close with everything else unchanged extends the retained
        // kernel over the new closed bars — O(new bars), never the whole span.
        if let Some(cache) = current.as_mut().filter(|cache| {
            cache.key
                == AvwapCacheKey {
                    closed_len: cache.key.closed_len,
                    ..key
                }
                && cache.key.closed_len < closed_len
        }) {
            cache.append_closed(key, partial_sig, inputs);
            return;
        }

        // Full replay from the anchor bar — an anchor drag, a config edit or a
        // timeline rebuild. The anchor instant is that bar's own open, so every
        // bar from it onward participates — the same `close_time >= anchor`
        // rule the kernel's golden fixture pins.
        let Some(anchor_ms) = bar_at(inputs, anchor_slot).map(|bar| bar.open_time) else {
            *current = None;
            return;
        };
        let bands: [(bool, f64); AVWAP_BAND_PAIRS] =
            request.bands.map(|band: AvwapBand| (band.on, band.mult));
        let mut kernel = AnchoredVwap::new(anchor_ms, request.source, bands);
        let mut rows = Vec::with_capacity(last_slot - anchor_slot + 1);
        for slot in anchor_slot..closed_total.max(anchor_slot) {
            if slot > last_slot {
                break;
            }
            let Some(bar) = bar_at(inputs, slot) else {
                break;
            };
            // No cross-bar series is read: every anchored source is price-scaled,
            // so the host-maintained cvd can honestly stay empty here.
            let mut ctx = Ctx {
                bar_index: kernel.plots().len(),
                cvd: &[],
            };
            if kernel.on_close(&IndicatorBar::from(bar), &mut ctx).is_err() {
                break;
            }
            rows.push(plot_row(&kernel, rows.len()));
        }
        if let Some(bar) = partial
            && let Some(row) = live_row(&mut kernel, bar)
        {
            rows.push(row);
        }
        *current = Some(AnchoredAverage {
            key,
            partial: partial_sig,
            first_slot: anchor_slot,
            rows,
            kernel,
        });
    }
    fn refresh_partial(&mut self, partial: Option<&Bar>, partial_sig: Option<AvwapPartialSig>) {
        if self.partial != partial_sig {
            if self.partial.is_some() {
                self.rows.pop();
            }
            if let Some(bar) = partial
                && let Some(row) = live_row(&mut self.kernel, bar)
            {
                self.rows.push(row);
            }
            self.partial = partial_sig;
        }
    }
    fn append_closed(
        &mut self,
        key: AvwapCacheKey,
        partial_sig: Option<AvwapPartialSig>,
        inputs: &AverageInputs<'_>,
    ) {
        let partial = inputs.partial;
        let closed_total = inputs.prefix.len() + inputs.closed.len();
        if self.partial.is_some() {
            self.rows.pop();
        }
        let done = key.anchor_slot + self.kernel.plots().len();
        for slot in done..closed_total {
            let Some(bar) = bar_at(inputs, slot) else {
                break;
            };
            let mut ctx = Ctx {
                bar_index: self.kernel.plots().len(),
                cvd: &[],
            };
            if self
                .kernel
                .on_close(&IndicatorBar::from(bar), &mut ctx)
                .is_err()
            {
                break;
            }
            self.rows.push(plot_row(&self.kernel, self.rows.len()));
        }
        if let Some(bar) = partial
            && let Some(row) = live_row(&mut self.kernel, bar)
        {
            self.rows.push(row);
        }
        self.partial = partial_sig;
        self.key = key;
    }
}
