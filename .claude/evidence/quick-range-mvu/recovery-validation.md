# Recovery validation and preliminary finding MVU-P1

2026-09-15: root resumed after the trader enabled full filesystem permissions.
Checkpoint44 transfers ownership from the disappeared implementation agent.
The86 input hashes in candidate-inputs-v3.json matched before any new edits;
the prior full ordered PASS remains evidence for that pre-repair input only.

Feature validation was rerun because interrupted session36372 had no surviving
session/process and no terminal receipt. Independent checklist agent reported
exit0:2074passed,0failed,11ignored,39.98s. Feature test executable4895c18869080ee5
SHA256 E541A7D71E905B51BB1F0527259F873315C664389879C84B2422E871819DB42B.
Its fresh app build completed exit0 in2m50s, SHA256
6040D6D1466BB2CE664D3F4019DBB303FDFF357F5A9DD67F51FB162725AD5748.
Both predate the repair below and are not final-head validation/captures.

## MVU-P1: canonical fractional slot identity

Independent preliminary reviewer mvu_pre_review found that exact annotation
used floor while the viewport's existing canonical rule is(bar-0.5).ceil.
At0.75 it reported slot0 with slot1's timestamp; the left half of the first
future slot on an alternative chart was refused as AnchorMismatch.
This is a required correctness fix, not a score or final review finding.
Initial attempt1; no prior formal PR repair batch exists for this mission.

Test-first receipt: cargo test -p quantick-chart-interaction exact_fractional_position
-- --nocapture failed before the repair: position0.75 produced Some(0), expected
Some(1). Exit101,0passed/1failed,13filtered, compile0.77s. The fixture follows
the actual series slot convention and retains duplicate timestamps.

Repair moves the existing pure canonical coordinate rule into chart-interaction
and delegates Viewport::slot_of to it. Both exact validation and the rendered
series now use the same rule; no second floor/round implementation remains.
The app's existing viewport tests remain. Added all-three-action integration
coverage for fractional market/future anchors and readback.
Core suite after repair:14passed/0failed, plus doc tests, exit0.
Guard report: UI-free46907 <= unchanged46911; headless0; graph30edges;
unreadable/undecodable/blind/failed scans all0. Report is not a full guard PASS.
Current full ordered checks, feature revalidation, visual/performance and
independent final PR reviews remain due after the final code edits.

App MVU-P1 regression session76528 completed exit0:1passed,0failed,2084filtered,
0.24s after3m48s compilation. Independent reviewer recompiled actual headless
source with rustc --test:14PASS and shared-law delta resolved; not final PR approval.

## MVU-P2: preserve exact coordinates across serialization

Root identified and the independent reviewer reproduced a second issue in
quick_range_input: three-decimal serialization maps0.5004 to0.5. With distinct
timestamps the corrected resolver refused the request; with duplicates it
silently reported slot0 instead of1. The independent executable used the actual
core and existing rust_decimal dependency, not an invented runtime.
Initial attempt1, no formal PR batch. The fix uses canonical shortest f32
decimal text and validates the existing parser's exact numeric roundtrip.
Unrepresentable coordinates are refused, not rounded. Future readback uses the
same helper. An all-three-action app regression serializes through the real
quick_range_input with both distinct and duplicate timestamps and checks stored
positions/readback by f32 bits. Control helper regression session39686:exit0,
1passed,24filtered,22.79s compile. Other filtered suites ran zero tests and are
not claimed as full validation. Full ordered loop64485 is now running.

## Fourth447 regression evidence remains open

Independent baseline agent ran the current candidate tape-lane test verbatim
against retained9ff57501. It PASSED (exit0,1passed,0.12s), so it cannot serve as
failing-before evidence. This result is preserved, not relabeled. The agent is
checking whether the gesture fixture actually enters the defective path.

Resolved diagnostic: tape-lane-baseline.md records owner-state PASS after every
gesture frame and historical protection predating quick-range. No fourth
product repair occurred. GOAL A4 now reconciles #447's original explicit
non-defect disposition allowance; this is not a test/guard waiver. Final
source-first delivery review must independently check that reconciliation.

## Precommit evidence hygiene

After staging previously untracked evidence, git diff --cached --check found
trailing spaces on quoted blank lines in combined-sources.md and an extra
EOF blank in ordered-validation-v4.md. The commit was not invoked. Only that
Markdown whitespace was normalized; source quotations retain their words and
all87 tested input hashes remain unchanged. Repeated staged diff check PASS.
This evidence-only correction does not rerun or relabel the successful full
runtime loop64485; final-head CI and independent reviews remain mandatory.
