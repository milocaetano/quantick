# Coordinator validation

Implementation: [#325](https://github.com/milocaetano/quantick/issues/325).
This record covers instruction behavior, not an always-on scheduler. No A+
assessment/remediation or runtime modification was performed.

## Local checks

Run from `feat/campaign-coordinator` on 2026-09-06:

```text
quick_validate.py .claude/skills/campaign  -> Skill is valid! (exit 0)
quick_validate.py .agents/skills/campaign  -> Skill is valid! (exit 0)
relative Markdown link resolution        -> PASS: 5 relative links resolved
cargo test -p quantick-guards --quiet     -> 149 + 18 + 5 passed; 0 failed
git diff --check                         -> exit 0
```

The bundled skill-creator validator used an isolated Python environment with
PyYAML. Initial invocation lacked that dependency; installing it in the isolated
environment resolved the tooling failure. The initial context guard exceeded
its existing budget headroom. Compressing the mission reference's historical
parser explanation preserved its facts and all operative rules; the context
budget was not raised. Both entrypoints follow the score skill's established
pattern of loading detailed instructions from `docs/` on invocation.

Authored prose and branch/commit text are English. The mission archive includes
the explicitly attributed Portuguese user request under the repository's
quotation exemption. Runtime performance is unchanged; the touched path is
rare agent orchestration. No app trunk, Rust code, hook or runtime config grew.

The user explicitly directed proportional local validation for this docs/skills
change. Full workspace format/lint/build/test runs remain in the unchanged
Linux CI, with Windows workspace build and engine/guards tests. The PR records
actual CI results at its head; these local results do not claim those checks
ran locally. #324 retains the broader mission/hook policy redesign.

## Persisted forward test

The [simulation parent #326](https://github.com/milocaetano/quantick/issues/326)
contains its charter, human-task H1, native children and checkpoints.
[T1 #327](https://github.com/milocaetano/quantick/issues/327) proves entrypoint
routing. [T2 #328](https://github.com/milocaetano/quantick/issues/328) is the
independent continuation task.

[Checkpoint 1](https://github.com/milocaetano/quantick/issues/326#issuecomment-5561325450)
deliberately leaves T1 in progress after evidence publication, reproducing an
interruption. A fresh agent received only the parent URL and followed the
persisted evaluator instructions; it had no session transcript and made no
writes. It fetched parent/child comments and reconciled
[T1's accepted evidence](https://github.com/milocaetano/quantick/issues/327#issuecomment-5561320526).

The [independent report](https://github.com/milocaetano/quantick/issues/328#issuecomment-5561336310)
identifies T2 as ready, avoids duplicating T1, verifies all six operations and
both entrypoints, and correctly keeps closure blocked by H1. The coordinator
then closes T1/T2 with that evidence and publishes checkpoint 2. This tests
stale projection, uncertain publication readback, evidence-based dependencies,
human-blocker independence and context-free next-action selection.

The first pass inspected the draft worktree referenced by the GitHub checkpoint;
the final PR provides the committed source and exact-head reviews. This is a
bounded forward test of an instruction skill, not proof of days of unattended
execution. Permission denial is real: the current OAuth grants `read:project`
but not `project`; the attempted roadmap item creation was rejected. Project
payloads are persisted and independently executable work was completed. Live
Project creation, field/view setup and status writes remain unverified until
that permission is granted. The simulation remains open for that reason.

## Review corrections

A preliminary independent bug review found an impossible recursive requirement
to checkpoint checkpoint writes and an unbounded history read on resume.
The contract now makes journal publication non-recursive, defines parent
bootstrap/readback and stable publication keys, and treats complete snapshots
as bounded recovery boundaries with older bodies loaded only on demand.
Final-head architecture, delivery and AI verdicts are recorded on the PR.
