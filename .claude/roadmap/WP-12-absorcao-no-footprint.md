# WP-12 — absorção no footprint

**Missão**: o footprint já mostra POC, imbalances diagonais e zonas
empilhadas. Falta o sinal que o Setup A usa como armação: **esforço sem
resultado** — nível no extremo da barra com volume anômalo e o close longe
dele.

Branch: `feat/footprint-absorption` · worktree
`../quantick-worktrees/feat-footprint-absorption`

Depende de: nada. **Sequencial** com WP-11 e WP-13.

## Onde vive

Método puro em `crates/engine/src/footprint.rs`, ao lado dos sinais que já
existem — `imbalances(ratio, min_qty)` (`:298`), `stacked_zones(...)` (`:337`),
`extreme_ratio(extreme)` (`:380`). O idioma desses três é a especificação do
novo: **todos os cortes são parâmetros do chamador**; o engine não fixa
threshold nenhum, e diz isso explicitamente em `profile.rs:25-27` — *"the
engine attaches no thresholds of its own"*.

Render: toggle em `footprint_config` / presets, com a célula ganhando contorno.

## Critérios de aceite

1. Método puro no `engine`, parâmetros do chamador (volume mínimo do nível e
   distância mínima do close ao extremo, em níveis).
2. **Test-first com fixture golden** — regra da casa para código de engine:
   fixture CSV de trades → absorções esperadas, com as linhas `#` de comentário
   contando a aritmética à mão (idioma dos fixtures existentes em
   `crates/engine/tests/fixtures/`).
3. **Célula em ladder `is_aggregated()` não gera sinal.** Dado engrossado pelo
   cap já é rotulado como aproximado; sinalizar em cima dele seria construir
   conclusão sobre dado que a casa declara impreciso.
4. `None`/vazio quando não há evidência, nunca um valor inventado — o padrão
   de `extreme_ratio`, que devolve `None` em extremo unilateral.
5. O realce herda a marca de lado inferido (WP-05): absorção é medida contra
   agressão, e a agressão é tick rule.
6. Nomes de teste no idioma da casa.

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto declarado: cálculo **por barra**, sob demanda do render — não
      per-trade.
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] Golden test com fixture commitada.
- [ ] `visual-qa` no painel de footprint (denso e vazio).
- [ ] PR aberto com CI verde. Merge não faz parte.
