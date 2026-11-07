# Layout surfaces under the Dark and Light appearance (2026-11-06)

Main's configurable appearance (#765) and the new layout (`feat/quantick-new-layout`) met in one merge. The layout's tab strip, Home tab, menu bar, settings window, native market windows and statusbar painted fixed constants; under Light several of them vanished (`HOME_INK` and `Color32::WHITE` measure 1.1 to 1.2:1 on Light chrome). This records how those surfaces now take their colours.

Three opinion panels proposed token maps under distinct lenses: browser-tab conventions, a dark-first trading terminal, and contrast/accessibility. The trader's mocks (`docs/layout/chrometab/`, `docs/thema/white/`) decided wherever they spoke. Where the panels disagreed, the decision and the reason are below.

## Rule

Every layout surface reads the resolved palette (`theme::get(ctx)` or its equivalent), never a `theme::` colour constant. A future theme edits one TOML file in `crates/appearance/themes/`.

## Tokens

New tokens, three only: `tab_strip`, `brand`, `replay_ink`. Everything else reuses main's tokens.

| Surface | Token | Dark | Light | Decided by |
|---|---|---|---|---|
| Strip and native window title area | `tab_strip` (new) | #10141D (today's `inset`) | #D9D9D9 | Light `inset` #E6E6E6 against `chrome` #ECECEC hides the strip; the panels split, the browser lens won |
| Active tab | `chrome`, joined to the toolbar with no bottom edge, 1 px `border` on top and sides | | | Mock: the active tab and the toolbar are one sheet. Contrast lens: fill alone is 1.07:1, so shape and outline carry it |
| Inactive tab | no fill, `text_muted` | | | Mock |
| Venue suffix ("from WINZ26 2") | `text_muted` | | | `text_faint` fails 4.5:1 in both themes |
| Hover | `bare_hover` | | | All three panels |
| Close × | `text_muted` glyph, visible on the active and hovered tab; hover = round `bare_hover`, `text_primary` glyph | | | Mock shows × on the active tab only |
| Unsaved edit dot | `amber` | | | Mock |
| Separator | `seam_line`, hidden next to the active or hovered tab | | | Light `border` is too heavy for a divider |
| Keyboard focus ring | `accent`, 2 px inset | | | The accent marks focus only, never the active tab |
| Home tab | pinned: no close, no separator after it; "uan" `text_primary` in every state, never dimmed | | | The trader forbids logo changes: the wordmark keeps the previous build's ink |
| Home wordmark "tick" | `brand` (new) | #3AF0B2 | #12C68C | The trader's approved W6 green (2026-11-06); a logotype has no contrast floor |
| Feed state on a tab | live: filled dot `buy_ink`; needs attention (connected while the venue reports a failure, e.g. an MT5 terminal closed behind an open socket): filled diamond `warn`; replay: filled triangle `replay_ink` (new, #C58CFF / #7F36B3) plus the word "Replay"; disconnected: hollow ring `warn`; stopped: hollow ring `text_faint` | | | Shape plus colour, so it survives colour blindness; replay stays labelled in words (data honesty); amber now means "unsaved" in tabs. A connected feed with a venue notice is neither live nor lost: its diamond shares the ring's ink and differs from both by shape; only a lost connection wears the warn ring. `quantick_workspace` decides the state (`TabFeed`), `quantick_appearance` owns the mark (`FeedMark`), the app's strip maps one to the other |
| Per-market colour | none | | | The symbol text is the identity; hue already means buy, sell, unsaved and replay |
| Menu bar | `chrome`, items `text_primary`, hover `bare_hover` | | | Mock |
| Settings window | main's `chrome::apply_palette` (`chrome` fill, `border` stroke, `widget_shadow`); fields `inset`; toggle off track `text_faint` | | | `border` is 1.5:1, under 3:1 for a control |
| Native window title text | `text_primary` focused, `text_muted` not; keeps the `amber` 3 px focus stroke | | | |

## Open for the trader

- **Light toolbar.** The TradingView-style Light mocks show a white toolbar; main's Light `chrome` is #ECECEC. This merge keeps main's value. Raising it is a one-line change in `light.toml`; `tab_strip` should then stay about 20 levels darker than `chrome`.
