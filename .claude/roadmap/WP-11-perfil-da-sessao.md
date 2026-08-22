# WP-11 — perfil da sessão com POC, VAH e VAL

**Missão**: as bordas e o meio do range medidos, não desenhados no olho. Hoje
existe o FRVP manual (o trader escolhe duas barras); falta o perfil da **sessão
corrente**, com âncora automática, atualizando em desenvolvimento.

Branch: `feat/session-profile` · worktree
`../quantick-worktrees/feat-session-profile`

Depende de: nada. **Sequencial** com WP-12 e WP-13 (todos tocam a família de
desenhos e o registro).

## O cálculo já existe pronto no engine

```rust
VolumeProfile::merge(ladders, level_cap) -> Option<Self>   // engine/profile.rs:77
  .value_area(fraction: Decimal) -> Option<ValueArea>      // :208
  .poc() -> Option<i64>                                    // :178  (empate → bucket mais baixo)
  .bucket_price(bucket: i64) -> Decimal                    // :135
  .is_aggregated() -> bool                                 // :144
pub struct ValueArea { pub poc: i64, pub vah: i64, pub val: i64 }  // :41
```

`merge` devolve `None` para lista vazia ou base groups incompatíveis — recusa
inventar linha. `value_area` segue a convenção Sierra/CQG: expande em **pares**
de linhas impressas, empate expande para baixo, para assim que captura a
fração; só linhas impressas contam, gaps não custam nada.

As ladders vêm de `ChartState::bar_footprints()` (`state.rs:477`) e da parcial
(`partial_footprint()`, `:484`); velas de histórico sem tape usam
`BarFootprint::approximated`. O FRVP já faz exatamente isso — `frvp.rs` é o
molde direto, e o doc dele resume a divisão certa: *"the drawing owns where;
the engine owns what"*.

**O que não existe**: qualquer noção de "sessão" (pregão, RTH, corte horário)
no engine. O recorte de slots é do app, como `covered_slots` (`frvp.rs:161-176`)
faz para o FRVP.

## Porta: desenho ou camada — decidir e justificar

- **Ferramenta de desenho** na família do FRVP: registro é *"one implementation
  file plus one name"* na macro `register_drawing_tools!`
  (`drawings/mod.rs:851-896`). Ganha `QUANTICK_DRAWING_TOOL=<id>` de graça;
  para colocá-la sem clique, o precedente é um `QUANTICK_*_DEMO` (padrão FRVP).
- **`ChartLayer`** (`chart_layers.rs:56-115`, 16 variantes hoje, com assert
  `MASK_FITS` limitando a 32): ganha `QUANTICK_CHART_LAYERS` de graça e é mais
  natural para algo que existe **sempre** durante a sessão, sem o trader
  posicionar.

Recomendação: **camada**, porque o perfil da sessão não tem "onde" para o
trader escolher — a âncora é a sessão. Mas a decisão é do PR.

## Critérios de aceite

1. Âncora automática no primeiro print da sessão; POC, VAH e VAL desenhados e
   **atualizando com a barra em formação**, sem recomputar o span inteiro por
   frame (o FRVP resolve isso com cache key + saída antecipada em key-hit,
   `frvp.rs:222-227` — imitar).
2. **Refresh nunca toca o histórico de undo.** `items_mut()`/`draft_mut()` são
   `pub(crate)` marcados "derived-state refresh only" e há teste dedicado
   (`refresh_never_touches_the_undo_history`, `frvp.rs:696`). Um teste
   equivalente é obrigatório.
3. **Cobertura declarada**: se a fita começou depois da abertura (backfill
   parcial), o rótulo diz "desde HH:MM" — nunca "sessão", que seria mentira.
4. `is_aggregated()` propagado: ladder engrossada pelo cap é rotulada, seguindo
   o que o footprint já faz.
5. Marca de lado inferido onde o número depender de delta (WP-05); POC/VAH/VAL
   são de volume total e **não** dependem — não marcar o que é fato.
6. Testes headless no padrão: `egui::Context::default()`, duas passadas,
   asserção sobre o texto pintado; mais funções livres puras para a lógica de
   recorte (o molde é `covered_slots`).

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto declarado: **per-frame por pane**, nunca no caminho per-trade
      (a regra literal do `frvp.rs:6-11`).
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] `new-extension`: porta nomeada (camada ou ferramenta), registro como
      única edição ao existente.
- [ ] `ui-harness`: hook no mesmo commit (ou justificar que o registry já dá).
- [ ] `visual-qa` + `trader-ux-review`.
- [ ] PR aberto com CI verde. Merge não faz parte.
