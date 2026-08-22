# WP-04 — os quatro scripts `.pine` do dia 1

**Missão**: pôr na tela os gates que hoje só existem no papel. Quatro scripts
embarcados: `opening_balance`, `cvd_divergencia`, `bei` e `canal_t`. Sem eles o
trader opera de memória — e gate de memória não é gate.

Branch: `feat/day-one-scripts` · worktree
`../quantick-worktrees/feat-day-one-scripts`

Depende de: nada. Bloqueia: nada. Paralelizável com qualquer outro pacote
(toca só `scripts/`, `library.rs` e `crates/pine/tests/`).

**Um pacote só, de propósito**: os quatro scripts editam a mesma lista
`EMBEDDED_SCRIPTS`. Quatro agentes em paralelo brigariam nesse arquivo.

## Procedimento por script (verificado, cinco arquivos)

1. `crates/app/scripts/<nome>.pine` — o fonte.
2. `crates/app/src/indicators/library.rs:24` — **append** de
   `("<nome>.pine", include_str!("../../scripts/<nome>.pine"))` em
   `EMBEDDED_SCRIPTS`. **Sempre no fim.** `EMBEDDED_SCRIPTS[0]` é consumido
   posicionalmente por `indicator_worker.rs:1144`, cujo teste afirma
   `descriptor.title == "EMA"` — inserir no início quebra esse teste.
3. `crates/pine/tests/corpus/ok/<nome>.pine` — cópia byte a byte. Todo arquivo
   em `corpus/ok/` precisa passar por `compile` inteiro (`corpus.rs:23-48`), e
   os testes de semântica leem a cópia do corpus, não a de `app/scripts`.
4. Teste-pino em `library.rs` (mod tests), no molde de
   `the_embedded_copilot_matches_its_semantics_fixture` (`:200-212`).
   **Atenção — buraco real na base**: esse teste existe **só para o copilot**.
   Hoje `cvd`, `ema` e `delta_histogram` já divergiram entre as duas cópias e
   ninguém reclamou. Sem o pino, os scripts novos derivam em silêncio.
5. `crates/pine/tests/<nome>_semantics.rs` — molde de `copilot_semantics.rs`.

Nada a mudar no menu (derivado de `ScriptLibrary::entries()`) nem em
`docs/pine-dialect.md` (o guard valida registry→doc, não script→doc).

## O molde do teste de semântica

`crates/pine/tests/copilot_semantics.rs` é o padrão. Estrutura: `SCRIPT` via
`include_str!("corpus/ok/<n>.pine")` → `test_inputs()` devolvendo um
`Vec<InputValue>` **posicional, na ordem exata dos `input.*`** → fita sintética
curta escrita à mão (23–26 barras) → `run()` que faz
`compile` → `ScriptIndicator::new` → `rebind(&inputs)` e dirige o indicador
como o host dirige (cvd acumulado pelo teste, um `Ctx { bar_index, cvd }` por
barra, `on_close` por barra fechada) → `marker_rows(indicator, "Título")` que
acha o plot **pelo título** e devolve os índices não-NaN.

Regra de ouro do formato: **sempre um par** — uma fita que deve acender e uma
quase idêntica que não deve. Um teste que só prova que o sinal aparece não
prova que ele discrimina.

Consequência de projeto: **títulos de plot são API de teste**. Todo
`plot`/`plotshape` precisa de `title=` estável e humano.

## O que o dialeto permite (e onde ele morde)

- **Percentil/rank por loop na história: sim.** Idioma provado no corpus:
  `s = 0.0` / `for i = 0 to 4` / `s := s + close[i]`. Receita segura para o
  BEI: `for i = 0 to n - 1` contando `trade_count[i] <= x`, com `n` vindo de
  `input.int(..., maxval=500)`.
- **Caps**: 10.000 iterações por loop por barra; offset de história **dinâmico**
  limitado a **500 barras** (offset constante dimensiona exato e eleva o piso
  para todos os anéis); janela de kernel `ta.*` até 100.000; aninhamento de
  parser 64.
- **`ta.*` stateful dentro de loop é erro de compilação** (`PINE_STATEFUL_IN_LOOP`)
  — o percentil tem de ser aritmética pura (contagem/soma), nunca `ta.highest`
  no corpo.
- **"last write wins within the bar"**: dentro de um loop o anel grava o valor
  do *subject* da última iteração. Mantenha o subject invariante na barra
  (`close`, `delta`, `trade_count`) e varie **só o offset**. `x[i]` está certo;
  `f(i)[0]` não.
- **`plot*`, `input.*`, `indicator`, `hline`, `fill` são top-level only** —
  dentro de `if`/`for`/função é `PINE_SYNTAX`.
- **`ta.pivothigh/pivotlow(src, l, r)`** devolve o valor **na barra que o
  confirma**, `r` barras depois; `l` e `r` **não podem mudar entre barras**
  (o comprimento do kernel é pinado).
- **Cor condicional em `plot` não funciona**: argumento que não dobra em
  constante vira warning e o default entra no lugar. Para duas cores, use dois
  `plot`/`plotshape` com `na` mascarando — é o que o copilot faz no semáforo.
