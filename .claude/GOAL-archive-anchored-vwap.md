# Mission: Anchored VWAP indicator

Criar o indicador **Anchored VWAP** estilo TradingView:

- Ancoragem por **clique-direito** em qualquer ponto do gráfico ("Anchor VWAP here").
- Ancoragem por **ícone na toolbar** → próximo clique no gráfico define a âncora.
- Settings estilo TradingView: cor, espessura, **source** (open/high/low/close/hl2/hlc3/ohlc4) e **bandas de desvio padrão** (on/off + multiplicadores), persistidos.
- Visual definido com um **agente designer**.

## Acceptance criteria

1. Kernel determinístico test-first no crate `indicators`: fixture + saída esperada antes do código; sources configuráveis; bandas de stddev; golden test.
2. Dois fluxos de ancoragem funcionais (clique-direito e ícone→clique); âncora reposicionável/removível.
3. Settings TradingView-like persistidos entre sessões.
4. Capability aditiva (`new-extension`): port nomeado, edits de registro apenas, blast radius no PR.
5. Agente designer consultado; `ui-harness` hooks para toda superfície nova; `visual-qa` todas PASS; `trader-ux-review` sem Blocker.
6. Performance: paths classificados por taxa (per-trade/per-frame/rare); `APP_HEALTH_SUMMARY` fps/frame_avg sob tape densa vs. controle em `main`, números no PR.
7. 4 checks verdes após rebase em `main`; `arch-review` com Blocker/Should-fix resolvidos ou deferidos no PR; **PR aberto** (merge fora do escopo).

Branch: `feat/anchored-vwap` — worktree `../quantick-worktrees/feat-anchored-vwap`.
