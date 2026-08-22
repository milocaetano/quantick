# Mission: strategy break-retest limit entry + replay no-fire bugs

Melhorar a estratégia de Force Bar armada em retângulos: entrada por ordem
limitada no retest da borda cortada da região (cancelada quando o alvo da BF
é atingido antes), e corrigir os caminhos silenciosos que fazem BFs não
executarem durante o replay.

Branch: `feat/strategy-retest-limit` — worktree
`../quantick-worktrees/feat-strategy-retest-limit`.

## Mission-specific criteria

1. **Retest-limit entry (kernel + sim, test-first)**
   - `StrategyParams` ganha política de corte (default = comportamento atual).
   - BF do lado da instância fechando além da borda da região na direção do
     trade, com a política ligada → `PlaceLimit` na borda cortada
     (Sell: `region.low`; Buy: `region.high`), bracket projetado da BF
     (mesma âncora da entrada a mercado), `cancel_at` = take profit da BF.
   - `sim`: ordem limit com `cancel_at` — um print negociando em/através do
     nível antes do fill cancela a ordem com razão própria; validação de lado
     na colocação; um print nunca satisfaz fill e cancel ao mesmo tempo.
   - Cancelamento por alvo: OneShot → disarm nomeado; Auto → volta a Armed.
   - Fill no retest → InPosition com bracket; disarm com ordem pendente varre
     a ordem (comando de limpeza retornado pelo kernel).
   - Fechamento dentro da região continua entrada a mercado como hoje; close
     além da borda oposta continua não fazendo nada.
   - Golden test integração (tape fixa → mesmos comandos/fills 2×).

2. **Bug: re-arm frio após seek** — re-armar depois de
   TimelineReset/BarSpecChanged/MarketChanged re-aquece o ruler com as barras
   fechadas que o pane já tem (novo método no port `Trigger` para declarar a
   janela de warmup). Teste: seek → rearm → BF na primeira barra elegível
   dispara.

3. **Bug: região expira em silêncio na âncora direita** — retângulo ganha
   extend-right (payload persistido, paint até a borda direita, settings UI);
   `strategy_region` trata a região ativa além da âncora direita quando
   ligado; arming dialog avisa quando a região não cobre o presente. Default
   off preserva desenhos existentes.

4. **Honestidade do não-disparo** — `BarVerdict` distingue o floor
   (ratio na banda mas corpo abaixo do piso ≠ "quiet"), e o status/badge
   nomeia o gate que segurou um force visto ("fora da região", "região
   inativa", "conta ocupada", "lado oposto"). Testes de status.

## Standard gates (code change + hot path + user-visible + trader action + determinism)

- Quatro checks verdes após rebase na `main` mais recente:
  `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo build --workspace`, `cargo test --workspace`.
- Performance declarada por caminho tocado (per-trade: scan de resting orders
  no sim +1 comparação; per-bar: kernel; rare: arming/rearm/UI) — e bench
  headless do sim sobre tape densa, main vs branch, números no corpo do PR
  (app não será aberto sem autorização — memória `no-agent-app-launches`).
- ui-harness: surfaces novas/alteradas alcançáveis por env hook
  (`QUANTICK_STRATEGY_DEMO` cobre o dialog; estender se o estado retest
  precisar de foto).
- visual-qa: BLOCKED salvo autorização para abrir o app; declarar no PR.
- trader-ux-review sem Blocker aberto.
- Segunda operadora: a opção nova entra pelo mesmo seam programático
  (`StoredPreset`/`arm_strategy_instance`), legível no badge/status.
- arch-review rodado sobre `git diff main...HEAD`; Blockers/Should-fix
  resolvidos ou deferidos no corpo do PR; marker
  (`git rev-parse HEAD > .git/arch-review-ok`) antes de `gh pr create`.
- PR aberto com evidências no corpo. Merge nunca faz parte da missão
  (memória `merge-por-outro-agente`).
