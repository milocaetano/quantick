# Mission: native MCP chart image

Deliver a native MCP PNG capture of the Quantick window through the existing observer evidence path, so Codex can inspect the visible chart.

**Tier:** medium. A bounded adapter capability with integrity and privacy constraints; no domain, render or trading changes. Completeness-only delivery review applies.

## Request ledger

- R1: Return a native MCP image through a named observer tool. Source: implementation delegation, "captura ... como bloco de imagem MCP nativo". A1.
- R2: Reuse evidence.capture/read and the authenticated gateway; no parallel transport, direct egui access or render serialization. Source: implementation delegation. A2.
- R3: Bound pagination; validate identity, size, base64 and digest; preserve revocation, expiry and error behavior. Source: implementation delegation. A3.
- R4: Preserve screenshot/evidence consent and notice; whole-window privacy; revision/gaps/state_skew metadata; no duplicate base64. Source: implementation delegation. A4.
- R5: Observer remains read-only, with no order tools or real order operations. Source: all delegations. A5.
- R6: Test success, missing image, corruption, pagination, budgets, revocation/expiry and ambiguity. Source: implementation delegation. A6.
- R7: Document exact observer registration and a real window/GPU smoke test; report files, tests and limitations. Source: implementation delegation. A7.
- R8: Preserve unrelated changes; publish a scoped branch/PR only after proof of functionality. Source: publication and final steering delegations. A8.
- R9: Follow the integrated mission workflow and record its tier/evidence/gates. Source: mission delegation. A9.
- R10: Perform a real window/GPU smoke test before any commit/push/PR; if user action is unavoidable, report it and keep the PR unopened. Source: final steering delegation; supersedes the earlier prohibition on connecting to a real instance. A10.

## Decisions

- D1: The user explicitly accepted the chronology exception through the coordinator: initial edits preceded GOAL/tier and the pre-edit cargo check. Guards were built before the edits. No retrospective preflight is claimed.
- D2: Coordinator approved moving only uncommitted MCP work from inherited ab47a7d5 to origin/main (35eaf082), retaining the original commit/branches. Branch: feat/mcp-native-chart-image.
- D3: Preexisting config/bubbles.toml changes are preserved in the stash named preexisting-bubbles-before-mcp-native-image, excluded from the PR and validation inputs.

## Assumptions

- S1: Whole-window capture is the first implementation, explicitly allowed by the request; cropping is not required.
- S2: No additional questions qualify: existing evidence schemas, limits and errors define the behavior. The only tool argument is optional instance_id.
- S3: Rate class is rare, on explicit MCP request. Bounded bundle assembly occurs in the adapter; no trade/depth/frame-path changes.
- S4: PR-dependent reports and final completion follow local tests/reviews and real smoke proof. They cannot run before a PR exists; no pre-PR gate will be represented as a durable PR verdict.

## Acceptance criteria

- [ ] **A1** — quantick_capture_chart returns one native image/png block through tools/list and tools/call. *Evidence:* adapter and STDIO tests; PR Validation section. (R1)
- [ ] **A2** — The tool invokes only existing evidence.capture/read through ControlLink; app/render/domain code remains unchanged. *Evidence:* diff and recorded fake calls; PR Design section. (R2)
- [ ] **A3** — Corrupt, oversized, substituted or incomplete resources fail without partial pixels; gateway lifetime/grant errors propagate without retry. *Evidence:* capture_chart tests; PR Validation section. (R3)
- [ ] **A4** — Pixels appear only in image content; metadata preserves gaps and revisions; documentation names whole-window sensitivity and consent. *Evidence:* no-duplicate test and README; PR Privacy section. (R4)
- [ ] **A5** — Observer lists only read tools; capture never invokes a write capability or changes grant policy. *Evidence:* observer STDIO test, main/profile diff inspection, real smoke. (R5)
- [ ] **A6** — Requested success and failure scenarios have passing automated tests below app. *Evidence:* cargo test -p quantick-mcp and named capture tests; PR Validation section. (R6)
- [ ] **A7** — Maintained README contains exact Windows observer registration, grants, tool invocation and expected real-smoke evidence; final report lists changes/checks/limits. *Evidence:* README and final handoff. (R7)
- [ ] **A8** — PR diff excludes config/bubbles.toml and inherited evidence removals; publication occurs after proof. *Evidence:* git diff origin/main...HEAD and publication chronology. (R8)
- [ ] **A9** — The medium mission records retained sources, chronology exception, criteria and applicable gates using the integrated workflow. *Evidence:* this mission and final verifier. (R9)
- [ ] **A10** — A real window/GPU run returns a valid PNG image with expected metadata before commit/push/PR, or publication remains blocked pending a named user action. *Evidence:* external smoke artifacts and PR summary if published. (R10)

