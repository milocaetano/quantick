# Report wheel scenario

Use a `--features harness` build with report autostart. Set
`QUANTICK_PAPER_REPORT_SCROLL=report:-20,rows:-24` to inject ordinary wheel
input over the report and then its nested trade list. The comma-separated
stages use `report` or `rows` followed by a signed notch count; negative
scrolls down. At most four stages and 120 total absolute notches are accepted.
Each stage settles for 120 frames, then emits one notch with 12 intervening
frames. An absent target ends that stage; native propagation can remove the
list target after its end is reached. Later stages still run.

The enlarged regression uses 40 trades, expanded calendar, Light body 25,
monospace 24 and small 18, a 1000x700 logical viewport, and 1000 frames.
The sequence permits 44 wheel events; propagation may finish sooner (34 in
the retained headless fixture). Verify the complete last trade row and footer,
including painted footer bounds, after the initial overflowing state.
No scroll offsets are assigned and the hook does not change layout.

`PAPER_REPORT_SCROLL_WHEEL` logs position and delta;
`PAPER_REPORT_SCROLL_VIEW` logs `footer_visible`, `footer_top` and
`viewport_bottom`. Invalid sequences log `PAPER_REPORT_SCROLL_REJECTED`.
