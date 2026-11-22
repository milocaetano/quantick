# Release builds and capture hooks

`quantick-app` defaults to no harness features. `--features harness` enables
the existing scenario, control, drawing and quick-range families, including
their dependency features. Unit tests also compile their own test hooks.
Production local-agent access remains available through the app's settings.

Audit the actual executables from the repository root:

```sh
cargo build --release -p quantick-app
python tools/ci/binary_hook_scan.py --binary target/release/quantick-app
cargo build -p quantick-app --features harness
python tools/ci/binary_hook_scan.py --harness
```

On Windows, append `.exe` to the explicit release path. The default scan path
selects the platform's debug executable. CI checks both default debug and
release builds, and checks that the capture build contains every registered
hook. The scan includes names grouped in registry prose and adjacent string
literals; Rust strings do not require a separator in the executable. Schema
descriptions and bundled configuration comments have no exemption.

The default executable retains these operator configuration names, declared
and captured once in `crates/app/src/launch.rs`:

| Name | Reason retained |
| --- | --- |
| `QUANTICK_BOOK_DEPTH` | Binance order-book depth selection. |
| `QUANTICK_BUBBLES` | Operator's bubble presets file. |
| `QUANTICK_CONFIG` | Operator's feed configuration file. |
| `QUANTICK_DEALS_DIR` | Deal recording destination. |
| `QUANTICK_DEFAULT_FEED` | Opening feed selection. |
| `QUANTICK_DEFAULT_SYMBOL` | Opening market selection. |
| `QUANTICK_INDICATORS_DIR` | Indicator script library. |
| `QUANTICK_LOG_FORMAT` | Text or JSON operational logging. |
| `QUANTICK_REPLAY_DIR` | Recorded-session library. |
| `QUANTICK_STRATEGIES_DIR` | Strategy script library. |
| `QUANTICK_TRADES_DIR` | Paper-trading journal destination. |
| `QUANTICK_THEME` | Operator's appearance configuration file. |

`QUANTICK_GIT_COMMIT` also remains in the known-name table: it identifies
compile-time build metadata, never a runtime hook. The bare `QUANTICK_`
prefix supports unknown-name diagnostics and store-write refusal when someone
launches a default build with capture variables. That protection prevents a
capture session from saving into the operator's real cockpit.

No test or capture hook name is retained in the default executable. Use the
feature-enabled binary for UI captures and hook-registry generation.
