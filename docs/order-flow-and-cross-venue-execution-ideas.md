# Order-flow and cross-venue execution ideas

Status: research note, not investment advice or an approved implementation
plan.

Market information last checked: 2026-08-03.

## Executive summary

The central idea is to separate:

- the **signal venue**, where Quantick reads order flow and price discovery;
- the **execution venue**, where a strategy seeks the lowest effective cost.

The first experiment would analyze BTC trades and order-book activity on
Binance and test whether enough time remains to execute on Aster BTC/USD1,
which currently charges 0% maker and 0.005% taker. Bitfinex also deserves
priority: it currently advertises zero trading fees, and its raw `R0` book
exposes identifiers for visible orders.

The hypothesis is plausible but unproven. A Binance order-book imbalance must
not blindly trigger an order on Aster. Quantick must first confirm that Aster
has not already followed the move and that a net edge remains after fees,
spread, slippage, market impact, latency, funding, and the USD1/USDT basis.

The proposed Quantick feature is a deterministic laboratory for
**cross-venue lead/lag and execution quality**, rather than another heatmap.

## Costs, basis points, and leverage

One basis point (`bp`; plural `bps`) is 0.01%:

| Basis points | Percentage |
| ---: | ---: |
| 1 bp | 0.01% |
| 5 bps | 0.05% |
| 10 bps | 0.10% |
| 40 bps | 0.40% |
| 80 bps | 0.80% |

On USD 100,000 of notional value, 1 bp is USD 10.

Trading fees are normally charged on the **full position notional**, not the
posted margin. Leverage does not necessarily increase the fee rate, but it
allows a trader to control more notional with the same capital and therefore
magnifies the fee relative to that capital.

For example, USD 1,000 of margin controlling USD 10,000 at 10x leverage, with
a 5-bps taker fee on each side, incurs approximately USD 10 in entry and exit
fees. That is 10 bps of the position but 1% of the posted margin, before
spread, slippage, and funding.

Short-term trading does not necessarily require an extreme win rate. It
requires positive net expectancy:

```text
net expectancy =
    win probability  * average win
  - loss probability * average loss
  - average all-in cost
```

When the target is only a few bps, however, two taker orders can consume the
entire edge. Win rate alone is not enough; payoff, fills, and all-in cost are
decisive.

## Venues considered

The fees below are public base schedules observed on the date at the top of
this document. They may change by jurisdiction, product, account tier, token
staking, promotion, and order type. Quantick must treat them as versioned
configuration, never as values hidden in strategy code.

| Venue or product | Base maker / taker | Potential use | Main limitation |
| --- | ---: | --- | --- |
| Bitfinex spot, margin, and derivatives | 0 / 0 | Zero trading fees and an order-level `R0` book | Liquidity and spread must be measured per product |
| Aster BTC/USD1 perpetual | 0 / 0.005% (0.5 bp) | Only 1 bp in taker fees for a round trip | Smaller book and USD1/USDT basis risk |
| Aster USDT perpetuals | 0 / 0.04% (4 bps) | More active books | 8 bps for a taker round trip |
| Lighter Standard | 0 / 0 | Account and order IDs in trade messages | Approximately 200–300 ms speed bump |
| Paradex Retail | 0 / 0 | Separates interactive and API liquidity | Speed bump and restrictive order limits |
| Extended perpetuals | 0 / 0.025% (2.5 bps) | Intermediate-cost alternative | Lower adoption and less distinctive data |
| Hyperliquid perpetuals | 0.015% / 0.045% | Counterparty addresses and on-chain flow | Approximately 9 bps for a base-tier taker round trip |
| Binance, OKX, and Bybit perpetuals | Tier-dependent | Liquidity and price discovery | Higher taker costs than the alternatives above |

### Bitfinex

- The current schedule advertises 0% maker and 0% taker fees across spot,
  margin, and derivatives.
- The `R0` WebSocket book publishes an ID, price, and amount for every visible
  order.
- This supports studies of order lifetime, queue churn, and persistence.
- Bookmap and ATAS already connect to Bitfinex, so reproducing a heatmap alone
  would not differentiate Quantick.
- A useful Quantick implementation would combine deterministic replay,
  order-level analytics, cross-venue comparison, and effective cost by order
  size.
- Zero trading fees do not remove funding, financing, custody, counterparty,
  or jurisdiction risk.

### Aster

Aster offers two economically different BTC routes:

- BTC/USD1: 0 maker and 0.005% taker;
- BTC/USDT: 0 maker and 0.04% taker.

In the one-off snapshot collected during this research, BTC/USD1 had an
approximately 1.11-bps spread and USD 3.46 million in 24-hour quote volume.
BTC/USDT had an approximately 0.016-bp spread and USD 708 million in quote
volume. This is a transient observation, not a benchmark, but it illustrates
that the lower-fee route has a much smaller book.

Aster publishes price-level aggregated L2 and supports hidden orders. The
visible order book is therefore incomplete by design and Quantick must label
it accordingly.

