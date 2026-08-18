# GOAL — ferramentas que param de pedir retrabalho

**Missão**: quatro incômodos das ferramentas de desenho, num só passe — o
ícone do Fibonacci lê como o do TradingView, a retração desenha a partir do
**primeiro** clique (hoje as linhas nascem no segundo), a configuração de uma
ferramenta (níveis, cores, rótulos, estilo) pode ser salva como padrão e
restaurada de fábrica com um clique, e a nota de texto abre já digitável com
sua barra de configuração embaixo.

Branch: `feat/fib-defaults-and-inline-text` ·
worktree `../quantick-worktrees/feat-fib-defaults-and-inline-text`

## O que o levantamento em `main` já provou

- `fib.rs:450 level_price` **já** põe 0 no ponto solto e 1 no ponto inicial —
  a numeração da imagem já é a de hoje. O que trai é o span: `Extend::Forward`
  é o default (`fib.rs:148`) e projeta `(âncora mais à direita → borda)`, ou
  seja, as linhas nascem no segundo clique.
- O preview do draft **já** completa a geometria com o ponto sob o cursor
  (`pane.rs:6119-6124`), então os níveis aparecem durante o arrasto — mas do
  cursor para a direita, não do primeiro clique até ele.
- `PresetHost` já guarda estilo padrão por ferramenta e preset **nomeado**
  padrão (`mod.rs:113-134`, aplicados em `pane.rs:3410`). Falta o gesto de um
  clique: salvar a configuração inteira como padrão sem batizar preset.
- `FibPayload::export_preset` (`fib.rs:378`) já serializa níveis, cores,
  banda, rótulos e `extend` — o template pedido cabe nele.
- A nota de texto hoje só é editável pelo inspector (`text.rs:opens_settings_on_place`).

## Critérios de aceite

1. **Ícone**: Fib retracement e Fib extension no trilho, no flyout e no
   favorito desenham as âncoras como o TradingView. O dado novo é do
   registro (port de ícone), nunca um caso especial no chrome; teste de
   contrato cobre todo ícone declarado.
2. **Retração a partir do primeiro clique**: durante o arrasto e no objeto
   pronto, os níveis correm do primeiro clique até o segundo ponto. Teste
   headless fixa o default por `FibKind`; screenshot com
   `QUANTICK_DRAWING_DRAFT` prova o preview em voo.
3. **Padrão salvável**: um clique grava a configuração completa da ferramenta
   (níveis, cor por nível, rótulos, `extend`, banda, estilo) como padrão de
   novos objetos, e outro restaura o de fábrica. Objetos já na tela nunca são
   repintados por isso. Round-trip provado no `PresetStore`, aplicação provada
   na criação, e um host falso segundo implementador testado.
4. **Texto inline**: colocar a nota abre o campo na âncora com o cursor já
   dentro e a barra de configuração embaixo; o que se digita vai para o
   payload; Esc/clique fora encerra sem perder o texto. Hook de env alcança a
   superfície.
5. **Quatro checks verdes** após rebase em `origin/main` atualizado.
6. **Impacto de performance declarado** por caminho tocado (per-frame: paint
   do Fib e da nota; raro: store de presets e chrome do inspector), com
   `APP_HEALTH_SUMMARY` (fps / frame_avg) da branch contra um controle em
   `main` na mesma janela.
7. **arch-review** rodado sobre `git diff main...HEAD`, todo Blocker e
   Should-fix resolvido — ou deferido explicitamente no corpo do PR.
8. **visual-qa** com todas as superfícies PASS (ou defeito aceito por escrito)
   e **trader-ux-review** sem Blocker em aberto.
9. **PR aberto** com as evidências e o blast radius (arquivos adicionados vs.
   editados) no corpo. Merge não faz parte da missão.

## Evidência (fechamento)

1. **Ícone** ✔ — `IconDots` no port (default `&[]`), duas Fib declaram 2 e 3
   âncoras; testes `icon_dots_stay_in_the_unit_square_and_only_accompany_strokes`
   e o contrato dos strokes. Lido no tamanho real e redesenhado depois da
   primeira captura (4 linhas em 14 px eram uma mancha).
2. **Retração a partir do primeiro clique** ✔ — `Extend::for_kind`;
   `a_retracement_being_dragged_draws_from_the_first_click`,
   `an_extension_still_projects_from_its_last_anchor`,
   `each_fib_kind_opens_reaching_current_price_from_its_own_start`. Captura do
   arrasto com `QUANTICK_DRAWING_DRAFT=1`.
3. **Padrão salvável** ✔ — `save_tool_default` / `reset_tool_default` /
   `has_saved_default` + slot `config` no `PresetStore`; round-trip, absence,
   arquivo antigo, fluxo completo com um segundo host (`MemoryPresetHost`) e
   precedência do preset nomeado. Capturas das abas Levels e Style.
4. **Texto inline** ✔ — `inline_text` / `set_inline_text` / `holds_text` no
   port; editor com moldura, flip e clamp; duplo clique reabre; undo de uma
   entrada; identidade de aba/pane. Seis testes, hook `QUANTICK_TEXT_NOTE`.
5. **Quatro checks** ✔ — fmt/clippy/build/test verdes (1477 no app).
6. **Performance** ✔ — `frame_cpu_ms` mediana: main 2,747 / 2,658 ms contra
   branch 2,744 / 2,769 ms, alternado A/B/A/B, 22 objetos, fps 59 nas quatro.
   A corrida mais cara da branch teve tape 3,6× mais denso (3,05 vs 0,84
   trades/s).
7. **arch-review** ✔ — step 0 (`code-review high`) trouxe 15 achados; 13
   corrigidos, 1 deferido (persistência 2–3 gravações por clique, caminho
   raro), 1 já corrigido antes do relatório chegar.
8. **visual-qa / trader-ux-review** ✔ — 5 superfícies capturadas com fps 58–59;
   3 defeitos achados e corrigidos (placeholder duplicado, campo sem moldura,
   rótulo ambíguo), 1 achado de UX corrigido (o reset não dizia que limpa a
   escolha de preset padrão).
9. **PR** — aberto no fim desta sessão.
