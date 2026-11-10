# Alert interface validation receipt

Implementation commit: `485a77bb802255c0dc420137e39997ac7fda1051` (tree `43a4ed82a0025b6b17b6c4d6fb8f36a3b6f2e0e5`). Integration base: `839789cbc89c08343bc0ce889f30ea133e75d92b`. Subsequent evidence-only commits do not alter the tested source.

The ordered CONTRIBUTING loop passed: formatting, workspace clippy with all targets, workspace build, and workspace tests. App: 2,265 passed, zero failed, 26 ignored; downstream tests and doctests passed. Final guards passed. Every one of the 2,222 build-input hashes matched the stable source manifest before commit, during independent review and after evidence collection. Raw logs and manifest are in `.artifacts/validation/repaired-*`.

Independent HIGH review round two found no remaining code blockers. It closed truthful storage-readback and operator-compatibility findings; the compact toolbar correction also has passing native rendered evidence. Overall review remains FAIL because C11 native interaction evidence is incomplete. The two-round review budget is exhausted; no successful review marker or PR was created. Raw independent report: `.artifacts/review/high-review-485a77bb.md`.

Exact immutable harness: `.artifacts/alerts-ui-native/bin/quantick-alerts-485a77bb.exe`, SHA-256 `457E82EE5087E63C6DFEB0A12852F0C53F4C29ABA5316526BA456718B4960122`; source/build mapping `.artifacts/validation/harness-485a77bb.json`. Its freshly dumped hook registry matches the committed registry.

- [Designer evidence](alerts-designer-review.md): 14 final native captures, including all four toolbar cells. Minor short-window error-toast overlap remains documented.
- [Independent consumer](alerts-agent-consumer.md): final supported-schema and admitted local transport checks pass, including explicit removal permission, stale revisions, retries and history retention. The fixed-scope MCP adapter cannot request deletion; a supported local client with explicit scope and app grant can. Optional MCP opt-in is follow-up work.
- [Simulated trader review](alerts-trader-review.md): persona feedback and inspected final captures, not observed trader input.
- [Native performance](alerts-native-performance.md): historical results remain separate from the exact-final matched refresh and its host-load limits.
- [Pending interaction matrix](alerts-native-interaction-checklist.md): independent native mouse/keyboard/scroll/Escape/focus workflows still required for C11.

The supported Windows input helper previously failed with a missing pipe (OS error 2). Static images, socket calls and unit input injection are not substitutes. No acceptance-ready claim is made. The concrete next action is to restore the supported helper and complete the pending matrix on the identified executable.
