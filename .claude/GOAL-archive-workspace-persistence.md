# Goal: the workspace persists — DONE (PR #135 open)

Give quantick a saved **workspace**: the arrangement the app opens on —
which tabs, which canvas layout, which bar spec per pane, the split, the
focus, the dock, the timezone, the rail, the window — captured to
`ui-state.toml` and restored at startup, with an explicit *Save workspace*
action in the menu bar. Ship the two-chart default the user asked for:
timeframe pane beside a tick-bar flow pane.

This is the P1 row the chart-layouts goal (PR #129) deferred by name:
`ui-state.toml` (§14 of `docs/ux/ui-design-model.md`, "Biggest single
lever" in `docs/ux/ux-audit-2026-08.md` §6).

Branch `feat/workspace-persistence`, worktree
`../quantick-worktrees/feat-workspace-persistence`.

## UX design (agent's call — the user delegated it)

Two tiers, named apart so "did it save?" is never a question:

1. **Explicit** — `Workspace → Save workspace` (Ctrl+Shift+S) writes the
   current arrangement and says so in the status line. This is the option
   the user asked for.
2. **Automatic** — `Save on exit` (checkbox, on by default) means the
   trader who never opens the menu still reopens where they left off.

Plus `Restore saved workspace` (discard this session's drift) and
`Reset to factory layout` (forget the file, back to config defaults).

One file, one workspace. Named multi-workspaces ("scalp", "swing") are
deliberately **out of scope** — deferred in the PR body.

`ui-state.toml` owns only what no other store owns. Layers stay in
`chart-layers.toml`, indicators in `indicators-state.toml`, drawing presets
and paper state in theirs. One field, one file — no store may disagree with
another about a pixel.

## Acceptance criteria

- [ ] 1. `ui-state.toml` store: versioned TOML beside the config,
      `QUANTICK_UI_STATE` override, temp-file-and-rename write, unknown
      version / unreadable file ignored entirely (never half-restored) —
      the same discipline as `chart-layers.toml`.
- [ ] 2. Saved and restored: window size; tab strip (feed, symbol, canvas
      layout, split fraction, focused pane, bar spec per pane) and the
      active tab; dock open/width/tab; timezone; tool rail side/visibility.
      Every restored value is validated against the live config — a feed or
      symbol that no longer exists is dropped, not resurrected.
- [ ] 3. `Workspace` menu with Save / Restore / Reset / Save-on-exit, a
      status-line confirmation on save, and a keyboard shortcut.
- [ ] 4. **Default layout = two charts**: timeframe pane first, tick-bar
      flow pane second, out of the box. Declared in config, not hardcoded.
- [ ] 5. Tests: round-trip of the store, rejection of a bad/unknown-version
      file, restore-drops-unknown-feed, save-on-exit toggle honoured,
      factory default opens two charts with the flow pane on tick bars.
- [ ] 6. Performance declared: restore is startup-only, save is
      event-driven/debounced off the frame path; the per-frame cost is a
      dirty-flag check, measured (`APP_HEALTH_SUMMARY` fps/frame_avg vs. a
      `main` control run under the same tape).
- [ ] 7. `new-extension`: the store docks as its own module with
      registration-only edits elsewhere; blast radius (added vs. edited
      files) stated in the PR body.
- [ ] 8. `ui-harness`: `QUANTICK_UI_STATE` hook added and registered in the
      skill table; every new surface reachable from a fresh launch.
- [ ] 9. `visual-qa` pass on the affected surfaces; `trader-ux-review` with
      no unresolved Blocker.
- [ ] 10. `arch-review` over `git diff main...HEAD`, every Blocker and
      Should-fix resolved or deferred in the PR body.
- [ ] 11. Four checks green after rebasing on latest `main`; **PR opened**
      with the evidence in its body. No merge.
