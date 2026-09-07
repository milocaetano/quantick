# Reduce delivery rework

**Tier:** high — workflow authority, review and validation decisions change.
Issue: https://github.com/milocaetano/quantick/issues/344

## Request ledger

- R1: Reduce repeated requirement reconstruction; distinguish product and operational obligations.
- R2: Measure review progress by finding identity with bounded retries.
- R3: Make documentary follow-up validation proportional without weakening quality.
- R4: Prioritize completing active campaign PRs and expose stage timing.
- R5: Prove behavior with #339/#341-shaped and negative regression scenarios.
- R6: Deliver a reviewed PR with green CI; main merge belongs to the user.

## Decisions and assumptions

- D1: User authorizes changing the workflow and preparing the PR, not main merge.
- S1: Related #324 remains open for the broader tier redesign; this issue is its bounded throughput subset.
- S2: Work uses an isolated main-based worktree; running campaign branches and grants are not changed.
- S3: Canonical instructions govern both clients; existing marker algorithms and hook boundaries remain unchanged.

## Acceptance criteria

- [x] A1: Source-preserving reconciliation with stable IDs and operational gate mapping precedes implementation. Delivery separates outcome/evidence gaps from traceability-only gaps; later real omissions still fail without repeated atomization. Evidence: mission/delivery contract and independent scenarios. (R1)
- [x] A2: Finding identities, separate new findings and finite per-finding/mission repair limits replace raw open-count stalls; authority/blocker boundaries remain. Evidence: contract and scenarios. (R2)
- [x] A3: Only inspected prose/evidence deltas get proportional local checks; changed executable inputs, stale evidence/base and known failures cannot reuse green results; final-head CI and current reviews remain required. Evidence: contract, skill integration and scenarios. (R3)
- [x] A4: Campaign prioritizes merges/repairs and tracks meaningful stages, within concurrency and ownership. Evidence: campaign contract and scheduling scenario. (R4)
- [x] A5: Independent exercises cover #339/#341-shaped traces, omitted product outcomes, executable-input changes, stale base/evidence, independent new findings and recurrent failures, with preserved responses. Evidence: committed validation report. (R5)
- [x] A6: Claude/Codex entrypoints reach the same rules; PR targets main and does not merge it. Evidence: mappings, links and PR metadata. (R6)

## Gates and closing steps

- [x] G1: English, context/language/encoding guards and diff hygiene pass.
- [x] G2: Required local checks, skill validation and hook regression suite pass; record exactly which checks ran or were reused. Runtime rate: rare agent workflow only; no Rust runtime changes.
- C1: Archive this mission before final independent architecture and delivery reviews; publish draft PR, resolve AI findings, confirm exact-head CI, mark ready, report URL.
- C2: No merge to main, no active campaign mutation or override. Preserve existing work and grants.
- UI, trading runtime, dependency and performance benchmarks: not applicable; this changes agent instructions only.

## Verbatim user request

Attributed user request (Portuguese):
> Corrija o workflow para reduzir retrabalho e acelerar a entrega, preservando os gates de qualidade. Prepare o PR; o merge para main é meu.

Accepted preceding proposal (scope reference): stabilize requirements before implementation; track progress by finding; proportionate documentary validation; prioritize concluding reviewed PRs; regress #339/#341 without allowing incomplete delivery; isolated PR before applying rules to the campaign.

## Evidence

The implementation and independent instruction exercises are indexed in
`docs/workflow/validation.md`; retained evaluator responses are under
`docs/workflow/evidence/`. Raw local outputs accompany the review dossier.
A6 covers the common entrypoints and main-only-human boundary; C1 tracks actual
PR publication, current reviews, full final-head CI and readiness separately.
Those closing steps are pending at archival and are recorded on the PR.