### Lighter, Paradex, and Hyperliquid

These venues expose data that is unusual in traditional order-flow platforms:

- Lighter exposes identifiers for both accounts and both orders involved in a
  trade;
- Hyperliquid exposes buyer and seller addresses;
- Paradex publishes separate best prices for interactive and API liquidity.

These fields could support research into participant persistence, toxic flow,
which participants lead moves, and differences between retail and
professional flow.

Quantick already supports public Hyperliquid trades and visible L2. Lighter and
Paradex would add fields that the current feeds do not provide, but their free
lanes include artificial delays. Zero fees do not imply an unrestricted
low-latency API.

### Misleading comparisons to avoid

- MEXC promotions on its website or app do not necessarily apply to API
  execution. Its June 2026 schedule announced 0.06% maker and 0.08% taker fees
  for the futures API.
- Variational Omni offers zero fees but uses an RFQ model with an internal
  liquidity provider, not a central limit order book. It is not suitable for
  conventional order-book analysis.
- A zero maker fee does not guarantee free execution. Queue position, missed
  fills, and adverse selection can cost more than the displayed fee.
- Reported volume can be influenced by incentives. Executable depth, stable
  spreads, and price impact by size are more useful measurements.

## Binance for signals, Aster for execution

### Hypothesis

Binance can serve as a source of price discovery. If an order-flow event occurs
there and Aster responds with a stable delay, a strategy may be able to execute
on Aster before it has fully followed the move.

This is a lead/lag hypothesis, not guaranteed arbitrage.

### Candidate Binance signals

The signal should combine confirmed trades with order-book dynamics:

- order-flow imbalance (OFI);
- microprice displacement from the mid-price;
- aggressive buy/sell volume and sweep velocity;
- depletion and replenishment of the best queues;
- order persistence and cancellation intensity;
- volatility and spread regime;
- agreement between executed aggression and subsequent book movement.

A static imbalance is fragile because displayed walls can be cancelled.
Executed trades, persistent depletion, and failed replenishment should carry
more weight.

### Mandatory Aster confirmation

Before emitting an opportunity, the system must determine:

- Has Aster already followed the move?
- What is the volume-weighted executable price for the intended size?
- Is there enough depth without crossing too many levels?
- Is the spread within its permitted range?
- Is the local feed synchronized and recent?
- Is the USD1/USDT basis stable?
- Are the mark price, index price, and funding behaving normally?

The decision should use:

```text
net executable edge =
    predicted Aster price movement
  - entry and exit fees
  - executable spread
  - market impact
  - expected slippage
  - USD1/USDT basis uncertainty
  - funding
  - latency and model-error buffer
```

An opportunity exists only when this value exceeds a safety margin. The last
traded price must never stand in for the executable price in the order book.

### Proposed event flow

```text
Binance trades + synchronized L2
                  |
                  v
    deterministic order-flow features
                  |
                  v
      predicted direction and move
                  |
                  v
   Aster L2/trades + USD1/USDT basis
                  |
                  v
 all-in cost and executable-depth estimate
                  |
                  v
       opportunity event / no event
```

## Candidate Quantick features

### Cross-venue lead/lag monitor

- measured delay by horizon and market regime;
- probability and median time for the destination to follow;
- remaining move after Aster begins to respond;
- confidence interval, sample count, and false-signal rate;
- warning when the historical relationship deteriorates.

### Effective cost router

```text
effective cost = fee + spread + impact + funding + basis + latency risk
```

The result should be a curve by order size. A venue can be cheapest for a USD
500 order and most expensive for a USD 50,000 order.

### Flow toxicity and adverse selection

- price movement 50, 100, 250, 500, and 1,000 ms after an event;
- probability that a passive fill immediately moves against the position;
- cancellation and replenishment response around sweeps;
- replenishment half-life after a level is consumed;
- maker-versus-taker outcome in each regime.

### Participant persistence

Where a venue explicitly publishes identifiers:

- recurring aggressive accounts or addresses;
- persistence in size and direction;
- participants that lead other venues;
- toxic-flow score by anonymous identifier.

The system must not claim that an address maps to a real-world identity. A
single entity may also operate many accounts.

### Data-confidence indicator

Every result must disclose:

- whether the feed is synchronized, stale, recovering, or gapped;
- visible order-book coverage;
- aggregated price-level or order-level data;
- the possibility of hidden liquidity;
- venue and local receive timestamps;
- whether the aggressor side is venue-declared or inferred.

A signal must disable itself across gaps instead of silently extending stale
order-book state.

## Validation roadmap

### Phase 0: synchronized recorder

Record Binance and Aster concurrently, including:

- trades and updates required to reconstruct visible L2;
- venue timestamps and a local monotonic receive clock;
- sequence IDs and connection generations;
- gaps and resynchronizations;
- USD1/USDT, mark/index price, and funding observations;
- the exact fee schedule assumed for the session.

No live orders are required in this phase.

