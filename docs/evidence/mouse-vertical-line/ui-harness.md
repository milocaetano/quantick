# Mouse vertical line UI harness evidence

Fresh-launch routes added by this change:

- `QUANTICK_CONTEXT_MENU=indicator` opens the indicator context menu through
  the same menu path used by a secondary click.
- `QUANTICK_INDICATOR_MOUSE_LINE=<index>` enables the persisted setting for an
  exact indicator through the registered action path.

The exercised CVD scene showed the unchecked and checked menu states, a dashed
vertical guide only in CVD while the pointer was inside the price plot, no
horizontal guide, and no guide in indicators left disabled. Moving the pointer
away from the price plot cleared the guide. The app remained at zero worker
backlog at 150 percent display scale.

Operability uses `indicator.mouse_vertical_line.set` with exact tab, pane, and
indicator-slot identity. Its typed result and observer analysis state provide
readback; the capability and UI behavior registries provide discovery. An
invalid overlay target is rejected before durable mutation.
