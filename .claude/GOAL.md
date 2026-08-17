# Missão: o popup de propriedades abre onde o trader o deixou

O popup de propriedades de desenho (o *drawing inspector*, uma janela só,
compartilhada por todas as ferramentas da barra) passa a lembrar a posição
para onde o trader o arrastou: a posição é gravada no workspace atual
(`ui-state.toml`) sem exigir Save explícito, vale para qualquer desenho
selecionado depois, e sobrevive a fechar e reabrir o app.

## Estado de hoje

- A janela é `egui::Window::new(...).id("drawing_inspector")` em
  `crates/app/src/app.rs:5954`.
- Já existe posição manual **em memória de sessão**: `inspector_pos` e
  `inspector_moved` (`app.rs:761`, `app.rs:796`), escritas ao arrastar a barra
  de título (`app.rs:5241`) e zeradas por duplo-clique (`app.rs:5237`).
- Nada disso chega ao workspace: `SavedChrome` (`crates/app/src/ui_state.rs:208`)
  guarda dock, rail, favoritos, timezone — a posição do popup falta.
- O reparo de posição já existe: `clamp_into_chart` (`app.rs:347`) devolve para
  dentro do pane uma posição que não cabe mais.

## Escopo

**Dentro**: o popup de propriedades de desenho, uma posição só, global a todas
as ferramentas da barra (é o que a descrição pede: "se eu clicar em algum outro
desenho ou no mesmo, a popup vai abrir onde abriu do último desenho").

**Fora** (decidido com o Camilo): a janela "Drawn objects", as settings de
indicador, de footprint e a aparência de candle. E o estado *pinned* do
inspector — a missão é a posição da janela flutuante, não se ela flutua.

## Critérios de aceitação

### Específicos da missão

1. **A posição vai para o workspace.** Campo novo em `SavedChrome`, com
   `#[serde(default)]`: um `ui-state.toml` escrito antes desta mudança
   continua carregando inteiro, e sem posição lembrada o comportamento é o de
   hoje (colocação automática ao lado do objeto).
2. **Autosave, não Save explícito.** Soltar o arrasto grava o workspace
   sozinho. Sem aviso "Workspace saved" na barra de status a cada arrasto — o
   aviso é do gesto deliberado, e repeti-lo aqui vira ruído.
3. **Vale para o próximo desenho, qualquer um.** Com posição lembrada,
   selecionar outro desenho (ou o mesmo de novo) abre o popup lá, não ao lado
   do objeto. Provado por teste headless.
4. **Sobrevive ao restart.** Ida e volta pelo `ui-state.toml` provada por
   teste: gravar, recarregar, e o popup abrir na posição gravada.
5. **O caminho de volta continua existindo e também persiste.** Duplo-clique na
   barra do popup volta à colocação automática *e* apaga a memória no
   workspace — senão o reset dura só até o próximo launch.
6. **Posição impossível é reparada, nunca obedecida.** Restaurar numa janela
   menor, ou com o rail em outra doca, clampa para dentro do chart pelo caminho
   que já existe; o popup nunca abre fora da área visível. Teste com janela
   menor no restore.
7. **Alcançável por hook de env.** Um hook novo posiciona o inspector sem mão
   humana, para o visual-qa provar o restauro; registrado no `ui-harness`.

### Portões padrão (mudança de código, superfície visível)

- [ ] `cargo fmt --all -- --check` verde
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` verde
- [ ] `cargo build --workspace` verde
- [ ] `cargo test --workspace` verde
- [ ] Impacto de performance declarado por caminho tocado (per-frame: o desenho
      da barra de título do inspector; raro: a gravação do workspace no fim do
      arrasto), com medição `APP_HEALTH_SUMMARY` contra um controle em `main`
- [ ] `arch-review` rodado sobre `git diff main...HEAD`, todo Blocker e
      Should-fix resolvido ou adiado explicitamente no corpo do PR
- [ ] `visual-qa` com todas as superfícies PASS (Camilo autorizou abrir o app)
- [ ] `trader-ux-review` sem Blocker em aberto
- [ ] PR aberto com CI verde e a evidência no corpo. **Merge não faz parte da
      missão** — é sempre chamada do Camilo.

## Riscos conhecidos

- **Disco**: C: em 9,8 GB livres (98% cheio) no início da missão. Sete
  worktrees no disco, três delas de PRs já mergeados (`feat-force-bar`,
  `feat-strategy-anchors`, `feat-workspace-memory`) carregando `target/`. A
  limpeza prescrita pelo CLAUDE.md foi bloqueada pelo classificador; se um
  build morrer por falta de espaço, é aqui.
