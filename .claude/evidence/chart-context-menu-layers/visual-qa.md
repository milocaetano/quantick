# Chart context menu visual QA

Verdict: PASS.

- `default-menu.png` — default `time+flow` layout at 1400 × 900 points. The bare chart menu starts with `Anchor VWAP here`, exposes one `chart layers` row, omits market orders, and retains limit/stop orders.
- `expanded-narrow-menu.png` — single flow layout at 1000 × 1100 points. The complete chart-layer inventory opens to the right without horizontal or vertical clipping; disabled `backfill divider` remains visibly unavailable rather than disappearing.
- Both captures used a dense live Binance canvas with paper-demo state and isolated scratch stores. Health summaries reported 59 FPS and 1.52–1.66 ms frame CPU time.
- Structured popup rows are not part of the control-plane scene contract. The egui regression test reads the painted row rectangles directly and verifies the same hierarchy and inventory.

Trader UX review:

- Rafa: PASS — fast order entry remains in the ticket/hotkeys, while accidental market execution is removed from the context menu; resting limit/stop actions remain.
- Marina: PASS — the familiar right-click remains, and display management costs one predictable rightward expansion.
- Duda: PASS — `chart layers` names the grouped controls plainly, disabled state remains visible, and the menu no longer presents market actions beside display controls.

No unresolved Blocker or Should-fix finding remains.