## Gates

- [ ] **G1** — Repository artifacts are English. Source: CLAUDE.md. Evidence: guards and review, PR Validation.
- [ ] **G2** — Full ordered fmt/clippy/build/test workspace loop passes on the final runtime inputs; exact-head CI is green. Source: CLAUDE.md and delivery contract. Evidence: external logs and CI.
- [ ] **G3** — Architecture bug/shape review is current, with required findings resolved. Source: arch-review. Evidence: PR report.
- [ ] **G4** — Extension uses ControlLink and existing tools registry with a fake implementation; blast radius and rare-path cost declared. Source: new-extension. Evidence: tests and PR Design.
- [x] **G5** — Initial mission chronology exception has explicit user acceptance, with no manufactured pre-edit evidence. Source: delivery contract and coordinator's user-decision message. Evidence: D1 and PR Process note.

<!-- required-ai-review-goal-gates:v1 -->
- [ ] **G-AI1** — AI review is executed for the current PR review.
- [ ] **G-AI2** — A durable AI-review report is published on the PR.
- [ ] **G-AI3** — `ai_review_threads.sh list` returns zero unresolved threads.
- [ ] **G-AI4** — `ai-review-complete` is valid for the current review key.
<!-- end required-ai-review-goal-gates:v1 -->

G-AI evidence destination: the new PR's durable review report and private producer receipt; pending, not executed.

## Not applicable

- Engine test-first/determinism: no engine changes.
- Dense-tape hot-path comparison: no per-frame, per-trade or per-depth changes.
- New UI harness hook and chart UX changes: no new or changed app UI surface; the existing capture notice is reused. Real screenshot delivery is covered by A10.
- Dependency policy checks: no dependency changes planned; reclassify if Cargo.lock changes.

## Closing steps

- C1: Archive this mission before review using the integrated mission convention.
- C2: After automated and real smoke proof, commit the scoped change, publish branch and draft PR to main. Only the user merges main.
- C3: Publish current arch/AI reports, resolve threads, then run medium delivery completeness review and publish its report.
- C4: After green exact-head CI, mark ready and run mission_ship_gate.sh mission <pr>; report success only after PASS.
- C5: Send title, PR URL, changes, tests, limitations and ongoing/completed status to source thread 01a0a86e-06ad-7983-91d0-c5940401d8f1.

## Verbatim delegated requests

## Pre-publication validation record

On 2026-09-16, the resumed mission completed the ordered workspace fmt, clippy,
build and test loop with exit code 0, then repeated it with durable logs under
`C:/Users/User/AppData/Local/Temp/quantick-mcp-native-image/`:
`fmt.log`, `clippy.log`, `build.log`, and `test.log`. The runner printed
`VALIDATION: PASS`: 3926 tests passed, 19 ignored, across 107 result summaries.
`source-identity.txt`, `tracked.diff`, and
`new-source-hashes.txt` bind that run to base
`35eaf082fbb3dee38e7d7e1ece3f29fc8bc996d4` and the pre-commit source.
No runtime source changed after that run. The archive is an evidence-only delta.

- A1-A6: all 55 MCP tests passed, including ten new capture tests. Repository
  guards passed without raising any baseline. The earlier cycle failure was
  test-file classification: the cfg(test) suite now lives in `tests/mod.rs`.
