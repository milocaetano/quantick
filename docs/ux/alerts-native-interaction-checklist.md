# Native alert interaction checklist (C11)

C11 was run on 2026-11-10 with real Windows input: `SendInput` mouse moves, clicks, wheel and keys, sent only after a guard confirmed that the foreground window belonged to the isolated demo process and that the cursor sat inside its client area. Every event is logged with its time and process id. The guard aborted five times when another window (a remote-desktop client) took the foreground; no event was sent in those cases. Each instance used the fixture stores and synthetic `ALERTTEST` replay under `.artifacts/alerts-ui-native/c11*`; no user store was read or written and no order was sent.

The first pass, on `quantick-alerts-485a77bb.exe` (SHA-256 `457E82EE…4B960122`), found defects. They were fixed and the matrix was repeated on rebuilt harness executables:

| Executable | Commit | SHA-256 | Cells |
| --- | --- | --- | --- |
| `bin/quantick-alerts-c11fix.exe` | `03b2fdf4` | `5B39DFBB693E24557233D2A99195DF79829C38CE4586E5EFF7999B51BAC4C36E` | dark-normal (full flow) |
| `bin/quantick-alerts-c11fix3.exe` | `44e19bb9` | `1EC6F869CFCD3B91BF8CD6E29443CCA3D7219E13333965C1898C620BC526AB37` | light-short |
| `bin/quantick-alerts-c11fix4.exe` | `5858a812` | `361DA13592F3C9FA0FFD10D0CC2B60F4A150C12B545D314AF2042BC4EE0A34AE` | dark-short, light-normal |

The dark-normal flows were not repeated on the final executable. The fixes that came after them (#5, #6, #7) were checked natively in the later cells. Captures and input logs: `.artifacts/alerts-ui-native/{c11-dark-normal,c11fix-dark-normal,c11fix2-light-short,c11fix3-light-short,c11fix4-dark-short,c11fix4-light-normal}/{shots,input.log}`. Launcher: `launch-user-preview.ps1` with an added `none` mode, so that no surface is opened by a hook.

## Defects found and fixed

1. **The context bar painted over the editor.** The selected drawing's context bar (an `Order::Foreground` area) was drawn on top of the alert window. Hovering the Trigger combo offered "Delete rectangle (Del)", and a click there did not open the combo. The bar now stands down while the alert surface is open.
2. **The summary leaked internal names.** It read `PerBar` and `drawing 8 (6:replay9:…/4d64…)`. It now uses the combo labels and drawing names (`Price Entering Parallel channel 8; Once per bar`) in the editor and the manager.
3. **The operator popup hid choices.** It kept the height of the previous target kind: "Less Than" (line) and "Inside"/"Outside" (rectangle) sat behind an unmarked scroll. Each kind now keeps its own popup state.
4. **The expiration error contradicted its preview.** A past date showed "must be in the future" beside the last valid cutoff. The preview is now hidden while the fields are invalid.
5. **A search could claim there were no alerts.** A search matching nothing read "No alerts. Create one for a price or drawing." while alerts existed. It now reads "No alerts match the search."
6. **Tab focus could land out of view.** In a short window, Tab moved focus to Expiration and the footer while the body stayed scrolled, so the focused control sat behind the footer. A newly focused control is now scrolled into view.
7. **An alert without a message raised an empty toast.** The toast now names the alert, followed by the message when there is one.

Each fix has a regression test. The test for #6 was seen failing before its fix; the others were written before their fixes but were not run red first. App: 2,268 passed, 26 ignored. Clippy and guards are clean.

## Matrix

- [x] **Opening.** Alerts opens from the global menu (⋮ → Alerts…), from the manager (+ Create) and from the selected drawing (context bar "Create alert", which prefills Rectangle 12). Seen in all four cells.
- [x] **Conditions.** Value, line, channel and rectangle targets work. Operators match the catalogue: value 5; line 5; channel 4; rectangle 6, with Above upper / Below lower boundary. Two conditions with All, and Remove condition, work.
- [x] **Keyboard.** Tab and Shift+Tab work, with a visible focus ring. Combos open with Space and are driven by arrows and Enter. The fixed footer is reachable by Tab; an alert was created with Tab then Space.
- [x] **Editor fields.** Cadence and expiration (presets, Custom, an invalid date and a past date) work, as do a multi-line message and the popup and toast checkboxes. Name and values are kept across the Conditions, Message and Notifications tabs.
- [~] **Sound preview.** Preview was invoked with no failure toast; the code shows one when the sound cannot play. Nobody listened, so whether it was audible is not verified.
- [x] **Invalid fields.** An invalid date and a past expiration disable Create and show the reason.
- [x] **Missing target.** The row reads "Suspended: drawing is missing", the editor shows "Drawing 12 (unavailable)", and Locate shows "Alert target is missing from the open workspace". Retargeting through Edit, then saving, recovered the alert.
- [x] **Honest suspension states.** Rows read "Suspended: replay is paused" and "Suspended: price is outside the drawing's time range".
- [ ] **Storage-error state.** Not exercised with native input in this pass.
- [x] **Edit and lifecycle.** Edit + Save changes, Pause, Resume and Rearm all work.
- [x] **Delete confirmation.** It offers "Delete this alert? History is kept." with focus on Keep alert. Escape cancels only the confirmation; Confirm delete removes the row.
- [~] **Stale or cancelled actions.** The cancelled confirmation left the alert intact. A stale revision was not exercised natively.
- [x] **Expired and completed.** An expiry set two minutes ahead became Expired, with its action disabled and Edit still offered. Once-alerts became Completed, offering Rearm.
- [x] **History.** History shows the price, revision and Delivered, and keeps the event after the definition is deleted.
- [x] **Focus is not stolen.** Typing "focustest" into Search continued unbroken while an alert fired, raising its popup.
- [x] **Toast.** Seen natively naming the alert (dark-short).
- [x] **Scrolling and Escape.** The wheel scrolls the editor and manager bodies in short windows. Escape nests: popup, then draft, then manager, then closed.
- [~] **Focus restoration.** Focus after closing was not checked.
- [x] **Toolbar, inspector and handles.** The context bar no longer covers the editor (fix #1). The inspector was not opened during this pass.

## Observations left for the trader

- One Escape with no popup open discards a modified draft without asking. This is by design (Escape = Cancel), but it cost a test draft.
- The editor is anchored at the centre, so it moves when its content height changes. Controls shift under the cursor, and one test click landed on the wrong tab.
- History stamps replay events with arrival (wall-clock) time: 16:51 UTC for a 09:05 replay print. Should replay alerts use market time?
- In a short window the context bar and "Create alert" overlap the flow legend, and a toast covers the 10×/20× replay buttons for 8 s. Both placements predate this feature.
- A popup for an alert without a message shows only its title. Ctrl+Z after deleting the target rectangle did not restore it; focus was probably not on the chart, and this was not investigated.