- **`bgcolor`/`barcolor`/`alertcondition` compilam mas são inertes** (warning).
  Não construa nenhum gate em cima deles.
- **Inputs**: defaults têm de dobrar em constante; `options` é sempre vazio
  (**não existe dropdown**); a persistência é **posicional** — input novo vai
  sempre no fim, ou os valores salvos do usuário remapeiam.

## Os quatro scripts

### 1. `opening_balance.pine`
Box do OB entre dois timestamps de input, linhas do meio e dos alvos, e um
label com **largura do range** e **persistência de CVD**. Como o eixo x é
`bar_index` e barras por atividade não mapeiam hora linearmente, a fronteira
de janela é detectada comparando `time` de barras consecutivas — não há
builtins de calendário (rejeitados por design).

### 2. `cvd_divergencia.pine`
S1: pivô do preço no extremo + `cvd` no pivô anterior via `ta.valuewhen`,
margem `m = z × ta.stdev(delta, 100)`. Linha ligando os dois toques e label
"DIV". A marca aparece `pivot_r` barras depois do pivô — atraso honesto,
declarado no cabeçalho do script.

### 3. `bei.pine` — o sinal-assinatura
S7: `trade_count ≤ P25` ∧ `volume ≥ P90` (percentis por loop de rank sobre
janela de 150–200 barras) ∧ `|delta|/volume ≥ 0,6` ∧ corpo ≥ 0,65 do range com
close na ponta coerente. `plotshape` só no fechamento — nunca em preview, nunca
repintando. Follow-through (barra seguinte devolvendo > 50% do corpo cancela)
aparece como marca separada, para o trader ver a diferença entre "disparou" e
"confirmou".

### 4. `canal_t.pine`
Os quatro contadores da subclassificação rotacional/drive (razão pullback/perna
via `valuewhen` sobre pivôs, toques no AVWAP, sobreposição média das últimas 12
barras via `sma(x,12)`, contra-rotações), num label de score. **Sem veredito
agregado** — os quatro números, o trader decide. É a mesma disciplina do HUD.

## Idioma da casa (obrigatório)

`copilot.pine` (233 linhas) é o padrão-ouro; `zigzag.pine` (33) é o mínimo:

- `//@version=5` na linha 1.
- **Cabeçalho de comentário para o trader** antes do `indicator(...)`:
  o que cada marca significa, **o que a ausência de marca significa**,
  calibração honesta medida, e a limitação ("Information, not advice: the last
  decision belongs to the trader").
- Todos os `input.*` juntos, **agrupados e numerados na ordem de leitura**,
  com títulos que carregam grupo e unidade (`"2 Structure: retest tolerance
  (×ATR)"`), `minval`/`maxval`/`step` **sempre** preenchidos (o diálogo gera
  slider a partir deles), e os `"Display: …"` por último.
- Seções separadas por régua de comentário; cálculo antes, desenho depois.
- **Todo `ta.*` stateful e toda leitura `[n]` hoisted para o top level**, com a
  justificativa escrita no código (`and` curto-circuita; kernels têm identidade
  por call-site).
- Guarda de `na` canônica: `if not na(ph)`.
- Toggles de display gateiam **só os sites de desenho**, nunca o cálculo.
- Marcas nunca cobrem a vela: `location.abovebar`/`belowbar` ou âncora em
  `hi + atr`. Paleta binária consistente (vermelho = venda, teal = compra).
- Comentários explicam a **decisão** e o **porquê**, nunca parafraseiam o
  código.

## Critérios de aceite

1. Os quatro scripts compilam (o guard `embedded_scripts_compile_against_the_
   dialect` cobre) e cada um tem **teste-pino** + **teste de semântica com par
   acende/não-acende**.
2. Nenhum script depende de builtin inerte (`bgcolor`, `barcolor`,
   `alertcondition`) para expressar sinal.
3. Nenhum script repinta: marcas só em barra fechada.
4. **Orçamento de performance reportado no PR**: ≤ 50 µs por commit run é o
   alvo; 200 µs é hard fail em review (`docs/indicator-system-plan.md:617`).
   Medir com `crates/pine/benches/interp.rs` (`cargo bench -p quantick-pine`)
   ou adaptando `preview_cost.rs` (que é `#[ignore]`, diagnóstico, sem gate).
5. Todo `plot`/`plotshape` com `title=` estável.
6. Os limiares de partida vêm da §04 do operacional e aparecem como `input`
   com faixa, não como constante enterrada.

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto de performance declarado com ns/barra do bench.
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] `ui-harness`: os scripts são alcançáveis por
      `QUANTICK_INDICATOR_SCRIPTS_AUTOSTART=<nomes>` (hook existente) — citar na
      evidência.
- [ ] `visual-qa`: as marcas na tela, em dado denso e vazio, janela estreita e
      normal.
- [ ] `trader-ux-review`: as marcas informam sem cobrir preço nem barra em
      formação.
- [ ] PR aberto com CI verde. Merge não faz parte.
