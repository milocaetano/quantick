# F2 fixture and command inventory

This index points to command-generated stdout/stderr receipts. It is not a
substitute for those receipts and does not turn compile-only checks into test
execution.

Current adopted campaign base: `9ff57501249f51d8f75c21f52094ec3dc3c39af2`
(`4a33c57d8f14da72200ba746a69a04a3c5ca9b02`). The source-first GOAL SHA-256
remains `17b6d89db5ba15e6a6ff5c4328b42308e9ce734e51ef71273b983ee409d9c7b4`.

## Pre-repair fixture identities

- Binance real HTTP/WebSocket host fixtures:
  `crates/feed/src/continuity/tests.rs`, SHA-256
  `4df6ab91c6b1277fa0d3647729e50577fdd83fad4c67d29e2c3f995b47c0871e`.
- Hyperliquid ordered real-WebSocket fixture:
  `crates/feed-hyperliquid/src/stream.rs`, SHA-256
  `1843d624b52113011c5b3742c6c4dcd0f80b942704a4fd5bb2bf0a13c8cef607`.
- Hyperliquid real-WebSocket/provider-neutral-host fixture:
  `crates/feed/src/hyperliquid.rs`, SHA-256
  `51935f10ebaf02c78e1b686006ce7ddd2d99b90f404937dd38cc4a585754f3c6`.
- Normal app drain plus `health.summary`/`feed.status` fixture:
  `crates/app/src/app/tests/feeds_sources_tests.rs`, SHA-256
  `0696e9a1ffdb0f24c77b4bca2bab9bbcfc2fb14ff974e31af00996589b00054e`.

## Raw receipts

| Receipt | SHA-256 | Terminal status |
| --- | --- | --- |
| `binance-pre-repair.log` | `9065440f94ca2100a4b5266ef6c47c1da0ca8c0c09bb34b3c5b23d25b27fb6e5` | exit 101; two expected missing-event timeouts |
| `binance-post-repair.log` | `2f7d642aed4252d351e0d7ea26c8d6710052e5c7ff8130c2491ea21e58309748` | exit 101; assertions reached, fixture retained command sender and cleanup timed out |
| `binance-post-repair-attempt2.log` | `32ea72952a31c40483a212bdb08d34935cce651abd3cce7f78f87feea91362f9` | exit 0; 4 passed |
| `hyperliquid-pre-repair.log` | `ba74fe9a130c319e35766b86ca3989221e009eca926d00077df8d45aaaf94f4e` | exit 101; planned ordered API absent |
| `hyperliquid-post-repair.log` | `c80a0cffe2367e076fee435288bf98f4e5faa31cd8a5ec65fbb9342bdea83b82` | exit 0; ordered fixture passed |
| `hyperliquid-host-pre-repair.log` | `7e57c5e2bdd6e53fcfe8e446e22370933c721340b0ebdfb4a21f6b6fafe420fc` | exit 101; planned host source/diagnostic port absent |
| `hyperliquid-host-post-repair.log` | `b26c847e452b6fab2c80e3be1d2763a82efb6b04e773428f8d659bdbedf0e82b` | exit 0; real source-to-host fixture passed |
| `headless-feed-suite.log` | `47aa55e106c7ffc03ee209dccf5079433e98ee11b1fc9f0fad6790a41a7bf82b` | exit 0; complete three-crate suite |
| `app-pre-edit-check.log` | `09be5486c9aec3ecedc443b6aca122b08fad86340bbbfb18ed19436a30b7cbbc` | exit 101; post-feed-change E0004, not a pristine baseline PASS |
| `base-adoption.log` | `6b3199e23f8077a6435409d2a6633cf4c6d22dee257ba0dfd5284b3032512319` | exit 0; disjoint dirty fast-forward to integrated A1R base |
| `app-fixture-pre-repair.log` | `f9864b01a3e6b38cde04bf08cec79d8dd60b53560beb654c7b1a401c8d2689f8` | exit 101; fixture-hashed compile-red E0004, assertions not executed |
| `app-post-repair-check.log` | `eb1ce0a73e54b989eb9bed16fef2f0e6602d7cadc112fdcfcf6ab19fb6ecbba6` | exit 0; compile-only app/tests check |
| `headless-clippy.log` | `ff91a2af07e7c7c2eae6ab9bee5a5d817c64acf5e0e5074902ffde44cdc126f6` | exit 0; three touched headless crates/all targets |
| `app-focused-test.log` | `22aa553389285d9395fe3fd6caa36ca80b83327c3657eab734892c3e932d3d24` | exit 0; historical pre-S2-correction app assertion, superseded by the source repair below |
| `schema-generation-observer.log` | `499a8bc4bc99b30e4833191923b8f18efc79ce8a545ea8b31511c43bf507ef0e` | exit 0; historical pre-S2-correction generation result |
| `schema-generation-catalog.log` | `de30a57661fec0c1a1bda7839c5a07c755c6118f10fe55518512b4f41429e13f` | exit 0; historical pre-S2-correction generation result |
| `schema-validation-observer.log` | `ebcd717305e84626ca9cc7ddd3fc540ece9e0009d45e0f4a0a182772bd183819` | exit 0; historical pre-S2-correction validation result |
| `schema-validation-catalog.log` | `df371544abe50ec4e696a88965cb2d4c6f68d49abd8a1bd7b7f4618212971295` | exit 0; historical pre-S2-correction validation result |
| `hyperliquid-duplicate-ack-pre-repair.log` | `3efe6b1761fb49e8970a9cbc69d1a08787da4504f9555a4f6ef8e480ed9e71a9` | exit 101; duplicate acknowledgement displaced the expected disconnect edge |
| `hyperliquid-duplicate-ack-post-repair.log` | `1e642de16915c124ba658d6107a42a551ab847dc34e9551b267f5d533deb92bf` | exit 0; edge-triggered acknowledgement fixture passed |
| `s2-repair-fmt.log` | `fc5e6a94e7b7bcf9b0204f099795a3e32ef733c63fad766bf96cee936f22b58f` | exit 0; formatter after the S2 contract repair |
| `s2-hyperliquid-host-post-repair.log` | `cadd8402ae20c15b97060061a3050ce8199fa40d30821a0ef5a025bc5c5dbf97` | exit 0; corrected existing-continuity host event order passed |

The S2 correction removes the draft public `FeedEvent::Excluded` variant and
the draft category-specific integrity fields. Malformed and stale rows now use
separate existing `FeedEvent::Continuity` values with exact known counts; only
stale sets `non_monotonic`. The current app fixture SHA-256 is
`18a7d3002c1aa241f81e4b3f95417471dc3387f3e00966c15f78fb4c86a6f495`.
Current schema regeneration/validation after that correction is pending the
next app slot. The released-v1 manifest is still byte-identical at SHA-256
`fff12d5cc40eb0219ae336693b7aaea2e3f52695964d5da3a99b71c04ee87079`.

Paired quiet-host measurements, full ordered workspace validation, reviews,
CI, and delivery are still pending. Business/external operation and repair
counters remain `0/0`; test/check attempts above are command receipts, not
business operations.
