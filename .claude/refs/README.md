# Referências externas (não compilam aqui)

Scripts e materiais de fora do quantick, guardados **como vieram**, para servir
de fonte quando algo for portado. Nada nesta pasta é compilado, testado ou
embarcado — os scripts do app vivem em `crates/app/scripts/` e sua cópia de
teste em `crates/pine/tests/corpus/ok/`.

| Arquivo | Origem | Estado |
| --- | --- | --- |
| `tv-barras-forca-exaustao.pine` | TradingView, Pine v5, autoria do Milo | não portado |

## `tv-barras-forca-exaustao.pine`

Marca três tipos de barra sobre o candle, com prioridade **maior barra >
exaustão > força**:

- **Força** — corpo entre 1,5× e 2,5× a média de 20 corpos (faixa fechada, com
  teto: a barra grande demais deixa de ser força e vira exaustão).
- **Exaustão** — corpo > 2,5× a média de 20 corpos **anteriores** (`bodySize[1]`,
  para a própria barra não inflar a referência que a julga).
- **Maior barra** — range (`high - low`) igual ao maior das últimas 20.

Note que força e exaustão medem **corpo** e a maior barra mede **range**: são
réguas diferentes, e é por isso que a terceira marca não é redundante com as
outras duas.

### O que quebra ao portar para o dialeto Quantick

Verificado contra `docs/pine-dialect.md` e `crates/pine/src/builtins.rs`:

1. **`barcolor()` é inerte** (aceito, warning) — e ele é *todo* o produto visual
   deste script. Pintar a vela não existe até as colunas de cor por barra
   chegarem. A porta é `plotshape`/`plotchar` com `na` mascarando, e como cor
   condicional em `plot` não dobra em constante, é **uma marca por cor**
   (o `copilot.pine` faz assim no semáforo).
2. **`alertcondition()` é inerte** e **`alert()` não existe** — `alert` não está
   em `BUILTINS`, então é `PINE_UNKNOWN_NAME` no load, não warning. O bloco
   inteiro de alertas sai; o sinal tem de viver na marca desenhada.
3. **`group=` e `inline=` não são reconhecidos** pelo compilador de inputs
   (`compile.rs` lê só `minval`/`maxval`/`step` por nome). A organização vira
   convenção de título, como no idioma da casa: `"1 Força: multiplicador mínimo"`.
4. **Ordem dos inputs é persistência posicional** — input novo vai sempre no
   fim, ou os valores salvos do usuário remapeiam.

O resto passa direto: `math.abs/min/max`, `ta.sma`, `ta.highest`, `bar_index`,
`barstate.isconfirmed`, ternário, `if/else if`, `na` e `not na(...)`.