### Phase 1: deterministic replay study

Run the same logic intended for live operation. Simulate fills against the
historical Aster book, never against Binance prices or candle closes.

Measure:

- gross and net expectancy in bps;
- win rate and average win/loss;
- maximum favorable and adverse excursion;
- frequency of executable opportunities;
- fills, unfilled signals, slippage, and impact by order size;
- Binance-to-decision-to-Aster latency distribution, including p95/p99;
- results by volatility, spread, and liquidity regime;
- sensitivity to costs, additional delay, and nearby parameter values.

Use out-of-sample and walk-forward periods. Reject results that only work on
one day or at one exact parameter value.

### Phase 2: live shadow mode

Generate signals and simulated orders without sending them. Compare the
predicted fill with the book observed after the decision. This phase exposes
latency and stale-state errors that replay may not represent accurately.

### Phase 3: minimum-size execution consumer

Only if the previous phases hold up should an external bot test minimum size
with:

- maximum position and daily-loss limits;
- kill switches for stale feeds, sequence gaps, or disconnections;
- maximum spread, slippage, and order age;
- cancel-on-disconnect where available;
- reconciliation against venue order and fill state;
- blocking when the USD1/USDT basis or funding exceeds its limit.

## Fit within Quantick's architecture

- A public Aster integration belongs in an independent `feed-aster` crate.
- `engine` and `orderbook` remain free of networking, clocks, and credentials.
- Lead/lag features belong in a deterministic domain module reusable by the
  chart, replay/backtest, and bot.
- An authenticated execution adapter remains a separate consumer and must not
  create a reverse dependency into the core.
- Quantick should first emit evidence-rich opportunity events instead of
  becoming a complete order-entry platform.

## Criteria for proceeding or abandoning the idea

The idea advances only if, out of sample:

1. Binance leads Aster for longer than measured end-to-end latency;
2. expectancy remains positive after all costs and stressed slippage;
3. the result survives several order sizes and nearby parameter values;
4. the USD1 basis and limited Aster liquidity do not dominate the predicted
   move;
5. shadow-mode fills resemble simulated fills;
6. degradation and data gaps are detected and block signals.

If the lead disappears after costs or requires unrealistic latency, the
hypothesis must be rejected. Discovering that through recording and replay is
still a useful result.

## Suggested priority

1. Build a synchronized Binance/Aster recorder and lead/lag study.
2. Capture the Bitfinex raw book for order-level analysis and zero-fee
   execution comparison.
3. Extend the existing Hyperliquid integration into participant-flow research.
4. Evaluate Lighter for account/order analytics while modeling its speed
   bumps.
5. Evaluate Paradex for retail-versus-API flow, not as free low-latency
   execution.

## Sources checked

- [Bitfinex: Zero Fees Q&A](https://blog.bitfinex.com/products/zero-fees-qa/)
- [Bitfinex: WebSocket raw books](https://docs.bitfinex.com/reference/ws-public-raw-books)
- [Aster: perpetual fees](https://docs.asterdex.com/trading/perpetuals/fees-and-specs/fees)
- [Aster: API documentation](https://docs.asterdex.com/product/aster-pro/api/api-documentation)
- [Aster: hidden orders](https://docs.asterdex.com/trading/perpetuals/order-types/hidden-order)
- [Lighter: fees and latency](https://docs.lighter.xyz/trading/trading-fees)
- [Lighter: WebSocket reference](https://apidocs.lighter.xyz/docs/websocket-reference)
- [Lighter: API rate limits](https://apidocs.lighter.xyz/docs/rate-limits)
- [Paradex: trading fees](https://docs.paradex.trade/trading/trading-fees)
- [Paradex: retail and pro profiles](https://docs.paradex.trade/trading/trader-profiles)
- [Paradex: FastFill](https://docs.paradex.trade/trading/fastfill)
- [Hyperliquid: trading fees](https://hyperliquid.gitbook.io/hyperliquid-docs/trading/fees)
- [Hyperliquid: WebSocket subscriptions](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/subscriptions)
- [Extended: fees and rebates](https://docs.extended.exchange/extended-resources/trading/trading-fees-and-rebates)
- [MEXC: futures API fee update](https://www.mexc.com/es/announcements/article/updates-to-api-futures-trading-fees-jun-1-2026-17827791535742)
- [Variational: Omni](https://docs.variational.io/omni/about-omni)
- [Variational: RFQ model](https://docs.variational.io/variational-protocol/key-concepts/trading-via-rfq)
- [Bookmap: crypto connectivity](https://bookmap.com/knowledgebase/docs/KB-IntroductionToBookmap-Connectivity#crypto-connectivity)
- [ATAS: supported crypto connections](https://help.atas.net/en/support/solutions/articles/72000602619-which-account-for-trading-and-quotes-is-better-to-choose-)
- [CoinGecko: State of Crypto Perpetuals 2026](https://www.coingecko.com/research/publications/state-of-crypto-perpetuals-report-2026)
