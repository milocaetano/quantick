# Mission: copilot de contexto operacional (indicador "posso operar?")

Criar um indicador Pine embutido (`copilot.pine`) que diz ao trader **quando o
contexto permite operar reversão e quando não** — filtro de regime
range×tendência pintado no fundo do gráfico — e, dentro do contexto válido,
marca os setups da estratégia dele: topo/fundo duplo (pivots com tolerância
ATR) + divergência de CVD no segundo toque + agressão anômala (z-score do
delta) + absorção (esforço×resultado). Todos os limiares como `input` para
calibração sem tocar código.

## Classificação

- Mudança de código: sim (script embutido + registro em `library.rs`).
- Adiciona capability: sim — indicador novo, aditivo, edits registration-only
  (`new-extension`).
- Toca superfície visível: sim — renderiza no gráfico (`ui-harness`,
  `visual-qa`, `trader-ux-review`).
- Hot path: não — o script roda no indicator host por barra fechada/preview,
  nunca per-trade/per-frame. Declarar taxas no plano mesmo assim.
- Engine/determinismo: não toca o engine; o teste
  `embedded_scripts_compile_against_the_dialect` já guarda a compilação.

## Critérios de aceitação

1. `copilot.pine` existe em `crates/app/scripts/`, embutido via
   `EMBEDDED` em `crates/app/src/indicators/library.rs` (edit
   registration-only); `embedded_scripts_compile_against_the_dialect` verde.
2. Filtro de regime: fundo/bandas indicando OPERA (range) vs NÃO OPERA
   (tendência), critério `highest-lowest vs k·ATR`, parâmetros como `input`.
3. Detector de topo/fundo duplo: pivots (`ta.pivothigh/pivotlow`) com
   tolerância relativa a ATR e distância mínima em barras, marcados com
   `line`/`label`.
4. Sinal de setup (`plotshape`) apenas quando, no toque: divergência de CVD
   vs pivô anterior + |z-score do delta| acima do limiar + razão
   esforço×resultado acima do percentil configurado — tudo `input`.
5. Nenhum limiar mágico no código: todo cutoff é `input` com default
   documentado no próprio script.
6. Performance declarada no PR: caminho por-barra no host, zero custo
   per-trade/per-frame novo.
7. Gates padrão: `fmt`/`clippy`/`build`/`test` verdes após rebase em `main`
   atualizado; `arch-review` sem Blocker/Should-fix pendente; `ui-harness`
   (superfície alcançável por hook), `visual-qa` PASS, `trader-ux-review`
   sem Blocker; **PR aberto** com evidências no corpo (merge não faz parte).

## Ground

- Worktree: `../quantick-worktrees/feat-copilot-indicator`
- Branch: `feat/copilot-indicator` (de `origin/main` @ 679408b)
