# Mission — Copilot: sliders ao vivo + toggles de exibição por camada

Tornar o indicador Copilot (e o diálogo genérico de settings de indicador)
configurável com sliders que atualizam o gráfico ao vivo enquanto se arrasta,
e com toggles para ligar/desligar cada camada visual (semáforo/ribbon,
estrutura, divergência/near-miss, sinais, stop) — desenhado com opinião de
UX de trader.

Branch: `feat/copilot-live-settings` · worktree `../quantick-worktrees/feat-copilot-live-settings`

## Critérios de aceite

Específicos da missão:

1. **Sliders com preview ao vivo** — inputs numéricos que declaram
   `minval`+`maxval` renderizam como slider no diálogo de settings
   (`crates/app/src/indicator_panel.rs`); arrastar o slider re-executa o
   indicador e o gráfico atualiza sem precisar de Apply. Commit/rollback
   respeita o contrato commit/preview do `Indicator` (determinismo intacto).
   Inputs sem range continuam como DragValue — nada quebra nos demais
   indicadores.
2. **Ranges no script** — `crates/app/scripts/copilot.pine` declara
   `minval`/`maxval`/`step` sensatos em todos os inputs numéricos (e o
   corpus/teste do pine continua verde).
3. **Toggles de exibição por camada** — um toggle por camada visual do
   Copilot (semáforo, estrutura DT/DB, sinais+stop, near-misses), decidido
   com a trader-ux-review: bools no próprio script (append-only na lista de
   inputs), porque as camadas misturam plots e draw objects sem identidade
   em runtime — um toggle render-side só de plots deixaria notas/stop na
   tela contradizendo a camada desligada. Com o preview ao vivo, marcar o
   checkbox aplica na hora; Apply persiste. Toggle genérico por plot +
   identidade de grupo para draw objects fica registrado como deferido no
   corpo do PR.
4. **Opinião de UX de trader** — `trader-ux-review` roda sobre o design e o
   resultado; nenhum Blocker sem resolução (Should-fix deferido vai no corpo
   do PR).

Gates padrão (mudança de código + UI visível ao trader):

5. Quatro checks verdes após rebase na `main` mais recente
   (`fmt --check`, `clippy -D warnings`, `build`, `test` — workspace).
6. Impacto de performance declarado no plano: caminhos tocados classificados
   por taxa (per-frame para o diálogo/slider; re-execução do indicador em
   preview é por interação, não per-trade — evidência de que arrastar o
   slider não degrada fps num tape denso, `APP_HEALTH_SUMMARY` vs. controle).
7. `ui-harness`: toda surface nova/alterada alcançável por env hook (hook
   adicionado na mesma mudança); `visual-qa` com todas as surfaces PASS ou
   defeitos explicitamente aceitos.
8. `arch-review` sobre `git diff main...HEAD`; todo Blocker/Should-fix
   resolvido ou deferido no corpo do PR.
9. **PR aberto** com evidências no corpo — missão termina no PR aberto com
   CI verde; merge é decisão do usuário, nunca parte da missão.
