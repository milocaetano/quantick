# Pull request 306 refactor

The integration removes parallel toolbar state: trades bars now use `BarSpec::Trades` through `SpecSelector`. Deal-recording UI and persistence wiring moved from an additional `QuantickApp` implementation block into functions owned by `app/deal_recording_wiring.rs`; its default lives in `ChromeState` rather than widening the protected application root.

Guard baselines explicitly account for the remaining domain state and small docking additions. The complete guard suite passes.
