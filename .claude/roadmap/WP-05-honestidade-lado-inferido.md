# WP-05 — honestidade do lado inferido: estender, não inventar

**Missão**: fechar os buracos de rotulagem de dado inferido. Todo número
derivado do lado agressor precisa dizer que o lado foi inferido por tick rule —
hoje isso só aparece na status bar, na legenda do footprint e no FRVP. O CVD,
que é 100% derivado de delta, aparece na tela chamado apenas de "CVD".

Branch: `feat/side-inferred-coverage` · worktree
`../quantick-worktrees/feat-side-inferred-coverage`

Depende de: nada. Bloqueia: nada.

## O que o reconhecimento mudou neste pacote

A versão original deste item pedia "criar a flag `side_inferred` e um sufixo
`~`". Ambas as premissas estavam erradas:

1. **A flag já existe e já viaja ponta a ponta.** É um `bool` resolvido por
   frame em `app.rs:6048` e `app.rs:8155` (a partir de
   `Tab::side_note(&config).is_some()`), que desce por
   `CanvasChrome.side_inferred` (`tab.rs:116`) → `PaneChrome.side_inferred`
   (`pane.rs:765`) → `LayerFrame.side_inferred` (`footprint_render.rs:445`) e
   `frvp::RefreshInputs.side_inferred` (`frvp.rs:84`).
2. **A convenção de rotulagem já existe, e não é `~`.** É a string
   `" · side inferred"`, pintada em `footprint_render.rs:1157-1159` e
   `drawings/fixed_range_profile.rs:389-393` — o comentário do segundo diz
   literalmente "same label the footprint legend uses". A cor é
   `theme::AMBER`, reservada por convenção a dado não-venue-truth
   (`theme.rs:49`).

Portanto: **este pacote adota a convenção existente e estende sua cobertura.**
Inventar um segundo vocabulário (`~`) para a mesma verdade seria criar duas
convenções concorrentes na mesma tela. O documento do operacional será
corrigido para falar `· side inferred` onde hoje fala `~`.

Mover `side_inferred` para dentro de `FeedCapabilities` **não** faz parte deste
pacote: seria refatoração de algo que funciona, não adição de honestidade.

## Critérios de aceite

1. **Legenda de indicadores** — um indicador cuja saída depende do lado
   agressor exibe `· side inferred` quando a fonte infere. Alvo mínimo: CVD
   nativo (`indicator_worker.rs:166`) e qualquer script que use os builtins
   `delta`/`cvd`. `indicator_legend.rs::draw()` (`:93-99`) hoje não recebe
   `side_inferred`; `pane.rs` já tem o valor em `chrome.side_inferred` e passa
   a repassá-lo. O rótulo entra ao lado do nome em `indicator_legend.rs:154`,
   em `theme::AMBER`.
2. **Como decidir "este indicador depende do lado"** — a decisão precisa ser
   dado, não lista hardcoded de nomes. Duas rotas aceitáveis, escolha uma e
   justifique no PR: (a) o `IndicatorDescriptor` ganha um booleano que o
   compilador de pine preenche quando o script referencia
   `delta`/`cvd`/`buy_volume`/`sell_volume`, e os nativos declaram
   explicitamente; (b) o worker deriva isso da lista de builtins usados. A
   rota (a) é preferida por ser explícita e testável sem rodar o script.
3. **Chips de delta por barra no footprint** —
   `footprint_render.rs:585-595` (`config.show_delta_totals`) desenha totais
   sem marca; hoje só a legenda global fala. Decidir e implementar: ou os
   chips herdam a marca, ou o PR documenta por que a legenda basta (a segunda
   é aceitável se a legenda estiver sempre visível junto).
4. **Painel de footprint** — `footprint_panel.rs:205-206` ("bar delta totals")
   menciona a inferência quando ela está ativa.
5. **Nada de novo vocabulário** — a string é exatamente a já usada; se ela
   virar `const` compartilhada (hoje é literal em dois lugares), melhor ainda:
   um `const SIDE_INFERRED_NOTE` num módulo comum, com os dois call-sites
   existentes migrados.
6. **Testes que provam que chegou aos pixels** — o padrão da casa para UI é
   rodar contra `egui::Context::default()` em **duas passadas** (o egui assenta
   o layout na segunda), coletar `Shape::Text` de `output.shapes` e concatenar
   `text.galley.text()`. Helper canônico: `painted()` em
   `indicator_legend.rs:326-343`; versão inline em `statusbar.rs:562-575`. O
   assert imprime o texto pintado na mensagem de falha, como em
   `statusbar.rs:578`.
7. **Nomes de teste no idioma da casa**. Modelos reais:
   `side_note_labels_inferred_sides_and_stays_silent_on_venue_truth`
   (`config.rs:1683`), `the_quote_driven_disclosure_is_really_painted`
   (`statusbar.rs:533`), `the_legend_names_the_indicator_and_its_last_value`
   (`indicator_legend.rs:452`).
8. **Silêncio quando é verdade da venue** — feed que reporta lado de verdade
   (Binance, Hyperliquid) não ganha marca nenhuma. A honestidade é simétrica:
   rotular tudo é tão inútil quanto não rotular nada.
9. **Hook de harness** — não existe hoje forma de forçar o estado inferido sem
   conectar num MT5 real. O caminho disponível é `QUANTICK_CONFIG` apontando
   para um feed `provider = "metatrader"` (que já produz
   `"side: inferred (tick rule)"`), ou `QUANTICK_REPLAY_DIR` com sessão cujo
   header declare `# side_source=`. Documentar qual foi usado na evidência de
   `visual-qa`; se nenhum servir, adicionar hook novo é parte da definição de
   pronto (regra do `ui-harness`).

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto de performance declarado: **per-frame** (rótulo em legenda), sem
      alocação nova por frame — a string é `const`/`Arc`, não `format!` no
      caminho de desenho.
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] `ui-harness`: superfícies alteradas alcançáveis por hook; se faltou hook,
      ele entra nesta mudança.
- [ ] `visual-qa` nas superfícies tocadas (legenda, footprint, painel) — PASS
      ou BLOCKED declarado se não houver autorização para abrir o app.
- [ ] `trader-ux-review`: a marca informa sem poluir; não pode empurrar o nome
      do indicador para fora da legenda em janela estreita.
- [ ] PR aberto com CI verde. Merge não faz parte.
