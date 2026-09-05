# Mission: `gateway.rs` splits along the thread boundary

Split `crates/app/src/control/gateway.rs` (4,567 lines) into sibling modules
under `crates/app/src/control/gateway/` along the seam that already exists in
the code — the thread boundary — leaving `gateway.rs` as the UI-thread host.

**Why it matters:** the gateway is the file the threat model and the control
contract point at, and the one a reviewer asking *what can a client make the UI
do?* has to read whole. Today the answer is spread across a 1,930-line `impl`
and a 1,370-line run of free functions on the other thread, with the egui panel
and the screenshot service in between. After the cut, `server.rs` is the file an
auditor reads for the wire and `gateway.rs` for what the UI grants.

**Tier:** `medium`. A ~2,600-line move along a thread boundary, with a grep pair
that proves the seam and a wire contract that must not move a byte. It earns
more than `small` because the diff is far past the `small` ceiling and because
the contract tests make a silent behaviour change expensive; it is not `high`
because no behaviour is designed here — every body moves unchanged.

## Request ledger

- **R1** — Create `control/gateway/server.rs`: the gateway thread's free
  functions and private structs (`gateway.rs:2770-4142`), declared by a `mod`
  line in `gateway.rs`.
- **R2** — Create `control/gateway/semantic.rs`: the semantic-change diff
  (`:1495-1865`) and its key structs and helpers (`:2578-2768`), as an
  `impl ControlAccess` block plus the structs.
- **R3** — Create `control/gateway/screenshot.rs`: the screenshot service
  (`:1126-1486` plus the `*_for_test` accessors `:794-812`), as an
  `impl ControlAccess` block. The brief leaves one option open — "`execute_on_ui`
  and `begin_frame` are the frame service and may stay with the host if the
  mission judges them not screenshot-specific — say which" — and the mission
  says which, in the PR body.
- **R4** — Create `control/gateway/panel.rs`: `menu_label`, `draw_panel`,
  `draw_panel_body` (`:1988-2242`) and the two panel constants (`:81-82`).
- **R5** — Move both inline test modules (`:4144-4567`) to
  `control/gateway/tests/mod.rs` as two nested `_tests` modules, bodies
  untouched; `gateway.rs` ends with `#[cfg(test)] mod tests;`.
- **R6** — `gateway.rs` remains the host: "options, identities, lifecycle,
  local actions and frame service".
- **R7** — "Bodies unchanged, names kept." `git diff --color-moved=zebra`
  shows moves; every `pub(super)` and the one re-export listed in the PR body.
- **R8** — `runtime_id_bytes` moves with the server and is re-exported from the
  host: `pub(crate) use server::runtime_id_bytes;`.
- **R9** — "The seam is the thread": after the move,
  `grep -nE 'egui|eframe' control/gateway/server.rs` is empty, and
  `grep -nE 'TcpStream|TcpListener' control/gateway.rs` finds only the
  `request_enable` spawn and the types it hands over. Both greps in the PR body.
- **R10** — Docs: path references that point at the accept loop or dispatch
  updated to `gateway/server.rs`; "no new prose". Check `docs`,
  `.claude/skills` and `AGENTS.md`.
- **R11** — Baselines: run `--tighten`; each new file under 1,500 production
  lines. "`server.rs` at ~1,370 lines on disk is close to the threshold once
  doc comments count — if it crosses, split the connection session and the
  dispatchers from the accept loop rather than sign a raise."
- **R12** — `gateway.rs` at most 1,700 lines.
- **R13** — The size `!budget` lower by at least 2,300.
- **R14** — `--report` before and after: only `gateway.rs`-related lines and the
  new files move; no new file appears under `file.largest`.
- **R15** — Generated hook registry and capability inventory unchanged.
- **R16** — `cargo test -p quantick-app control` and `cargo test -p quantick-app
  gateway` run the same number of tests as before the move.
- **R17** — `sh .claude/hooks/guardrails_test.sh` green if any hook script names
  the gateway path — "check first".
- **R18** — Out of scope, and therefore an ask in its own right: "Any change to
  limits, scopes, the handshake, the rate limiter or a response's bytes" is
  forbidden; `crates/app/src/control/contract.rs` and the bridge tests must not
  change.
- **R21** — Out of scope, second entry: do not move the gateway to
  `quantick-control` — "it drives egui and a thread".
