# Mission: prepare the architecture before adding future capabilities

Deliver the first foundation milestone for extensible content, external agents,
and window-independent workspaces: a source-backed architecture audit, ownership
decisions and executable migration tasks. This starts the preparation program;
it does not claim that a documentation PR completes the runtime refactoring.

**Tier:** high. Cross-cutting design judgment and delegation require independent
architecture and full delivery reviews even though this first diff is docs-only.

Issue: https://github.com/milocaetano/quantick/issues/314

## Request ledger

- R1: Prepare architecture first, before implementing the future features.
- R2: Plan for community themes, including appearance defaults and typography.
- R3: Plan for an external official assistant through the existing control plane.
- R4: Prepare workspace ownership for future Chrome-like detachable/rejoinable tabs.
- R5: Preserve performance as the application grows, with measured evidence.
- R6: Build on the existing refactoring and produce ordered, agent-executable tasks.
- R7: Coordinate independent agents and integrate their findings into one plan.
- R8: Make architecture quality verifiable by reviewers, rather than a subjective label.
- R9: Prepare portable, reusable workspace/content templates.
- R10: Define the extension boundary for community plugins.
- R11: Prepare runtime indicator authoring and explicit saving/adoption of source.
- R12: Prepare eventual strategy and voice actions without widening current authority.
- R13: Preserve deterministic processing and one engine across consumers.
- R14: Check access to the supplied workspace reference and report the result.
- R15: Keep this published delivery entirely in English, including the request record.
- R16: Verify which Claude skills were used and whether a new import was performed.

## Decisions taken by the trader

- D1: Architecture preparation takes precedence over feature implementation.
- D2: The assistant coordinates agents and execution.
- D3: Chrome-like workspaces are a future behavior reference, not this milestone's UI.
- D4: The trader objected to the untranslated request in the PR. Use an explicitly
  labeled English translation instead of the original-language quotation. This
  user instruction overrides the mission skill's verbatim-in-repository convention.

## Assumptions

- S1: The first PR records the baseline and implementation contracts, as announced
  before work; follow-up tasks implement them. This keeps reviewable changes small.
- S2: Preserve current feed-per-tab and paper-session semantics during mechanical
  extraction; sharing and changed shutdown behavior require separate design work.
- S3: The linked Claude artifact could not be accessed. Only the user's textual
  description informs this milestone; do not infer unseen interaction details.
- S4: No new product taste, trading autonomy, provider selection or paid service
  decision is necessary to deliver architecture preparation.

## Acceptance criteria

- [x] **A1**: Current-state facts, remaining coupling and recent refactors cite
  source evidence at the inspected revision.
  *Evidence:* audit tables -> `docs/architecture/foundation.md`. (R1, R6, R8)
- [x] **A2**: Target boundaries cover content, operations/authority, window,
  workspace, market session and persistence, distinguishing proposals from code.
  *Evidence:* responsibility contracts -> `docs/architecture/foundation.md`. (R2, R3, R4, R9, R10, R11, R12)
- [x] **A3**: Tasks carry dependencies, scope, acceptance proof, rate/risks and
  coordination rules; the first runtime task is ready to execute.
  *Evidence:* task specifications -> `docs/architecture/preparation-tasks.md`. (R1, R6)
- [x] **A4**: Structural baseline and reproducible performance protocol distinguish
  measured results from unmeasured GUI/memory behavior.
  *Evidence:* commands and limitations -> `docs/architecture/baseline.md`. (R5, R8)
- [x] **A5**: Independent workspace, extension and control audits are integrated;
  open-work overlap and the unavailable reference are explicitly recorded.
  *Evidence:* audit provenance -> `docs/architecture/foundation.md`. (R4, R6, R7, R14)
- [x] **A6**: Runtime files, schemas and dependency graph remain unchanged in this
  first milestone; future implementations are explicitly not marked delivered.
  *Evidence:* diff boundary and retained fixtures -> `docs/architecture/baseline.md`. (R1, R5, R13)
- [x] **A7**: Published prose, including the request record and PR/issue metadata,
  is English; the translation is labeled and preserves the original scope.
  *Evidence:* language correction -> `docs/architecture/baseline.md`. (R15)
