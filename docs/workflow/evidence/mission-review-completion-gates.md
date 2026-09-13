# Mission review completion gate evidence

Issue: [#389](https://github.com/milocaetano/quantick/issues/389). The mission
is tier `high`. This record describes the implementation boundary and indexes
validation; final review report URLs, the exact PR head, and CI remain in the
PR's generated delivery-evidence block.

## Investigation and ownership

| Surface | PR #306 failure before this change | Mechanical owner after this change |
| --- | --- | --- |
| `mission` | `What done means` named PR readiness, CI, delivery review and body evidence but did not literally enumerate current AI completion/report/thread proof. | The final verifier reads `D1`–`D8` literally and requires the reviewed goal archive to match the canonical four-line `G-AI` block. |
| `ship` | Completion ended by calling `gh pr ready`; an already-ready PR skipped that command and therefore its hook. | Both draft and already-ready paths must finish through `mission_ship_gate.sh ship`. |
| `arch-review` | The skill exposed a raw shell redirect that any caller could use to manufacture `arch-review-ok`. | The skill calls the shared report producer; the producer publishes and reads back a current-identity PASS before writing the projection. |
| `delivery-review` | Its exact-diff marker was private and locally forgeable in the same way. | The same producer binds its durable PASS report and projection; the bounded `small` exemption remains the only omission. |
| `ai-review` | It required a durable report but the readiness gate validated only the private completion line plus a separate thread count. | The shared producer binds the durable COMPLETE report to branch, head, base, base tip and review key before recording completion. |
| `pr-gate` | It ran only on recognized `gh` commands and trusted current marker contents; a final sentence and a PR already outside draft were beyond its surface. | Ready/merge require matching durable reports as well as projections. The final verifier reuses this policy without executing a ready transition. |
| Guardrail tests | They covered current/stale AI completion at ready/merge but not mission/ship closure on an already-ready green, mergeable PR. | The PR #306 fixture drives both callers with AI completion absent and with exactly five listed open threads; tier and fabricated-marker cases surround it. |

The trust claim is deliberately bounded. A machine can prove that a structured
report was published for the exact review identity and that the canonical
producer wrote the matching local projection. It cannot prove that the
reviewer's judgment was good or resist a malicious actor able to forge both
GitHub comments and local state. This change detects the observed bare manual
marker instead of claiming cryptographic review attestation.

## Changed paths and rates

| Paths | Rate | Impact |
| --- | --- | --- |
| `guardrails.sh`, `review_report.sh`, `mission_ship_gate.sh` | Rare delivery commands | Additional local identity reads, literal thread listing and bounded GitHub queries during readiness/completion only. |
| Hook shell tests | Rare validation | Disposable repositories and stubbed GitHub responses; no live credentials or mutation. |
| Workflow skills, hook documentation and Codex mapping | On-demand instruction loading | No product runtime cost. |

No per-trade, per-depth-update or per-frame path changes. No application,
engine, feed, financial, UI, public control contract, Cargo manifest or lockfile
is touched.

## Regression contract

The fixture named `PR 306 <caller> refuses an already-ready green mergeable PR
without AI completion` supplies all optimistic GitHub signals and current
architecture/delivery projections. It must fail for both `mission` and `ship`.
The companion fixture supplies current AI completion but exactly five rows from
`ai_review_threads.sh list`; it must also fail for both callers. Positive cases
cover every tier, with delivery evidence absent only for a bounded `small`
mission.

The base branch has no `mission_ship_gate.sh`, so it cannot execute these
closure cases. The new suite first failed at that missing mechanical owner;
after implementation, the dedicated durable-report tests and the integrated
guardrail suite are required to pass. Removing the final verifier or its AI
checks makes the named PR #306 cases fail.

## Validation index

On the Windows development host, the first full workspace run reached 1,981
passing app tests but reproduced two existing orderflow projection failures;
both failed again alone, and this branch changes neither test nor runtime path.
The rebased `origin/main` head (`57767f25`) has green Ubuntu CI, whose workflow
runs the full workspace suite. This is recorded as failed local evidence, not
relabeled as a pass; green final-head Ubuntu CI remains mandatory.

The final branch must record these exact commands and input head in the PR:

- `sh .claude/hooks/review_report_test.sh`
- `sh .claude/hooks/guardrails_test.sh`
- `cargo test -p quantick-guards`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets`
- `cargo build --workspace`
- `cargo test --workspace`

Final architecture, AI and delivery reports are published through
`review_report.sh`; their URLs and the final literal completion reconciliation
are generated into the PR evidence rather than copied into this pre-review
artifact.