- **R22** — Out of scope, third entry: leave `invoke_local_action` (164 lines)
  and `service_replay_trace` where they are — "the host's own, and the next
  shape question, not this move".
- **R19** — "Every claim was measured against `origin/main` at `cc4c92f`… each
  carries its `file:line` so the mission re-checks it instead of trusting it."
  Verify evidence-ledger claims #1–#12 before acting; report every correction.
- **R20** *(purpose, and the ask that judges the others)* — "After the cut,
  `server.rs` is the file an auditor reads for the wire, `gateway.rs` for what
  the UI grants." The split must land on the thread boundary, not on a
  line-count convenience.

## Decisions taken by the trader

None. The interrogation round found nothing a wrong guess would throw work away
over; the live doubts are recorded as `S1`–`S4` below.

## Assumptions

- **S1** — `execute_on_ui` and `begin_frame` **stay in `gateway.rs`**. The
  request's own words list "frame service" among what the host keeps (R6),
  which settles the option the evidence ledger left open in R3. They are reached
  by paths beyond the screenshot service, so moving them would make
  `screenshot.rs` a dependency of unrelated host code. Safe to assume: the
  request answers it, and the PR body says which way it went.
- **S2** — The archival hits for `gateway.rs` — everything under
  `docs/control-plane/history/`, and `docs/evidence/*/report-*.txt`, which are
  recorded tool output — are **not** updated. They record what was true when
  they were written; rewriting them would falsify a record. Safe to assume: R10
  asks for "no new prose" and repo convention treats history as history.
  Measured: no live doc (`control-contract.md`, `observer-threat-model.md`,
  `adr-0001`) names `gateway.rs` by path, so evidence-ledger claim #12 is false
  as it stands and R10 has no live target. Recorded as a correction, not a gap.
- **S3** — If `server.rs` crosses 1,500 production lines it is split as R11
  prescribes (connection session and dispatchers away from the accept loop),
  with no baseline raise signed. Safe to assume: the request prescribes the
  remedy; only whether it triggers is unknown, and measuring answers that.
- **S4** — Measured against `origin/main`, not the brief's `cc4c92f`, and
  re-measured after each move: the budget read `52139` in the brief, `50996` at
  `62c8730`, and `48185` at `0c3431d` once #307 merged mid-review. R13's "lower
  by at least 2,300" is applied to whichever number is current. Safe to assume:
  R19 instructs re-measurement over trust, and the assumption is what made the
  mid-review merge a re-run rather than a wrong number.

## Acceptance criteria

- [ ] **A1** — `control/gateway/server.rs` exists and holds the gateway thread's
      free functions and private structs, bodies unchanged; `gateway.rs`
      declares it with a `mod` line.
      *Evidence:* `git diff --color-moved=zebra --stat`; the file listing.
      → PR body. *(R1, R7)*
- [ ] **A2** — `control/gateway/semantic.rs` exists and holds the
      semantic-change diff and its key structs, bodies unchanged.
      *Evidence:* the same diff; the moved names listed.
      → PR body. *(R2, R7)*
- [ ] **A3** — `control/gateway/screenshot.rs` exists and holds the screenshot
      service, bodies unchanged, and the PR body states explicitly whether
      `execute_on_ui` and `begin_frame` moved or stayed, with the reason.
      *Evidence:* the file; a named sentence in the PR body.
      → PR body. *(R3, R7, S1)*
- [ ] **A4** — `control/gateway/panel.rs` exists and holds `menu_label`,
      `draw_panel`, `draw_panel_body` and the two panel constants.
      *Evidence:* the file; `grep -nE 'egui' control/gateway.rs` showing the
      panel lines gone from the host. → PR body. *(R4, R7)*
- [ ] **A5** — Both test modules live in `control/gateway/tests/mod.rs` as
      nested `_tests` modules, bodies untouched; `gateway.rs` ends with
      `#[cfg(test)] mod tests;`.
      *Evidence:* the file; the test-count comparison of A11.
      → PR body. *(R5, R7)*
- [ ] **A6** — `gateway.rs` retains options, identities, lifecycle, local
      actions and the frame service, and is at most 1,700 lines.
      *Evidence:* `wc -l crates/app/src/control/gateway.rs`.
      → PR body. *(R6, R12)*
