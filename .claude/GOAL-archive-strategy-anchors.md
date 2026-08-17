# GOAL — strategy anchors: estratégia semi-automática ancorada em desenho

Branch: `feat/strategy-anchors` · worktree `../quantick-worktrees/feat-strategy-anchors`

## Missão

Armar, num desenho do gráfico (retângulo), uma estratégia parametrizada de
barra de força que executa sozinha no paper trading — o humano define a
região de congestão, o bot puxa o gatilho que o tempo de reação humano não
alcança. Tudo declarativo (banco de presets + desenhos nomeados + "armar
preset X no desenho Y") para que uma IA futura escreva essas estratégias em
português emitindo exatamente o que o menu de hoje emite.

## Decisões tomadas com o Camilo (2026-08-17)

1. **Portão da Onda 0**: seguir agora. Enquadramento registrado: a região é
   julgamento humano não mecanizável, então replay + paper com o bot É o
   instrumento de medição do setup híbrido (o harness sozinho não o mede).
   Esta ferramenta mede; não promete edge.
2. **Escopo**: um PR vertical, commits temáticos (fundação → crate → runtime
   → banco → backtest → docs).
3. **Rearme default**: um tiro e desarma (`OneShot`); `Auto` existe como
   opção do preset.

## Semântica default (tudo parametrizável no preset)

- Gatilho: barra **fechada** que é barra de força do lado configurado
  (régua do `force_bar.pine`: corpo entre `min_factor`× e `max_factor`× a
  SMA de `len` corpos, janela cheia, incluindo a barra atual; corpo acima do
  teto é exaustão e **não** dispara; doji não tem lado e não dispara) com
  `close` dentro da região de preço do desenho e barra dentro da janela de
  slots do desenho.
- Entrada: `PlaceMarket` com bracket, executa no próximo print (modelo do sim).
- TP = close + `tp_mult` × range(BF) na direção; SL = close − `sl_mult` ×
  range(BF) contra; defaults 1,0 / 1,0.
- Só atira com a conta flat (posição manual aberta também bloqueia).
- Desarme automático com motivo visível: seek/reset de timeline, troca de
  spec de barra, troca de símbolo, delete do desenho, rejeição do sim.
- Instâncias armadas não persistem entre sessões (coerente com desenhos e
  posições); só o banco de presets persiste.

## Critérios de aceite

1. Fundação de desenhos: `DrawingId` estável (sobrevive a delete/reorder —
   teste) + nome editável usado nos labels/manager.
2. Menu de botão direito por-desenho ("Add strategy…", Rename, ações
   existentes) com hook ui-harness novo adicionado nesta mudança.
3. Crate `strategy` (package `quantick-strategy`), pura e determinística,
   test-first: detector de força, `Region`, máquina de estados
   Armed→Fired→InPosition→Done/Armed, brackets projetados; golden test
   (mesma fixture → mesmos comandos, duas rodadas idênticas); porta de
   gatilho (`Trigger`) provada com implementação fake; os **três arquivos de
   contrato** editados (Cargo.toml raiz, CLAUDE.md, workspace_deps.rs — a
   whitelist é iterada, esquecer não quebra teste).
4. Runtime no app: instância armada emite `PlaceMarket` + bracket no paper
   trading pelo funil existente; badge de estado no desenho; desarmes de
   segurança acima; uma operação por instância por vez.
5. Banco: `quantick-strategies.toml` versionado (env override
   `QUANTICK_STRATEGY_PRESETS`), presets salvos e recarregados.
6. Backtest: impl do strategy port reusando o mesmo kernel + teste de
   integração; linha na tabela de estratégias do CLI.
7. Docs: `docs/ux/paper-trading.md` §10 revogado por escrito; doc da
   feature; CLAUDE.md.
8. Quatro checks verdes pós-rebase em `origin/main`.
9. Performance declarada por caminho: kernel per-bar-close, badge per-frame
   O(instâncias armadas), menu por-interação; nada novo per-print/per-depth.
10. `arch-review` sobre `git diff main...HEAD` com Blocker/Should-fix
    resolvidos ou deferidos por escrito no PR.
11. `visual-qa` + `trader-ux-review`: PASS, ou BLOCKED declarado (abrir o
    app exige autorização explícita do Camilo — regra da casa).
12. PR aberto com CI verde e evidência no corpo. **Merge não faz parte**
    (é de outro agente).

## Fora de escopo (registrado)

- Execução real (não existe rota de ordem; bridge MT5 é feed-only).
- BEI (S7 do operacional) como gatilho — a porta `Trigger` a recebe depois.
- IA em linguagem natural — esta missão só garante o formato declarativo.
- Persistência de instâncias armadas entre sessões.
