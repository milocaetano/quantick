# Missão: colapsar a legenda de indicadores do chart

Dar ao trader um jeito de fechar a legenda on-chart — hoje uma linha por
indicador, sempre visível no canto superior-esquerdo — e reabri-la por uma
setinha, sem que um indicador quebrado consiga se esconder atrás do colapso.

## Desenho escolhido: "Silêncio com exceção" (C)

Consultoria de UX com três desenhos concorrentes; o trader escolheu o C.

- **Colapsado**: um puck `> N` com a contagem dos indicadores ocultos. O canto
  volta ao gráfico — com posição aberta e 4 indicadores, o topo-esquerdo
  ocupado cai de 163 px para 94 px, e a order-flow key sobe junto.
- **A exceção não colapsa**: toda linha `error` ou `stale` continua desenhada
  inteira, com seus botões, **com a legenda fechada**. Não é política, é
  invariante: o colapso nunca recebe poder sobre uma linha doente, então
  nenhum refactor futuro consegue enterrar um erro ali.
- **Preview veste o puck**, decidido durante a implementação e não no desenho
  original: o chip âmbar "preview" vai para o próprio puck em vez de promover
  a linha. Diz a mesma verdade no chart — que o gráfico mostra ajustes não
  aplicados, e o dialog pode estar atrás dele — sem obrigar `stack_height_px`
  a aprender qual slot está em preview através de três construtores de
  `PaneChrome`. Preview é um estado da *sessão de configuração*, não uma
  doença do indicador.
- **Expandido**: idêntico a hoje, com o chevron prefixando a primeira linha —
  custo zero de altura no estado em que o trader mais fica.
- **Por pane e persistente**: no split, fechar a legenda do pane de fluxo não
  fecha a do pane de tempo, e a escolha sobrevive ao restart.
- **Nunca auto-colapsa**: a única automação é a inversa — um indicador que
  adoece se promove para a faixa de exceção. Expansão de informação, nunca
  ocultação.

## Critérios de aceitação

### Da missão

1. A legenda colapsa e expande por pane, pelo chevron de disclosure — mesmo
   glifo, mesma altura de 20 px e mesma direção do sub-pane colapsado
   (`indicator_render.rs`), e não um segundo idioma de colapso.
2. Colapsada, ela mostra o puck com a contagem dos ocultos; expandida, desenha
   exatamente o que desenha hoje.
3. **Teste** provando que um indicador em `error` e um em `stale` continuam
   pintando sua linha e sua mensagem com a legenda colapsada, e que o chip de
   `preview` aparece no puck sem que a linha seja promovida. A regra de
   honestidade de dados vira teste, não comentário.
4. `stack_height_px` aprende o estado colapsado, e o teste de predição
   (`the_predicted_stack_height_covers_what_the_legend_actually_draws`) cobre a
   matriz colapsado × expandido × nº de linhas doentes. A order-flow key nunca
   imprime sobre a legenda nem deixa buraco.
5. Persistência em `SavedTab` (`ui_state.rs`) com `#[serde(default)]`, sem
   bump de `FORMAT_VERSION`: um workspace gravado antes desta mudança abre
   expandido, idêntico a hoje.

### Gates padrão (injetados pela classificação)

**Mudança de código**

- [ ] Quatro checks verdes sobre `main` atualizada: `cargo fmt --all -- --check`,
      `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo build --workspace`, `cargo test --workspace`.
- [ ] Impacto de performance declarado no plano por taxa (per-frame aqui), não
      descoberto na review.
- [ ] `arch-review` rodado sobre `git diff main...HEAD`, todo Blocker e
      Should-fix resolvido ou deferido no corpo do PR.
- [ ] **PR aberto**. Merge não faz parte da missão.

**Caminho quente (per-frame)**

- [ ] Evidência medida, não crença: `APP_HEALTH_SUMMARY` (fps / frame_avg) sob
      fita densa, contra uma corrida de controle em `main`, na mesma janela de
      condições. Números no corpo do PR.

**User-visible**

- [ ] `ui-harness`: hook de env var novo (`QUANTICK_LEGEND_COLLAPSED`) alcançando
      o estado colapsado de um launch limpo, chamando a mesma função do gesto
      manual, mais a linha na tabela do SKILL.
- [ ] `visual-qa`: todas as superfícies PASS — colapsado saudável, colapsado com
      erro, expandido, split com um pane fechado e outro aberto, colapsado sob
      HUD de posição — ou defeito explicitamente aceito.
- [ ] `trader-ux-review` sem Blocker em aberto.

**Algo que o trader faz (ação nova)**

- [ ] Drivável sem mouse pelos três critérios do *second operator*: **act**
      (`LegendAction::SetCollapsed` + método nomeado no app, um caminho só, não
      mutação dentro do `if clicked()`), **read** (o estado é legível como dado,
      não só como pixels), **discover** (a capacidade se anuncia; onde não
      houver registro, dizer por que a ação fica local em vez de inventar um).

## Fora de escopo

Reformar a aba Indicators do dock, filtrar indicadores de sub-pane da legenda
(redundância real que o designer apontou, mas é outra missão), auto-colapso por
contagem.
