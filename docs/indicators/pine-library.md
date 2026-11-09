# Pine market indicator library

The library ships 22 distinct market indicators plus eight other existing scripts.
The original nine assets retain their names and source, including `ema.pine`.
Catalog titles and descriptions live in the stores crate; file names remain stable
identities. All new sources were independently authored from the formulas below.
No proprietary script or network oracle is needed to load or test them.

Each indicator consumes the chart's actual OHLCV bars. A period counts chart bars,
including tick, volume and other activity bars, rather than elapsed time. Volume
means buy volume plus sell volume. These formulas do not infer missing volume.

## Formula conventions

`N` is the selected length, `C/H/L/V` are close/high/low/volume, and
`TP = (H + L + C) / 3`. Windows include the current bar. The defaults below are
part of each script; lengths can be changed in its inputs. New length inputs are
bounded to 1..500. The unchanged original EMA input retains its previous limits.

| Script identity | Default | Formula / plotted columns |
| --- | --- | --- |
| `sma.pine` | 20 | Arithmetic mean of source |
| `ema.pine` | 9 | EMA of source, alpha `2/(N+1)` |
| `wma.pine` | 20 | Source weighted 1..N, newest heaviest |
| `vwma.pine` | 20 | `sum(source*V,N) / sum(V,N)` |
| `hma.pine` | 9 | `WMA(2*WMA(source,floor(N/2))-WMA(source,N),round(sqrt(N)))`; derived lengths at least 1 |
| `rma.pine` | 14 | Wilder average of source, alpha `1/N` |
| `rsi.pine` | 14 | `100 - 100/(1 + RMA(gain,N)/RMA(loss,N))` |
| `stochastic.pine` | 14,3,3 | `raw=100*(C-lowest(L,N))/(highest(H,N)-lowest(L,N))`; K=SMA(raw,3), D=SMA(K,3) |
| `stochastic_rsi.pine` | 14,14,3,3 | Same stochastic calculation on RSI, with separate RSI and stochastic lengths; K/D smoothed as above |
| `macd.pine` | 12,26,9 | Fast EMA minus slow EMA; signal EMA; difference histogram |
| `bollinger_bands.pine` | 20,2 | SMA basis and basis +/- multiplier times population standard deviation |
| `atr.pine` | 14 | RMA of true range: max(H-L,abs(H-prev C),abs(L-prev C)); first TR=H-L |
| `adx.pine` | 14,14 | +DI, -DI, ADX; rules below |
| `cci.pine` | 20 | `(TP-SMA(TP,N))/(0.015*mean(abs(TP[i]-current SMA)))` |
| `roc.pine` | 12 | `100*(C-C[N])/C[N]` |
| `momentum.pine` | 10 | `C-C[N]` |
| `williams_r.pine` | 14 | `-100*(highest(H,N)-C)/(highest(H,N)-lowest(L,N))` |
| `obv.pine` | cumulative | Cumulative `sign(change(C))*V`; first contribution zero |
| `mfi.pine` | 14 | `100*positive/(positive+negative)` rolling TP*V sums, classified by TP change |
| `cmf.pine` | 20 | `sum(((2*C-H-L)/(H-L))*V,N)/sum(V,N)`; zero-range multiplier zero |
| `donchian_channels.pine` | 20 | Midpoint, highest high, lowest low |
| `keltner_channels.pine` | 20,10,2 | EMA(C,20) basis and basis +/- multiplier*ATR(10) |

ADX uses `up=H-prev H`, `down=prev L-L`. +DM is up only when up>down and
up>0; -DM is down only when down>up and down>0. Ties and the first bar contribute
zero. DI is 100 times RMA(DM,N)/RMA(TR,N). Zero true range gives both DI values
zero. DX is `100*abs(+DI-(-DI))/(+DI+(-DI))`, or zero when both DI are zero.
ADX is RMA(DX,smoothing). This initial-zero DM convention is explicit and may
differ from platforms that discard the first movement sample.

## Warm-up and exceptional data

Quantick's EMA and RMA seed from the first N nonmissing samples, not the first
price. Their output is `na` until seeded; missing inputs return `na` without
advancing the smoother. Rolling windows require N consecutive nonmissing inputs.
A missing value stays invalid until it leaves the window. WMA/HMA and smoothed
oscillators therefore need their full nested warm-ups. RSI needs N changes
(N+1 closes). MACD needs max(fast,slow) closes for the line, then signal length
valid line samples. ADX needs N+smoothing-1 bars. Momentum/ROC need N+1 closes.

A zero denominator yields `na`, including a flat RSI, flat stochastic range,
flat CCI, zero ROC reference price, zero-volume VWMA/CMF, or MFI without either
positive or negative money flow. RSI with only gains is 100, only losses zero.
MFI with only positive flow is 100, only negative flow zero. The first MFI
sample contributes zero to both sums because no prior typical price exists.

ADX is deliberately zero on a zero-range flat market after warm-up. Donchian
channels and moving averages still produce the constant price. ATR becomes
zero and Bollinger/Keltner bands collapse to their basis.

VWMA, MFI and CMF propagate missing volume through their rolling windows, then
recover when it leaves. OBV reports `na` for missing volume and preserves its
prior accumulator; the next known bar contributes normally. Known zero volume
contributes zero. No script replaces unknown volume with invented trades.

## Numerical evidence and cost

`crates/pine/tests/popular_pine.rs` compiles the actual shipped sources and checks
every numeric plot at every bar, including warm-up `na`, against independent
batch formulas in `fixtures/popular_pine/reference.rs`. Expected values use
window rescans and closed-form geometric convolution rather than production TA
kernels or the evaluator. Separate hand-calculated linear-trend anchors check
the batch formulas and script outputs. Tolerance is absolute `1e-8` plus relative
`1e-8`; `na` must match `na`, and unexpected infinity fails.

The deterministic fixtures cover mixed OHLCV, flat prices, all-zero prices,
zero volume, and isolated missing volume with later recovery. Both default
parameters and integer lengths rebound to 3 are checked. Preview tests change
price/range/volume, preview twice, commit, reset, and replay; committed output
must match the run without previews. Existing corpus and offline oracle tests
remain separate and retain their existing scope; this library does not claim
TradingView certification.

The ignored `popular_pine_replay_budget` diagnostic compares the exact 22 market
scripts against the nine original scripts on the same deterministic 2,000-bar
fixture, with compilation outside the timer and five replay/reset runs each.
Run it with `cargo test -p quantick-pine --test popular_pine --release -- --ignored --nocapture`.
Wall-clock timing is diagnostic evidence, never a flaky CI threshold. All work
runs through the existing Indicator port. CCI alone scans its bounded N-sample
window in Pine; the other formulas use existing incremental kernels.
