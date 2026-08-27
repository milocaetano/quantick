# Mission: evidence bundles (roadmap 5.4 / plan PR 5c)

**One sentence.** Ship the control plane's evidence bundle — one coherent,
redacted, integrity-hashed in-memory capture of the semantic snapshot, the
event page, the health metrics and the semantic scene, retained under the
declared `CONTROL_EVIDENCE_*` bounds and read back as a paginated retained
resource, with an optional screenshot stamped with the scene's own capture
revision — and prove it end to end through the live control plane from an
existing validation skill.

**Branch:** `feat/control-evidence`, cut from `origin/main` (13e6f9c).
**Worktree:** `../quantick-worktrees/feat-control-evidence`.
**Rate class:** on-demand captures. Nothing runs per trade, per depth update
or per frame; the frame pays only for the screenshot harvest, and only while
one is armed.

Everything below is a criterion. A criterion without evidence is unmet.

## Mission criteria (roadmap 5.4, acceptance 1-7)

1. **An agent explains the running session without a screenshot.** A headless
   test connects through the live gateway, calls the evidence capture, reads
   the bundle back through its cursor, and the reassembled document carries
   instance and session identity, version/commit/protocol, OS and graphics
   backend, the coherent capture revision, and the workspace, chart, feed,
   health and scene projections — with no image involved.
2. **Feed, replay, indicator and connection changes appear through the
   cursor.** The bundle's event page carries them, and its `next_cursor` is a
   cursor `events.read` continues from (test reads past it).
3. **The bundle reports omitted information and coverage gaps.** Omitted
   scopes, unavailable fields, inferred data and an explicit
   `not_captured` list, every reason a stable code and never a rendered
   sentence.
4. **A bundle with a screenshot maps every named control to a region of the
   image.** The image carries the same capture revision as the scene it was
   taken with, and every scene control that reports bounds resolves to a
   rectangle inside the image (test).
5. **At least one existing validation skill reads and asserts through the live
   control plane.** `ui-harness` gains the control-plane read path and the
   `QUANTICK_CONTROL_EVIDENCE` hook that reaches a capture from a fresh
   launch; `visual-qa` asserts structured live state instead of relying only
   on window captures.
6. **Redaction: no token, user path, user drawing text or config key in the
   bundle.** A test plants canaries of each kind, captures, and hunts them in
   the encoded bundle and its manifest.
7. **Retention and size bounded by named constants.** Eviction by the earlier
   of `CONTROL_EVIDENCE_MAX_BUNDLES`, `CONTROL_EVIDENCE_MAX_TOTAL_BYTES` and
   `CONTROL_EVIDENCE_RETENTION_MS`; overflow and expiry answer with
   `control.resource_gone` / `control.backpressure` from the existing
   vocabulary, never a new code.

## Standard gates

8. **Every artifact in English** — code, comments, tests, docs, commits, PR.
   Graded by `arch-review` dimension 8, enforced by `language_guard.rs`.
9. **Four checks green** after rebasing on latest `main`: `cargo fmt --all --
   --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo build --workspace`, `cargo test --workspace`.
10. **Performance declared by rate class in the plan, not the review.** No
    request, no per-frame cost — proved by test; the capture stays inside
    `CONTROL_UI_BUDGET_US` under the existing budget guards.
11. **Schemas and catalog regenerated and versioned**, snapshot tests green,
    no egui type on the wire.
12. **Second operator.** The capture and the resource read are named calls
    with registered IDs, schemas and readable results, discoverable through
    `describe`; the MCP adapter exposes `quantick_capture_evidence` and the
    tool-list guard counts it.
13. **`ui-harness` hook for every new surface**, added in this change, with
    its row in the skill's table. `visual-qa` / `trader-ux-review` only with
    the owner's authorization to launch the app — otherwise BLOCKED in
    writing in the PR body, never skipped in silence.
14. **`arch-review` run** over `git diff main...HEAD` with every Blocker and
    Should-fix resolved, or deferred explicitly in the PR body; step 0's
    `code-review` runs first.
15. **The pull request is open**, CI green, evidence in its body in the mould
    of #221-#223. Merging is not part of the mission.

## Out of scope

Disk export (a cockpit action, PR 6), any market or financial write, the
authority layer of plan section 9.2, and retiring production `ui-harness`
hooks (that starts with the matching PR 6/PR 7 actions).
