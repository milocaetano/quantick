# Mission — linguagem visual das bolhas de agressão

Redesenhar como uma agressão é desenhada no chart — marca de consumo, rastro e
bolhas pequenas — para uma leitura harmônica, calma e profissional, com as
proporções derivadas da razão áurea, e publicar o resultado como o preset
**padrão** do projeto open source.

Branch: `feat/bubble-visual-language`
Worktree: `../quantick-worktrees/feat-bubble-visual-language`

## O que hoje incomoda (diagnóstico, não opinião)

| Sintoma relatado | Causa no código |
| --- | --- |
| "cortados em preto" | `show_consumption_front` desenha um `line_segment` vertical atravessando a bolha inteira (`orderflow_render.rs`, `draw_bubble`), na cor `front_color`; o preset ativo `dense tape btc` a define como `[2, 2, 0]` (quase preto). O `impact_ring` usa a mesma cor. |
| "rastros não ficaram legais / poluição" | Cada bolha que consumiu liquidez arrasta um retângulo com gradiente para a direita (`draw_aggression_bubbles`), `trail_color = [0, 0, 0]`, `trail_opacity = 0.77`. Numa fita densa isso é uma mancha preta contínua. |
| "bolhas menores não têm preenchimento" | `hollow_small_buys = true` (default): print de **compra** com raio abaixo de `readable_min_radius` vira anel aberto com apenas `HOLLOW_FILL_ALPHA = 0.22` de fill. |

## Critérios de aceite

### Específicos da missão

1. **A marca de consumo não corta mais a bolha.** A informação "esta agressão
   comeu book" continua legível, mas por uma forma que respeita o disco em vez
   de atravessá-lo. Provado por screenshot lado a lado (antes/depois) na mesma
   fita.
2. **O rastro deixa de ser mancha.** Ou é redesenhado para algo que decai e não
   soma numa massa preta, ou sai do preset padrão — a decisão é justificada no
   corpo do PR, e o "antes/depois" mostra a diferença numa fita densa.
3. **Nenhuma bolha do preset padrão lê como anel vazio.** Bolhas pequenas têm
   preenchimento sólido; se a distinção de lado por forma for mantida, é por
   outro meio que não "tirar o fill".
4. **Proporções derivadas de φ.** As razões da nova geometria (raio → marca,
   raio → halo, opacidades em escala) saem de constantes nomeadas ancoradas em
   φ = 1.618…, documentadas no código. Nenhum número mágico solto.
5. **Preset padrão publicado.** `config/bubbles.toml` ganha o novo preset como
   `default` e como `active`, e é o que o app abre numa instalação limpa. Os
   presets existentes continuam carregando sem erro (teste cobre isso).
6. **Nenhuma regressão de comportamento para quem já configurou.** Mudança de
   default nunca reescreve o `bubbles.toml` de um usuário existente.

### Gates padrão (injetados pela classe da missão)

**Mudança de código**
- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace`, `cargo test --workspace` — todos verdes, rebaseado no `main` atual
- [ ] Impacto de performance declarado por caminho tocado (per-trade / per-depth / per-frame / raro)
- [ ] `arch-review` rodado, todo Blocker/Should-fix resolvido ou deferido no corpo do PR
- [ ] PR aberto (mergear não faz parte da missão)

**Toca hot path (render per-frame)**
- [ ] fps / frame_avg do `APP_HEALTH_SUMMARY` sob fita densa comparados contra um control build do `main` — números no corpo do PR

**Toca superfície visível ao trader**
- [ ] Todo estado novo alcançável por env hook do `ui-harness` (hook adicionado na mesma mudança, linha na tabela do skill)
- [ ] `visual-qa` com todas as superfícies PASS ou defeitos explicitamente aceitos
- [ ] `trader-ux-review` sem Blocker em aberto

## Fora de escopo

Heatmap de liquidez, footprint, candles, layout. A missão é a linguagem visual
da agressão e o preset que a publica.

## Resultado

Concluída. PR aberto a partir de `feat/bubble-visual-language`.

Todos os critérios específicos atendidos, com um achado do próprio operador
durante a revisão: mover `active` para um preset novo derrubou em silêncio as
três janelas de agregação (`cluster_ms` 500→200, `candle_summary` on→off), o
que deixou a camada correta e ilegível. Corrigido e travado por
`the_shipped_default_aggregates_before_it_draws` — nenhum teste olhava para a
agregação do preset publicado, que é exatamente por que a regressão foi muda.
