# Mission — popup de indicador direta de usar

**Objetivo:** dar dois cliques em qualquer representação de um indicador abre a
popup de configuração, e essa popup deixa alterar as informações do indicador
(parâmetros e estilo) com efeito ao vivo no chart.

Branch: `feat/indicator-settings-popup`
Worktree: `../quantick-worktrees/feat-indicator-settings-popup`

## Decisões do usuário

- Escopo: **gesto + popup** (os dois).
- Aplicação: **ao vivo enquanto edita**, `Cancel` restaura o estado de abertura,
  `Ok` confirma.

## Ponto de partida (mapeado antes de começar)

- `indicator_legend.rs:143` — duplo-clique já existe, mas só no *texto* do nome
  na legenda de overlay.
- `indicator_panel.rs:46` — popup existe: `egui::Window` única, grade de
  `InputSpec` → widget, botões `Apply`/`Close`. Sem abas, sem estilo por plot,
  sem rollback.
- `indicator_render.rs:356` / `:381` — label dentro da sub-pane e faixa
  colapsada são `painter.text` puros, sem `Response`, logo sem gesto.
- Persistência dos inputs: `indicators/state_file.rs:93` (`indicators-state.toml`).
- **Gap de harness:** não existe `QUANTICK_INDICATOR_SETTINGS`; os análogos são
  `QUANTICK_FOOTPRINT_PANEL` e `QUANTICK_DRAWING_INSPECTOR`.

## Critérios de aceitação

### Específicos da missão

1. **Gesto abrangente** — duplo-clique abre a popup do indicador certo a partir
   de: linha inteira da legenda (não só o texto), label dentro da sub-pane,
   faixa do pane colapsado, e a série plotada no chart. Cada alvo tem teste.
2. **Popup com abas** — `Inputs` (os `InputSpec` de hoje) e `Style` (por plot:
   visível, cor, espessura). Indicador sem plots editáveis não ganha aba vazia.
3. **Ao vivo + rollback** — editar aplica no chart imediatamente; `Cancel`
   restaura exatamente os valores de abertura (inputs *e* estilo); `Ok` confirma
   e fecha. Teste cobre o rollback.
4. **Estilo persiste** — overrides de estilo sobrevivem a restart, junto dos
   inputs, sem quebrar arquivos de estado existentes (arquivo antigo carrega
   com defaults; sem bump de versão que invalide o do usuário).
5. **Defaults preservam o comportamento de hoje** — um indicador que ninguém
   editou renderiza pixel-a-pixel como antes.
6. **Nada vazou para os crates puros** — o override de estilo vive no `app`;
   `indicators` continua sem saber de UI. Direção de dependência intacta.

### Gates padrão — mudança de código

7. Quatro checks verdes após rebase no `main` atualizado: `cargo fmt --all --
   --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo build --workspace`, `cargo test --workspace`.
8. **Impacto de performance declarado no plano**, classificando cada caminho
   tocado por taxa (per-frame: desenho da legenda e da popup, hit-test do
   duplo-clique; rare: rebuild+replay do indicador ao editar).
9. `arch-review` rodado sobre `git diff main...HEAD`, todo Blocker e Should-fix
   resolvido ou explicitamente adiado no corpo do PR.
10. **PR aberto.** Merge não faz parte da missão.

### Gates padrão — caminho quente (per-frame)

11. Evidência medida, não crença: `APP_HEALTH_SUMMARY` (fps / frame_avg) sob
    tape denso comparado a um run de controle no `main`, números no corpo do PR.

### Gates padrão — superfície visível ao usuário

12. `ui-harness`: hook novo `QUANTICK_INDICATOR_SETTINGS` (abre a popup do
    indicador N), registrado no bloco de autostart de `app.rs` e na tabela do
    `SKILL.md`, na mesma mudança.
13. `visual-qa` com todas as surfaces PASS ou defeito aceito explicitamente.
14. `trader-ux-review` sem Blocker em aberto.

## Fora de escopo

- Mudar o motor de indicadores, a linguagem Pine ou o `Indicator` trait além do
  mínimo que o estilo exigir.
- Redesenhar a legenda ou o menu INDICATORS da toolbar.
- Merge do PR.
