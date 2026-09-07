# AI-review completion evidence

Task: [Q1 / #331](https://github.com/milocaetano/quantick/issues/331), child of
[campaign #330](https://github.com/milocaetano/quantick/issues/330). Branch:
`fix/ai-review-completion`. Worktree:
`C:/src/quantick-worktrees/fix-ai-review-completion`.

This document records implementation and local validation. Independent final
reviews, durable PR review reports, private review markers, publication and
exact-head CI remain coordinator-owned closing steps. Their absence here is
not a claim that those gates passed. No score increment is claimed; campaign
reassessment owns that conclusion.

## Change and trust boundary

Before this change, readiness could see current architecture/delivery evidence
and zero open AI threads when AI review had never run. Now readiness and
campaign merge also require `ai-review-complete` in the effective worktree's
private git directory. Its one LF-terminated line is `<branch> <shared-key>`.
The optional branch argument to the existing `require_marker` checks this
record shape and then uses its existing hexadecimal key comparator. The
architecture/delivery marker format, review-key calculation and tier behavior
remain unchanged.

The branch field closes a distinct case: switching to another task branch can
leave the raw main-based diff key equal. The main fixture asserts that equality
before asserting AI denial. Campaign keys already bind the explicit base ref
and tip; fixtures advance each while refreshing only the architecture/delivery
pair, so those earlier guards cannot account for the AI denial.

Completion does not mean zero findings. The canonical AI skill requires the
review to complete, publish required finding threads and a durable report, and
compare worktree/branch/head/clean status/base/key before and after the review
and publication. The report includes full HEAD SHA, explicit base ref and tip,
key, scope, six verdicts and finding IDs/count. Only then does its procedure
write the local projection. The existing unresolved-thread gate stays separate.
No-PR reviews print a report and do not record completion.

This is the repository's existing operational trust model: the hook validates
a local recording, not reviewer reasoning or GitHub report provenance. No new
skip, override, parser expansion, remote authority source or generic review
subsystem was added. Existing thread-count unavailability behavior and shell
command limitations remain. Main merging is exclusively the user's action.

## Scope and rates

| Paths | Rate and impact |
| --- | --- |
| `.claude/hooks/guardrails.sh` | Rare developer command. One small private marker read and branch query at readiness/merge. Existing comparison and campaign key reused. |
| `.claude/hooks/guardrails_test.sh`, `campaign_context_test.sh` | Rare validation. Disposable real Git histories and stubbed thread/GitHub responses; no credentials or live mutation. |
| `.claude/hooks/README.md` | Documentation of producer, consumer and preserved boundaries. |
| `.claude/skills/ai-review/SKILL.md` | On-demand instructions; completion procedure and report identity. |
| `.claude/skills/ship/SKILL.md`, `mission/SKILL.md` | On-demand instructions. Explicit draft/AI/readiness ordering; shorter existing ship prose pays for the producer instructions. |
| `docs/campaign/integration.md` | Documentation linking to the canonical producer with the same campaign key. |
| `.claude/GOAL-archive-ai-review-completion.md`, this file | Mission and evidence bookkeeping, no runtime path. |

No Rust source, Cargo manifest/lock, financial behavior, product contract,
hot path, UI surface or runtime dependency changes. No baseline is raised.
The mission description's pre-existing angle-bracket placeholder was rewritten
without changing its meaning because the affected skill validator rejects it.

## Local validation artifacts

Raw logs are retained outside the repository at
`C:/Users/camil/AppData/Local/Temp/quantick-architecture-a/Q1-validation/`.
The temporary `run.py` executes each named command in this worktree and records
merged stdout/stderr and its exit code. Git `sh.exe` is resolved explicitly on
Windows. Skill validation uses PyYAML installed only in that directory's
`python-deps`; the repository and product dependency graph are unchanged.

| Log | Exact command / result |
| --- | --- |
| `01-arm-guards.log` | `cargo build -p quantick-guards`: exit 0, 6.05 seconds. Original PowerShell redirection mixed encodings; preserved as received, with that limitation disclosed. |
| `02-arm-app.log` | `cargo check -p quantick-app --all-targets`: exit 0, 1 minute 43 seconds. |
| `03-guards-batch1.log` | `cargo test -p quantick-guards`: exit 0. |
| `04-guards-batch2.log` | Same guard command: exit 101, context total 238244, 320 bytes beyond allowed headroom. |
| `05-guards-context-repair.log` | Same guard command: exit 101, context total 237928, 4 bytes beyond allowed headroom. |
| `06-guards-context-repair.log` | Same guard command: exit 0 after further concise prose repair. |
| `07-shell-suite.log` | Bare `sh` launch failed before the shell test ran; command-only log retained. |
| `08-shell-suite.log` | `C:/Program Files/Git/bin/sh.exe .claude/hooks/guardrails_test.sh`: exit 0; 223 passed, 0 failed, plus 51 campaign-context tests. |
| `09-skill-ai.log`, `10-skill-ship.log`, `11-skill-mission.log` | Affected `quick_validate.py` calls initially failed at import: PyYAML absent. |
| `12-validator-dependency.log` | `python -m pip install --disable-pip-version-check --target .../Q1-validation/python-deps PyYAML`: exit 0; PyYAML 6.0.3 installed privately. |
| `13-skill-ai-review.log`, `14-skill-ship.log` | `python -X utf8 C:/Users/camil/.codex/skills/.system/skill-creator/scripts/quick_validate.py .claude/skills/ai-review` / `ship`: exit 0. |
| `15-skill-mission.log` | Same validator for `mission`: exit 1 on its pre-existing angle brackets in the description. |
| `16-skill-mission-repair.log` | Same mission validator: exit 0 after wording repair. |
| `17-local-links.log` | `python -X utf8 .../Q1-validation/check_links.py`: exit 0; 6 local Markdown links/anchors resolve. |
| `18-guards-evidence.log` | `cargo test -p quantick-guards`: exit 0 after evidence and skill validation repairs. |
| `19-guards-archive.log` | `cargo test -p quantick-guards`: exit 0 after mission archival. |
| `20-campaign-base-update.log` | Temporary update script stopped before mutation because its status parser removed the first line's leading space. |
| `21-campaign-base-update.log` | Corrected update script: exit 0. Explicit owned-path stash, verified fast-forward and restoration preserved all 10 owned files byte-for-byte with no incoming path overlap. |

The ordered final `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets`, `cargo build --workspace`, `cargo test --workspace` loop is
pending against the newly integrated combined base below. No commit is permitted
before all four pass.
Affected link and guard checks passed as recorded above.

## Fixture coverage

The main suite runs the new boundary cases under both `Bash` and `exec_command`
payloads. Each tier (`small`, `medium`, `high`, `max`) denies absent completion
at readiness/merge despite zero threads; current completion allows readiness
and still denies unresolved findings. Small's positive fixture removes delivery
evidence to prove the existing bounded exemption stays narrow.

Additional cases isolate stale keys; empty, malformed and multiline records;
an unterminated appended record; another worktree's identical record; a branch
switch with an equal raw key; same-branch reword; and committed source change
after the other two reviews refresh. Current AI evidence cannot replace missing
architecture or delivery evidence. Draft creation with no markers stays open.

Campaign fixtures isolate base-tip and equivalent-base-ref changes for both
readiness and merge. Current evidence permits the explicitly authorized,
head-pinned, green-CI merge; current AI evidence cannot supply a missing grant
or pending CI. Existing main, auto/admin, alternate-repository, compound-command,
remote identity and unresolved-thread tests are retained. Recording-owner drift
checks now include AI and its shared key/identity/report obligations.

## Coordination and conditional obligations

The coordinator prepared this task at campaign base
`a808b2d87b36d73041027e4d20c053544b454a96`. Q1 initially remained read-only
while F1 owned the host. The coordinator explicitly released the host after
F1 checks completed; both arming commands above passed before the first edit.
Q1 touches none of F1's runtime code or unique mission/evidence files.

F1 then integrated at campaign SHA
`c3a92d58bb8a41ec4d78d73e60312b5f765b4da5`. The coordinator reported the
verified merge and authorized the update. The worker checked the fetched ref,
verified no incoming path overlapped Q1's owned set, preserved only those edits
in a task-specific stash, fast-forwarded without creating a merge commit, and
restored all 10 files with identical SHA-256 hashes. The owned stash was removed
only after restoration succeeded. Fresh guard and full workspace validation
against this base are required; old-base review stamps are not reused.

The implementation worker has made no GitHub write, publication, checkpoint,
review marker, review verdict or merge. Before committing, it must notify the
coordinator so publication intent can be recorded. The coordinator receives
the exact clean committed head, base and review dossier, preserves the archive
before reviews, and alone publishes/reviews/integrates under campaign authority.
No child host goal or subagent was created.

Cumulative failure signatures are retained: context-budget 2; shell-launch 2
(one tool shell launch, one bare subprocess executable lookup, both before test
execution); logging-format 1; missing validator dependency 3 independent calls;
mission-description validator 1; temporary status-parser assertion 1 (before
any stash or base mutation). These are not reset by repairs. The current
successful results above supersede failures as validation, while the raw failed
artifacts remain part of the audit.

Independent architecture (bug and full shape), AI and delivery reviews,
resolution of all required findings, exact-head CI, readiness and authorized
campaign integration remain mandatory closing obligations. Main integration
and campaign score reassessment remain outside this child implementation.
