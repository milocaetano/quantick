# Pull request 306 refactor

The integration removes parallel toolbar state: trades bars now use `BarSpec::Trades` through `SpecSelector`. Deal-recording UI and persistence wiring moved from an additional `QuantickApp` implementation block into functions owned by `app/deal_recording_wiring.rs`; its default lives in `ChromeState` rather than widening the protected application root.

Guard baselines explicitly account for the remaining domain state and small docking additions.

Before integration, the PR branch carried parallel toolbar `kind`/`deals_n` state and added a 94-line `impl QuantickApp`; its application tree measured 111,826 lines, with the largest gateway at 4,142 lines, `pane.rs` at 5,655, `tab.rs` at 4,498, `QuantickApp` at 57 root items, and `Tab` at 63. After integration, the parallel selector state is gone, the recorder wiring is a 76-line owner module, `pane.rs` is 5,398 production lines, `QuantickApp` grew only 23 protected implementation lines over current main, and the current-main `Tab` split remains intact. The signed ratchet deposits total 53 production lines: pane 14, tab 15, and MT5 stream 24.

Ownership map: engine aggregation stays in `engine/deals.rs`; MT5 counter mapping stays in `feed-mt5/deals.rs`; durable files stay in `app/deal_recording.rs`; UI stays in `deal_recording_ui.rs` and `deal_recording_tab.rs`; app orchestration stays in `app/deal_recording_wiring.rs`; discovery/action/readback stay in `control/deal_recording.rs`.
