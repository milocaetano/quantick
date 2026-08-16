# Missão — `force_bar`: pintar a vela a partir de um indicador

**Objetivo em uma frase**: abrir o canal de cor por barra que falta no runtime
de indicadores e entregar, em cima dele, o indicador embarcado **Force bar** —
força, exaustão e maior barra — pintando a vela em qualquer tipo de gráfico
(tick, volume, dólar, imbalance), com as cores escolhidas pelo trader.

Branch: `feat/force-bar` · worktree `../quantick-worktrees/feat-force-bar`

## Por que é maior do que um script

`barcolor()` hoje compila e não desenha: `compile.rs:525` emite warning
`PineUnsupported` — *"per-bar color columns land with a later milestone"*.
`PlotBuffer` guarda `Vec<Vec<f64>>` e nada mais; `PreviewFrame` guarda
`values: Vec<f64>`; `draw_candle` (`candle_view.rs:53`) resolve a cor só de
`style.resolved(is_bullish(bar), forming)`. Não existe canal entre indicador e
vela. Esta missão é essa milestone — decidida com o usuário, não inferida.

## Decisões do usuário (tomadas antes de cortar o worktree)

1. **Pintar a vela de verdade**, não marcar ao lado dela.
2. **As três classificações** com a prioridade maior > exaustão > força.
3. **A vela em formação já se pinta**, via `PreviewFrame` (a cor pode mudar até
   fechar; o rollback do runtime garante que nada errado fica gravado).

Fonte do operacional: `.claude/refs/tv-barras-forca-exaustao.pine`.

## Critérios de aceite

### Específicos da missão

1. **A porta existe e é nomeada.** Um canal de tinta por barra em
   `quantick-indicators` — `0`/`None` significa "sem tinta", e todo indicador
   que não pinta continua produzindo exatamente o que produz hoje. Coberto por
   um indicador-fake de teste que pinta, provando que a porta serve qualquer
   implementação, não só o Pine.
2. **`barcolor(expr)` deixa de ser inerte.** A expressão de cor é avaliada por
   barra (não precisa mais dobrar em constante), o warning `PineUnsupported`
   sai, e um teste de semântica prova a cor certa na barra certa.
3. **`force_bar.pine` embarcado**, com as três classificações, prioridade
   maior > exaustão > força, e como `input`: períodos, multiplicadores (com
   `minval`/`maxval`/`step`) e as seis cores. Cópia byte a byte em
   `crates/pine/tests/corpus/ok/`, teste-pino contra a cópia embarcada, e teste
   de semântica **em par** — uma fita que acende e uma quase idêntica que não.
4. **A vela pintada chega à tela**, inclusive a que está em formação, e o
   preview faz rollback: a tinta transitória nunca fica gravada na coluna
   commitada.
5. **Qualquer tipo de barra.** O caminho não consulta o tipo de barra em ponto
   nenhum; provado com fixture de imbalance bars além da fixture padrão.
6. **Precedência determinística** quando mais de um indicador pinta a mesma
   barra: regra única, documentada no código e travada por teste.
7. **Determinismo**: golden test sobre a coluna de tinta — mesma fita, mesmas
   cores, bit a bit, em duas execuções.

### Portões injetados pelo tipo de trabalho

| Portão | Por quê | Evidência exigida |
| --- | --- | --- |
| Quatro checks verdes | qualquer código | saída dos quatro comandos após rebase em `main` |
| Performance declarada | toca caminho quente | classificação por taxa de cada caminho tocado + números |
| `new-extension` | capacidade nova | porta nomeada, edição de registro, defaults preservam hoje, fake testado, raio de impacto (arquivos criados vs. editados) no PR |
| Golden test | crate puro (`indicators`) | fixture + `assert_golden`, duas execuções idênticas |
| `ui-harness` | superfície de UI alterada | `QUANTICK_INDICATOR_SCRIPTS_AUTOSTART=force_bar` alcança; hook novo nesta mudança se faltar |
| `visual-qa` | UI | matriz de estados capturada, PASS ou defeito aceito por escrito |
| `trader-ux-review` | o trader toca em sessão | sem Blocker em aberto |
| `arch-review` | sempre, antes do PR | rodada sobre `git diff main...HEAD`; sha gravado em `<git-dir>/arch-review-ok` |
| PR aberto | sempre | URL do PR com CI verde. **Merge não faz parte.** |

### Declaração de performance (feita no plano, não na revisão)

| Caminho tocado | Taxa | Custo esperado |
| --- | --- | --- |
| `barcolor` no interpretador Pine | per-bar (commit) | alvo ≤ 50 µs por commit run; 200 µs é hard fail (`docs/indicator-system-plan.md:617`) — medido com `cargo bench -p quantick-pine` |
| `preview()` do script | ~10 Hz por barra em formação | mesma ordem do commit run; um preview por frame no pior caso |
| leitura da tinta em `pane.rs` | per-frame × barras visíveis | um lookup por barra desenhada; alvo é ruído dentro de `frame_cpu_ms` |
| `draw_candle` | per-frame × barras visíveis | inalterado em geometria; só a origem da cor muda |

Evidência: `APP_HEALTH_SUMMARY` (fps e `frame_avg`) sob fita densa contra uma
execução de controle em `main`, mais o bench do interpretador. Números no corpo
do PR.

### Dependência humana declarada

`visual-qa` e a medição de fps **exigem abrir o app**, o que esta casa proíbe
sem autorização explícita do Camilo. Sem essa autorização os dois portões são
reportados **BLOCKED**, com o bench do Pine e os testes automatizados entregues
mesmo assim — nunca PASS presumido.

## Fora de escopo (não fazer sem novo pedido)

- Cor dinâmica em `plot()` e `bgcolor()`. A porta nasce genérica o bastante
  para servi-los depois, mas esta missão entrega só `barcolor`.
- Alertas (`alert()` não existe no dialeto; `alertcondition()` segue inerte).
- Qualquer mudança nos scripts embarcados existentes além do append no registro.
