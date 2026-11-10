# Pending native alert interaction checklist

Test the immutable executable `.artifacts/alerts-ui-native/bin/quantick-alerts-485a77bb.exe`, built from commit `485a77bb802255c0dc420137e39997ac7fda1051`. SHA-256: `457E82EE5087E63C6DFEB0A12852F0C53F4C29ABA5316526BA456718B4960122`. Build/source mapping: `.artifacts/validation/harness-485a77bb.json`.

C11 is **not passed**. The supported Windows native input helper previously failed with a missing pipe (OS error 2). Run this checklist when that supported service is available. Static captures, admitted socket commands and injected unit-test pointer/key events do not satisfy it. Use isolated fixture stores; no live orders or user-store edits.

An independent interaction tester must repeat the matrix in light/dark themes and normal/short viewports, recording the exact executable hash, input operations, captures and observed results:

- [ ] Open Alerts through the global menu, manager and selected drawing; create value/line/channel/rectangle conditions, verify compatible operators and All conditions.
- [ ] Navigate fields and subviews with keyboard; set cadence, expiry, message and notification options; preview the existing sound; confirm draft preservation and reachable fixed footer.
- [ ] Exercise invalid fields, missing targets and storage-error state; verify visible reasons and recovery guidance without false empty success.
- [ ] Edit and save; pause/resume; confirm deletion; verify stale or cancelled actions do not mutate the wrong alert.
- [ ] Verify expired/completed states, immutable History after edits/deletion, and a firing notification without stolen focus.
- [ ] Exercise scrolling, Tab/Shift+Tab, Escape nesting and focus restoration in short windows; confirm toolbar/inspector and rectangle handles remain accessible.

Record failures as failures and attach evidence. No acceptance-ready claim or successful review marker is permitted while this matrix remains incomplete.