- A7: README registration and smoke instructions were exercised. The explicit
  scope hook excludes `observe`, which is inserted automatically; including
  that non-selectable floor rejects the hook. Scene and bundle read scopes
  are now listed completely.
- A8-A9: only MCP source/tests/docs and this mission archive are intended for
  publication. The unrelated bubbles stash remains preserved by its name.
  Initial chronology exception D1 remains explicit, not rewritten as a pass.
- A10: a freshly built real Windows/GPU window ran the WINQ26 replay fixture,
  with isolated stores and a separate MT5 listener, bridge autostart off.
  Health was 59-60 fps at about 16.7 ms/frame. The observer returned exactly
  one native image/png block, 1650 x 975 pixels, 270327 bytes. Pillow decoded
  and verified it; visual inspection confirmed chart candles, replay labels
  and bubbles rather than a blank framebuffer. Revision 2 matched the image
  descriptor; state-skew and unavailable-region coverage were retained.
- Screenshot SHA-256:
  `75692bf774f4266988bdbbd02c74554397470d63a9d1261d89f0d0d4eaea304d`.
  Artifacts: `allowed/capture.png`, `allowed/metadata.json`,
  `allowed/description.json`, `allowed/app.stderr.log`.
- A separate real window without the screenshot scope returned
  `control.scope_denied`, naming `observe.screenshot`, with no image.
  Artifact: `denied/refusal.json`. This proves withheld consent; mid-read
  revocation and expiry are separately covered by automated tests.
- MCP observer exposed 11 read-only tools and exactly the eight effective read
  scopes documented in README. No order tool, write capability or real order
  was invoked. All three launched validation processes were closed through
  their own window-close path after checking the exact executable path.
- The first launch read the existing strategy preset bank because its extra
  override was missing. Code inspection confirmed load-time migration was
  in-memory only; its real file timestamp remained August 31. Subsequent
  launches also isolated `QUANTICK_STRATEGY_PRESETS`. No strategy was armed.
- App binary SHA-256:
  `7c02161f5e34730c7d5544dbe468c3c4fae76320f22a377221bd2b65ffbcb171`.
  MCP binary SHA-256:
  `d39224681ed0cd25233356d44098bab0f633b39f1e85f06aa024c17d19d74ffb`.
- Registered `quantick` globally in Codex with this worktree's tested debug
  MCP executable and `--profile observer`; no existing Quantick registration
  was replaced. The already-running voice session did not hot-load that tool
  set. Client reconnection and end-to-end voice display remain operational
  limitations, not claimed smoke evidence.

PR-dependent review reports, exact-head CI and final completion are pending at
archival, with their evidence destinations declared above. The user alone
merges main. Publishing is now permitted because automated and real GPU proof
preceded any commit, push or PR.

Attributed resumption request, user in the voice session:

> Se você terminar o que você não terminou ontem já ajudaria bastante

## Original delegated requests

Attributed source: coordinator messages in this task, preserving the user's requirements.

> Prepare-se para implementar a integração MCP de imagem nativa do gráfico do Quantick em modo observer somente leitura. Não inicie alterações até o handoff para worktree terminar. Preserve alterações preexistentes e mantenha qualquer capacidade de trading/ordens fora do escopo.