- [x] **A8**: Skill provenance distinguishes the existing canonical workflow
  adapters from a new skill import.
  *Evidence:* workflow mapping -> `docs/architecture/baseline.md`. (R16)
- [x] **G1**: All authored prose in this delivery is English, including its request record.
  *Evidence:* guards and manual review -> `docs/architecture/baseline.md`.
- [x] **G2**: fmt, clippy, build and workspace tests pass before commit.
  *Evidence:* recorded command results -> `docs/architecture/baseline.md`.
- [ ] **G3**: Architecture review, including the medium bug pass, has no unresolved
  Blocker/Should-fix findings.
  *Evidence:* final diff verdict -> PR body (scratch review dossier before PR).

## Non-applicable gates

No runtime, UI, engine, capability, dependency or hot-path changes in this PR:
visual QA, trader UX review, new UI hooks, extension implementation tests and a
before/after GUI performance claim do not apply. Four checks and the bug/language
review still apply. Future task specifications name their applicable runtime gates.

## Closing steps

- C1: Archive this mission and run independent full delivery review on the final diff.
- C2: Open the PR with review/verification evidence and observe green CI. Do not merge.

## Request record - English translation

The trader requested an English-only delivery after seeing the original-language
transcript in this archive. The text below is an English translation, not a
verbatim quotation. The original remains in the conversation and outside the
repository in the review dossier. Reviewers compare that source with this
translation rather than treating translated text as the trader's exact words.
Product-name transcription variants are normalized to Quantick. Unclear speech
fragments remain marked; they do not authorize invented functionality.

### Clarifications and authorization

- The plan is to prepare the architecture first.
- You may start preparing the architecture and coordinate the agents.
- Reference: https://claude.ai/code/artifact/17195533-7538-456c-8808-759c5289d451
- Can you access this link?
- The objective for Chrome-like workspaces would be like this reference.
- Like Chrome [unclear trailing word in the transcript].
- First, let us focus on preparing the architecture.
- This started badly because there is Portuguese text there. Did you not import
  the Claude skills?

### Earlier product request

I will explain my plan, and then we need to plan and prepare Quantick's
architecture so that its kernel or core can support these objectives.

1. I want Quantick to be extensible and allow plugins and templates. Different
   themes in these templates should change the colors of the main indicators,
   toolbars and built-in elements [the transcript's term for these is unclear].
   Today this includes:
   - The default horizontal-line element.
   - A default element associated with a moving average [transcription unclear].
   - Volume-profile drawing colors.
   - Candle colors.
   - The color of "future print" [possibly footprint; wording unclear].
   - The chart.
   - Window color.
   - Possibly the window font and the font displayed in the window.
   - Size.
   These defaults are largely hardcoded. I want sufficient extensibility to
   install templates and let the community create templates with different
   themes, similar to Chrome today.

2. Since this is an open-source project, I want an official plugin through which
   I can sell a service, perhaps using Hermes or something similar. Its AI agent
   should connect to the platform's MCP and eventually do everything the user can
   do: analyze the market, place a line, add an indicator, or create one when
   asked. With our PineScript-like concept, it could create an indicator on the
   fly, display its result during execution and save it. The transcript then
   includes an unclear request involving "Jassal"; its meaning is unresolved.
   I believe we do not yet have strategies, but possibly we could create similar
   strategies through voice commands. The user could see the strategy running,
   buy or sell by voice, place a stop order, request an unclear operation and a
   volume profile, all by voice. The user could set an alarm or a strategy:
   essentially everything available through mouse and keyboard. I want Quantick
   to scale. [The architecture audit separately corrects the assumption about
   existing strategy support; this translation preserves the original request.]

3. We will add new features. I am considering workspace tabs like Chrome, with
   a broader system in which a tab can be detached and moved to another screen,
   effectively splitting Quantick into two windows, as when a Chrome tab is
   dragged out into another window. I would call this a workspace.

4. Prepare the existing architecture so Quantick can grow while maintaining its
   current high performance.

5. Given these objectives and where I want to go, with Camilo, define a plan and
   prepare tasks. I do not fully understand how far multitasking can go, but I
   want a plan to reach the objective.

6. I have already started substantial refactoring and code separation in
   Quantick. Inspect that work. I want AI reviewers to assess the architecture
   and recognize senior AI engineering quality. Define the steps and priorities
   we should tackle to reach that objective.
