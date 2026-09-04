# The paper branch's `app.rs` hunks, mapped to the functions that own them

Run twice: once before the first line was moved, and once with the branch
finished, against a freshly fetched `refactor/paper-policy-out-of-the-ticket`.
Both runs produced this same mapping.

Merge base: `d3cf317`. 27 hunks, 9 distinct owners.

| Hunk starts at | Owning function |
| --- | --- |
| 576 | `new_with_workspace` |
| 599 | `new_with_workspace` |
| 600 | `new_with_workspace` |
| 608 | `new_with_workspace` |
| 613 | `new_with_workspace` |
| 1300 | `new_with_workspace` |
| 1306 | `new_with_workspace` |
| 1326 | `new_with_workspace` |
| 1342 | `new_with_workspace` |
| 1358 | `new_with_workspace` |
| 1364 | `new_with_workspace` |
| 1462 | `open_trades_dir_picker` |
| 1494 | `poll_trades_dir_picker` |
| 1502 | `persist_cmd_trading` |
| 1533 | `persist_risk_settings` |
| 1537 | `persist_risk_settings` |
| 1566 | `persist_order_strategies` |
| 1569 | `persist_order_strategies` |
| 1573 | `persist_order_strategies` |
| 2212 | `adopt_tab` |
| 2216 | `adopt_tab` |
| 2231 | `adopt_tab` |
| 2246 | `adopt_tab` |
| 2252 | `adopt_tab` |
| 4941 | `draw_menu_bar` |
| 6497 | `arm_strategy_instance` |
| 6499 | `arm_strategy_instance` |

## The functions this branch moved

`capture_workspace` … `note_workspace`, `open_requested_indicator_settings`
… `maintain_indicator_state`, `attach_script_indicator`,
`detach_script_indicator`, and `apply_layer_actions` … `apply_layer_defaults`.

## Overlap

None. The paper branch's owners are:

  - `adopt_tab`
  - `arm_strategy_instance`
  - `draw_menu_bar`
  - `new_with_workspace`
  - `open_trades_dir_picker`
  - `persist_cmd_trading`
  - `persist_order_strategies`
  - `persist_risk_settings`
  - `poll_trades_dir_picker`

Not one of them is a method this branch moved, and every one of them is on
the mission's own out-of-scope list. The two branches touch `app.rs` in
disjoint regions and meet only at `size-baseline.txt`'s `!budget` line.