> Implemente e verifique, na worktree isolada do repositório Quantick, a menor integração necessária para o Codex receber uma captura do gráfico como bloco de imagem MCP nativo, mantendo o perfil observer estritamente somente leitura e sem qualquer ferramenta de envio de ordens. A inspeção anterior confirmou: crates/mcp usa STDIO e encaminha ao control-local autenticado; a captura já existe via evidence.capture/evidence.read e produz PNG base64 em app/control/evidence/image.rs, enquanto crates/mcp/src/protocol.rs atualmente só suporta Content::Text. Reutilize a captura existente e o gateway; não crie HTTP/WebSocket paralelo, não acesse estado egui diretamente pelo MCP e não faça serialização pesada no render. Prefira uma ferramenta clara como quantick_capture_chart no adaptador MCP, que aciona evidence.capture, lê todas as páginas, respeita limites/revogação/TTL, valida identidade, tamanho e digest, extrai o PNG e retorna Content::Image com mimeType image/png, além de metadados úteis sobre revisão, gaps ou state_skew. Não duplique o base64 em texto ou structuredContent. Preserve avisos visíveis e concessões próprias de screenshot/evidence; trate janela inteira como potencialmente sensível. Cubra com testes do adaptador/protocolo: caminho feliz, imagem ausente, base64/digest inválido, paginação/chunks, limite, revogação/expiração e instância ambígua conforme a arquitetura permitir. Rode formatação e testes/checks proporcionais, incluindo os crates afetados; documente como registrar o MCP no Codex em perfil observer e como fazer o smoke test real com janela/GPU amanhã. Não conecte à instância real nem opere ordens agora. Preserve alterações preexistentes. Ao terminar, entregue os arquivos alterados, testes executados, limitações e os passos exatos do teste real. Quando você terminar, ficar bloqueado ou precisar de uma decisão do usuário, envie um relatório conciso de volta para a tarefa de origem usando send_message_to_thread. Inclua o resultado útil ou a nova informação e diga se o trabalho continua.

> Além da implementação e dos testes já solicitados, o usuário quer que o resultado não fique apenas local: quando a mudança estiver pronta e os checks proporcionais passarem, faça commit numa branch adequada da worktree, publique a branch no remoto e abra um pull request para o branch padrão do repositório. No PR, descreva a saída MCP de imagem nativa, o modo observer somente leitura, os testes executados, limitações do smoke test real com janela/GPU e qualquer risco de privacidade/state_skew. Não inclua mudanças preexistentes não relacionadas. Depois envie ao thread de origem o título e o link do PR, além do relatório final. Se push ou criação do PR exigir autorização ou falhar, reporte o bloqueio exato. Quando você terminar, ficar bloqueado ou precisar de uma decisão do usuário, envie um relatório conciso de volta para a tarefa de origem usando send_message_to_thread. Inclua o resultado útil ou a nova informação e diga se o trabalho continua.

> O usuário informou que PRs do Quantick normalmente são conduzidos como uma `mission`. Antes de publicar, localize e siga o workflow de mission que já existe no repositório (AGENTS.md, CLAUDE.md, docs/workflow, comandos/skills locais e hooks aplicáveis). Classifique o tier proporcionalmente à mudança, registre a mission conforme a convenção existente, produza as evidências exigidas e execute os gates correspondentes. Não invente um formato novo nem ignore a disciplina do repo. Se houver uma skill/comando `$mission` disponível no ambiente dessa tarefa, use-a fielmente; se não houver, replique o processo documentado no repositório e diga isso no relatório. O PR final deve refletir a mission e não ser apenas um commit avulso.

> Decisão do usuário: aceita explicitamente a exceção de cronologia da mission. Registre com fidelidade que os primeiros edits precederam GOAL/tier e cargo check pré-edit, sem alegar preflight retroativo. Prossiga com os gates atuais, finalize a mission, commit, push e abra o PR solicitado.

> A correção de base está aprovada como decisão técnica necessária: mova somente a implementação MCP não commitada para uma branch limpa baseada em origin/main, preserve o commit/branches existentes e mantenha config/bubbles.toml recuperável e fora do PR. Siga as regras de mission da base integrada e confirme no relatório que o PR não carrega a grande remoção de evidências nem mudanças não relacionadas.

> Prioridade explícita do usuário: não abra nem publique o PR antes de provar que funciona. Conclua primeiro os testes automatizados e os gates da mission. Em seguida, antes do PR, faça o smoke test real com a janela/GPU do Quantick se houver uma instância ou forma segura de iniciar a build testada: registrar o MCP observer, habilitar somente as concessões necessárias, chamar a nova ferramenta e verificar que retorna image/png válido junto dos metadados esperados, sem ferramentas de ordens. Se o smoke real depender de uma ação inevitável do usuário, reporte exatamente a ação mínima e mantenha o PR ainda não aberto. Só depois da evidência de funcionamento faça commit/push/PR.
