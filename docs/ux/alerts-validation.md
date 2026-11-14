# Alert interface validation receipt

Implementation commit: `485a77bb802255c0dc420137e39997ac7fda1051` (tree `43a4ed82a0025b6b17b6c4d6fb8f36a3b6f2e0e5`). Integration base: `839789cbc89c08343bc0ce889f30ea133e75d92b`. Subsequent evidence-only commits do not alter the tested source.

The ordered CONTRIBUTING loop passed: formatting, workspace clippy with all targets, workspace build, and workspace tests. App: 2,265 passed, zero failed, 26 ignored; downstream tests and doctests passed. Final guards passed. Every one of the 2,222 build-input hashes matched the stable source manifest before commit, during independent review and after evidence collection. Raw logs and manifest are in `.artifacts/validation/repaired-*`.

Independent HIGH review round two found no remaining code blockers. It closed truthful storage-readback and operator-compatibility findings; the compact toolbar correction also has passing native rendered evidence. Overall review remains FAIL because C11 native interaction evidence is incomplete. The two-round review budget is exhausted; no successful review marker or PR was created. Raw independent report: `.artifacts/review/high-review-485a77bb.md`.

Exact immutable harness: `.artifacts/alerts-ui-native/bin/quantick-alerts-485a77bb.exe`, SHA-256 `457E82EE5087E63C6DFEB0A12852F0C53F4C29ABA5316526BA456718B4960122`; source/build mapping `.artifacts/validation/harness-485a77bb.json`. Its freshly dumped hook registry matches the committed registry.

- [Designer evidence](alerts-designer-review.md): 14 final native captures, including all four toolbar cells. Minor short-window error-toast overlap remains documented.
- [Independent consumer](alerts-agent-consumer.md): final supported-schema and admitted local transport checks pass, including explicit removal permission, stale revisions, retries and history retention. The fixed-scope MCP adapter cannot request deletion; a supported local client with explicit scope and app grant can. Optional MCP opt-in is follow-up work.
- [Simulated trader review](alerts-trader-review.md): persona feedback and inspected final captures, not observed trader input.
- [Native performance](alerts-native-performance.md): historical results remain separate from the exact-final matched refresh and its host-load limits.
- [Native interaction matrix](alerts-native-interaction-checklist.md): C11, run with real Windows input on 2026-11-10.

## C11 native interaction (2026-11-10)

The supported helper still failed with a missing pipe (OS error 2). C11 was therefore run with guarded `SendInput` instead: every mouse, wheel and key event was sent only after checking that the isolated demo held the foreground and the cursor was inside it. The executables, captures and input logs are in the checklist.

- **Defects found and fixed.** The pass found seven defects, all fixed in commits `03b2fdf4`, `3e7232dd`, `44e19bb9` and `5858a812`, each with a regression test: the context bar covered the editor; the summary showed debug names; the operator popup hid choices; the expiration error contradicted its preview; a search claimed there were no alerts; Tab focus landed out of view; a toast came up empty.
- **Rechecked natively.** All seven fixes were rechecked with native input on the rebuilt executables.
- **Checks after the fixes.** On the final tree, app tests: 2,268 passed, 26 ignored. Clippy and guards are clean.

The matrix now passes, except for these open items:

- The storage-error state was not driven natively.
- A stale-revision action was not driven natively.
- Whether the preview sound was audible is unverified.
- Focus restoration after closing was not checked.
- The inspector was not opened.

**Review gap.** These fixes came after the exhausted two-round review budget and have had no independent review. No review marker was written and no acceptance-ready claim is made. Whether the post-review fixes need one more review, and the observations at the end of the checklist, are for the trader to decide.
