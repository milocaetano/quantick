# WP-13 — assistente de Opening Balance nativo

**Missão**: automatizar o que o `opening_balance.pine` (WP-04) faz à mão —
marcar o OB, os extremos, o meio e a largura, com âncora automática na sessão e
rótulo honesto de cobertura.

Branch: `feat/opening-balance-tool` · worktree
`../quantick-worktrees/feat-opening-balance-tool`

Depende de: nada. **Sequencial** com WP-11 e WP-12. **Prioridade mais baixa do
roadmap** — o script do WP-04 já entrega o valor operacional; este pacote
entrega conveniência.

## Por que nativo e não só script

O dialeto rejeita builtins de calendário por design, então detectar "abertura
da sessão" dentro do `.pine` depende de comparar `time` entre barras — funciona,
mas é frágil. O app conhece o fuso da sessão (o replay declara `# timezone=`;
o MT5 converte por offset do hello), e é ele quem pode ancorar com segurança.

## Critérios de aceite

1. Ferramenta de desenho com âncora automática, na família das existentes.
   Registro é *"one implementation file plus one name"* na macro
   `register_drawing_tools!` (`drawings/mod.rs:851-896`); a ferramenta
   implementa `DrawingToolImpl` (obrigatórios: `id`, `name`, `settings_title`,
   `icon`, `hover_text`, `required_points`, `paint`, `hit_test`, e
   `test_geometry` sob `#[cfg(test)]`).
2. Ícone honesto: glyph Phosphor **ou** `icon_strokes` (polilinhas no quadrado
   unitário). Há teste que varre todos —
   `icon_strokes_have_two_points_each_and_stay_in_the_unit_square`.
3. **Rótulo de cobertura**: o backfill default do bridge é 30 min; se o
   primeiro print visto for depois das 09:01, o rótulo diz "OB parcial: fita
   desde HH:MM". Sem isso, a régua do dia mente sobre a própria base.
4. Largura do OB exibida em pontos, com o gate de largura mínima (§04) visível.
5. Desenhos **não persistem entre sessões** (só presets de estilo persistem, em
   `quantick-drawing-presets.toml`). Isso é o comportamento vigente e o pacote
   não o muda — redesenhar de manhã é ritual de leitura.
6. Sobrevive a mudança de spec de barra e a seek de replay via `reanchor`
   (âncora guarda `bar` + `time_ms`; fora da série vira `off_series` com fade
   honesto). Teste que prove.
7. Ganha `QUANTICK_DRAWING_TOOL=<id>` de graça pelo registry; para colocá-la
   sem clique, seguir o precedente `QUANTICK_FRVP_DEMO`.

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto declarado: **per-frame** no desenho.
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] `new-extension`: registro é a única edição ao existente.
- [ ] `ui-harness` + `visual-qa` + `trader-ux-review`.
- [ ] PR aberto com CI verde. Merge não faz parte.