- [ ] **A7** — The thread seam holds: `grep -nE 'egui|eframe'
      control/gateway/server.rs` returns nothing, and `grep -nE
      'TcpStream|TcpListener' control/gateway.rs` returns only the
      `request_enable` spawn and the types handed over.
      *Evidence:* both commands with their output, quoted.
      → PR body. *(R9, R20)*
- [ ] **A8** — `runtime_id_bytes` lives in `server.rs` and is re-exported by
      `gateway.rs`; every other cross-file path reference still resolves.
      *Evidence:* `cargo check -p quantick-app --all-targets` exit 0; the
      re-export line quoted. → PR body. *(R8)*
- [ ] **A9** — Baselines tightened: `--tighten` run, every new file under 1,500
      production lines, no raise signed, and the `!budget` lower by at least
      2,300 against the measured start (50,996, then 48,185 after #307).
      *Evidence:* `git diff crates/guards/size-baseline.txt`.
      → PR body. *(R11, R13, S3, S4)*
- [ ] **A10** — `--report` before and after differ only in `gateway.rs`-related
      lines and the new files; no new file appears under `file.largest`.
      *Evidence:* the `diff` of the two reports.
      → `docs/evidence/gateway-rs-sidecars/report-before.txt`,
      `report-after.txt`, and the diff in the PR body. *(R14)*
- [ ] **A11** — `cargo test -p quantick-app control` and `cargo test -p
      quantick-app gateway` run the same number of tests as on `origin/main`,
      and the generated hook registry and capability inventory are unchanged.
      *Evidence:* the two test counts before and after; an empty `git diff` over
      the generated files. → PR body. *(R15, R16)*
- [ ] **A12** — `crates/app/src/control/contract.rs` and the bridge tests are
      untouched, and no limit, scope, handshake, rate-limiter or response byte
      changed.
      *Evidence:* `git diff --stat origin/main...HEAD` showing neither path.
      → PR body. *(R18)*
- [ ] **A13** — Every evidence-ledger claim #1–#12 re-checked against the
      branch's own `origin/main`, with each correction reported rather than
      silently absorbed.
      *Evidence:* a corrections list. → PR body. *(R19)*
- [ ] **A14** — Doc and skill path references are correct after the move: any
      live reference pointing at the accept loop or dispatch says
      `gateway/server.rs`, and archival records are left alone, with that choice
      stated. No new prose.
      *Evidence:* `grep -rn 'gateway\.rs' docs .claude/skills AGENTS.md` with
      each hit classified. → PR body. *(R10, S2)*
- [ ] **A16** — Neither out-of-scope move happened: nothing left `crates/app`
      for `quantick-control`, and `invoke_local_action` and
      `service_replay_trace` are still in `gateway.rs`, in no sidecar.
      *Evidence:* `git diff --name-only origin/main...HEAD` touching only
      `crates/app`, `crates/guards` and `docs/evidence`; `grep -n` finding both
      functions in the host and `grep -rn` finding neither under `gateway/`.
      → PR body. *(R21, R22)*
- [ ] **A15** — The split lands on the thread boundary, not on line count: an
      auditor reading `server.rs` sees the whole wire path and nothing of the
      UI's grants, and reading `gateway.rs` sees the grants and no socket loop.
      *Evidence:* A7's greps plus `arch-review`'s verdict on the seam.
      → PR body. *(R20)*

### Injected gates

- [ ] **G1** — Every artifact in English, per `CLAUDE.md`.
      *Evidence:* `arch-review` dimension 8 verdict; `cargo test -p
      quantick-guards`. → PR body.
- [ ] **G2** — The four checks green after rebasing on latest `main`:
      `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
      `cargo build --workspace`, `cargo test --workspace`. Each run on its own.
      *Evidence:* the four exit codes. → PR body.
- [ ] **G3** — Performance impact declared: every touched path classified by
      rate (per-trade / per-depth / per-frame / rare) as part of the plan.
      *Evidence:* the classification. → PR body.
- [ ] **G4** — `arch-review` run over `git diff origin/main...HEAD`, its step 0
      `code-review` bug pass at `low` included, every Blocker and Should-fix
      resolved or deferred with severity.
      *Evidence:* the review verdict and the resolution list. → PR body.
- [ ] **G5** — `cargo test -p quantick-guards` green, and
      `sh .claude/hooks/guardrails_test.sh` run if any hook script names the
      gateway path (checked: none does — recorded, not skipped silently).
      *Evidence:* exit codes, or the grep proving non-applicability. → PR body.

### Not applicable, and why

- **Hot path** — the gateway adds no per-trade or per-depth work by design
  (`observer-threat-model.md:253`), and this change moves bodies between files
  without altering a call. No `APP_HEALTH_SUMMARY` run is owed; G3 still
  declares the classification.
- **User-visible surface** — `panel.rs` moves the egui panel's code but changes
  no pixel: the same functions, called from the same place, with unchanged
  bodies. `ui-harness` / `visual-qa` / `trader-ux-review` are not owed. Had a
  single line of `draw_panel_body` changed, they would be.
- **Adds a capability** — nothing is added; `new-extension` does not apply.
- **Adds something a trader does** — no new action, tool, trade or lock.
- **Engine / determinism** — `crates/app` is not the engine, and no bar-building
  code is touched. Test-first does not apply to a pure move; the existing tests
  are the golden ones and must keep passing at the same count (A11).
- **Docs/skills only** — this is a code change; the full shape pass applies.

### Closing steps

- **C1** — `delivery-review` returns PASS.
- **C2** — The PR is open, with the tier named beside the four verification
  boxes.

## Evidence as measured

Recorded at commit `23ba851`, rebased onto `origin/main` at `0c3431d` after
`refactor/tab-rs-sidecars` merged as #307. That branch and this one meet only
at the `!budget` line, exactly as the brief's parallel-work note predicted; the
figures below are against the rebased base.

| | Result |
| --- | --- |
| **A1-A5** files | `server.rs` 1,445 / `semantic.rs` 591 / `screenshot.rs` 278 / `panel.rs` 278 / `tests/mod.rs` 450 lines |
| **A1-A5, A7** bodies unchanged | line-multiset comparison of the original file against all six: every difference is an import line, a module header, an `impl` wrapper, or one of exactly **14 widenings** (13 `pub(super) fn` plus `pub(super) struct SemanticBaseline`), each enumerated. No body line lost |
| **A6** host size | `wc -l crates/app/src/control/gateway.rs` = **1,680** (1,676 production), 20 under the ceiling; keeps options, identities, lifecycle, local actions and the frame service |
| **A3** the open question | `execute_on_ui` and `begin_frame` **stayed in `gateway.rs`** (S1): both run every request, not only the ones that want pixels |
| **A7** seam grep 1 | `grep -nE 'egui\|eframe' control/gateway/server.rs` -> **no match** (exit 1) |
| **A7** seam grep 2 | `grep -nE 'TcpStream\|TcpListener' control/gateway.rs` -> exactly two lines: the `net::` import and `TrackedSocket.stream`, the type handed across. No `TcpListener` |
| **A8** re-export | `pub(crate) use server::runtime_id_bytes;`; the six cross-file paths of ledger #7 re-measured at 7/1/2/2/3/1 and all resolve |
| **A9** ratchet | ceiling 4,142 -> 1,696; `!budget` 48,185 -> 45,739, a fall of **2,446** (asked: >= 2,300). No new baseline entry, no raise signed; every sibling under the 1,500 threshold (`server.rs` closest, at 1,445) |
| **A10** report | only `gateway.rs`-related lines moved -- see the deviation below |
| **A11** tests | `control` 148 before / 148 after; `gateway` 36 / 36; app suite 1,899 passed both on the branch and on `origin/main`. No generated file in the diff |
| **A12** contract | `git diff --stat origin/main` names neither `contract.rs` nor any bridge test |
| **A14** docs | every `gateway.rs` reference is archival -- see the corrections below |
| **A16** out-of-scope moves | `git diff --name-only` touches only `crates/app`, `crates/guards`, `docs/evidence` and the archive -- `crates/control` untouched; `service_replay_trace` at `gateway.rs:678` and `invoke_local_action` at `:835`, and `grep -rn` finds neither under `gateway/` |
| **G2** four checks | `fmt` 0, `clippy --workspace --all-targets` 0, `build --workspace` 0, `test --workspace` 0 (each run on its own) |
| **G5** guards / hooks | `cargo test -p quantick-guards` green; `guardrails_test.sh` 111 passed, 0 failed |

### Corrections to the brief's evidence ledger (R19, A13)

Claims #1-#11 all re-measured true. Two corrections:

1. **Claim #12 is false as stated.** It says the threat model and the control
   contract name `gateway.rs` by path. They do not: `control-contract.md`,
   `observer-threat-model.md` and `adr-0001` speak of "the gateway" as a
   component and never give a file path. Every hit for `gateway.rs` is
   archival -- five files under `docs/control-plane/history/`, one recorded
   measurement in `.claude/skills/arch-review/references/docking.md`, and
   recorded `--report` output under `docs/evidence/`. None is updated: they
   record what was true when written. So **R10 had no live target**, and the
   split leaves no stale path behind.
2. **The budget had moved, twice.** The brief measured `!budget 52139` at
   `cc4c92f`; `origin/main` was at `62c8730` with `50996`; and #307 landed
   during the review, taking it to `48185`. The 2,446 fall is against whatever
   the measured number is, which is the point of re-measuring rather than
   trusting (S4). It now reads 48,185 -> 45,739.

### Deviations, stated rather than absorbed

- **`elapsed_us_since` stays in `server.rs`** (the brief's range #2), and
  `screenshot.rs` imports it by path. It was briefly moved to the host on the
  theory that a helper both children use belongs to the parent; measurement
  showed the host never calls it, so the parent was the wrong home. The cost
  is one import from the UI side into the server file -- an arithmetic helper,
  not a socket, so the seam greps are unaffected.
- **`file.largest` gains a line** that A10 did not predict:
  `crates/app/src/footprint_render.rs 2236`. It is a fixed top-8 list
  (`report.rs:204`), so `gateway.rs` leaving the top eight promotes the
  ninth-largest file into view. `footprint_render.rs` is untouched by this
  branch and sits at its existing baseline. No file this branch created or
  grew entered the list -- the four siblings are far below it -- so A10 holds
  in substance, and the extra line is reported rather than glossed.
- **`server.rs` names its imports rather than globbing them**, found by this
  mission's own shape pass: every other column-0 `use super::*` in the
  workspace is in a `tests/` path, and a glob would have made the seam claim
  unprovable by reading. Naming them moved 21 host imports that only the
  gateway thread used out of `gateway.rs`, which is why the host lands at
  1,680 with headroom rather than exactly on its ceiling, and why `server.rs`
  grew from 1,400 to 1,445.

### One test failure, diagnosed

The first `cargo test --workspace` run failed
`app::tests::control_plane_tests::gateway_a_client_that_never_reads_does_not_stall_another`
-- a socket-timing test in a file this branch never touches. Measured rather
than assumed: it passes 3/3 alone, 4/4 across full app-suite runs, and the
second full-workspace run was green. `origin/main` runs the same 1,899 tests.
It reproduces only under full-workspace contention, matching the known
CPU-contention flake, and is not caused by this change.

## The request as received, verbatim

> Attributed quotation of the trader's request, reproduced under the language
> rule's exemption for a marked, attributed quotation.

```
/mission medium refactor/gateway-rs-sidecars — crates/app/src/control/gateway.rs is 4,567 lines (4,142 production) and holds both sides of the control plane's socket in one file: the UI-thread host `impl ControlAccess` (`gateway.rs:632-2572`) and the gateway thread's free functions (`:2770-4142`: `gateway_run`, `accept_loop`, `connection_session`, `dispatch_prepared`, `dispatch_parked_wait`, the response encoders), plus the semantic-change diff with its key structs (`:1495-1865` and `:2578-2768`), the screenshot service (`:1126-1486`), a 232-line egui panel (`:1997-2242`) and two inline test modules (`:4144-4567`). Move each into a sibling under crates/app/src/control/gateway/ — `server.rs`, `semantic.rs`, `screenshot.rs`, `panel.rs`, `tests/mod.rs` — leaving `gateway.rs` as the host: options, identities, lifecycle, local actions and frame service. Bodies unchanged, names kept, ceiling tightened, budget lowered. Read C:\src\mission-gateway-rs-sidecars.md in full before anything else and build the request ledger from it.
```

The mission brief at `C:\src\mission-gateway-rs-sidecars.md`, read in full before
any action, is the request's second half: its evidence ledger (#1–#12), scope
(1–6), acceptance criteria, out-of-scope list and parallel-work note are carried
into `R1`–`R20` above.
