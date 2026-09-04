# The paper branch's `app.rs` hunks, mapped to the functions that own them

Run twice, as the mission required: once before the first line was moved
(against merge base `d3cf317`) and once with the branch finished and rebased
(against merge base `e0ae2ac`, after PR #298 moved `origin/main` forward).
Both runs produced the same set of owning functions.

Branch: `refactor/paper-policy-out-of-the-ticket`. 27 hunks, 9 distinct owners.

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

## Overlap

The paper branch's owners:

  - `adopt_tab`
  - `arm_strategy_instance`
  - `draw_menu_bar`
  - `new_with_workspace`
  - `open_trades_dir_picker`
  - `persist_cmd_trading`
  - `persist_order_strategies`
  - `persist_risk_settings`
  - `poll_trades_dir_picker`

This branch moved 60 methods. The intersection of the two sets is
**empty**.

Every one of the paper branch's owners is also on this mission's own
out-of-scope list. The two branches touch `app.rs` in disjoint regions and
meet only at `size-baseline.txt`'s `!budget` line, which this branch now
resolves by hand against the post-#298 number.
