# Mission: o cockpit volta como o trader deixou

**Objetivo:** quantick reabre com o cockpit inteiro — indicadores, favoritos da
barra, cores das ferramentas, camadas, footprint, símbolos — independente do
diretório de onde o app foi lançado; e o trader pode exportar esse cockpit para
um arquivo em Documentos, importá-lo de volta e reabrir os recentes.

Branch: `feat/workspace-memory` · Worktree: `../quantick-worktrees/feat-workspace-memory`

## Diagnóstico (a causa, não o sintoma)

O menu Workspace **já funciona** (Save workspace, Save as…, Open, Delete, Save
on exit). O relato "não memoriza mais nada" tem duas causas independentes:

1. **Todo store de estado do usuário resolve para caminho relativo ao CWD.**
   `default_path()` faz `PathBuf::from("ui-state.toml")` em nove módulos. Cada
   diretório de lançamento (raiz do repo, worktree, `target/release`, atalho)
   tem seu próprio cockpit, e nenhum deles é *o* cockpit. É exatamente o bug que
   `crates/app/src/paper_home.rs` já diagnosticou e resolveu — mas só para o
   journal de paper trading; o cockpit ficou para trás.

   | Store | Arquivo | O que o trader perde |
   | --- | --- | --- |
   | `ui_state` | `ui-state.toml` | abas, favoritos da barra, rail, timezone, dock, janela |
   | `indicators/state_file` | `indicators-state.toml` | quais indicadores, com seus inputs |
   | `indicators/preset_file` | `indicator-presets.toml` | presets de indicador |
   | `chart_layers` | `chart-layers.toml` | camadas ligadas/desligadas |
   | `drawings/presets` | `quantick-drawing-presets.toml` | cores e larguras das ferramentas |
   | `footprint_config` | `footprint-settings.toml` | ajustes do footprint |
   | `footprint_presets` | `footprint-presets.toml` | presets do footprint |
   | `symbols_file` | `quantick-symbols.toml` | símbolos adicionados à mão |
   | `paper_state` | `paper-state.toml` | (fora do bundle, mas mesmo bug de home) |

2. **O "workspace" é parcial.** `ui-state.toml` guarda abas + chrome. Os
   indicadores, camadas, cores de desenho e footprint vivem em arquivos
   irmãos que os *named arrangements* não capturam — então mesmo com o menu
   funcionando, abrir um workspace salvo traz metade do cockpit, e o trader
   não tem como ver qual metade falta.

## Decisões do trader (perguntadas, não assumidas)

- **Conteúdo do bundle:** cockpit completo — abas+chrome, indicadores ativos com
  inputs, presets de indicador, camadas, presets/cores de desenho, footprint
  (settings+presets), símbolos adicionados. **Fora:** paper trading (posições e
  histórico) — é resultado, não arranjo de tela.
- **UI:** exportar para Documentos, importar de arquivo, lista de recentes, e
  uma entrada que revela/abre o home durável.

## Critérios de aceitação

### Específicos da missão

1. **Um home durável, cwd-independente.** Os oito stores do cockpit resolvem
   para `Documents/Quantick/` (a shelf que `paper_home::shelf_dir()` já nomeia)
   em vez do CWD. Teste: lançar de dois diretórios diferentes com
   `APP_HEALTH_SUMMARY` e provar que o mesmo cockpit volta nos dois.
2. **A resolução preserva o que já existe.** A ordem `env var > pick explícito >
   home durável` de `paper_home` é a mesma aqui: cada `QUANTICK_*` continua
   vencendo, então toda validação de QA e todo autostart seguem apontando para
   scratch. Teste por store.
3. **Consolidação uma vez, copiando — nunca movendo.** O estado que hoje vive em
   `C:\src\quantick\*.toml` é copiado para o home no primeiro lançamento, com
   marcador no próprio home (como `.consolidated`), de modo que a
   uma-vez-só valha de qualquer diretório de lançamento. Nada é apagado.
   Teste: consolidar duas vezes é inofensivo; o arquivo de origem sobrevive.
4. **Um bundle autocontido.** Um workspace exportado é *um* arquivo TOML
   versionado com as sete seções acima. Round-trip: exportar → mudar tudo →
   importar → o cockpit volta idêntico. Teste unitário sobre o bundle.
5. **Importar é tudo-ou-nada.** Um bundle ilegível, de versão desconhecida ou
   com seção corrompida é recusado com motivo na status line e **nada** é
   aplicado — meio cockpit é pior que nenhum, porque o trader não vê a metade
   que falta. Teste do caminho de recusa.
6. **O menu Workspace ganha as quatro entradas** — Exportar para arquivo…,
   Importar de arquivo…, Abrir recente ▸, Mostrar onde está salvo — usando o
   `rfd` que o app já traz, fora da thread de UI (o diálogo nativo nunca
   bloqueia um frame).
7. **A lista de recentes sobrevive ao restart** e descarta com log a entrada
   cujo arquivo sumiu, pelo mesmo princípio de `Workspace::restore`: todo nome
   no menu abre alguma coisa.

### Gates padrão (injetados pela classificação)

**Mudança de código**
- [ ] quatro checks verdes após rebase no `main` atual: `cargo fmt --all --
      check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo build --workspace`, `cargo test --workspace`
- [ ] **impacto de performance declarado** no plano: todo caminho tocado é
      *rare* (startup e save event-driven/debounced). Nada per-trade,
      per-depth ou per-frame. A consolidação roda uma vez, antes da primeira
      frame; o custo declarado é de startup e é medido.
- [ ] `arch-review` rodado, todo Blocker/Should-fix resolvido ou deferido no
      corpo do PR
- [ ] **PR aberto** — a missão não termina antes disso; merge nunca faz parte

**Hot path** — não se aplica (nenhum caminho per-trade/per-depth/per-frame é
tocado). Mesmo assim, como a consolidação entra no startup, medir o tempo de
startup contra um run de controle no `main` e pôr os números no PR.

**Visível ao usuário**
- [ ] `ui-harness`: hook de env para cada superfície nova (menu Workspace
      expandido, diálogo de importar, lista de recentes), adicionado na mesma
      mudança
- [ ] `visual-qa`: todas as superfícies PASS, ou defeito explicitamente aceito
- [ ] `trader-ux-review` sem Blocker em aberto

**Adiciona capacidade**
- [ ] `new-extension`: porta nomeada (o bundle é um trait/porta que cada store
      implementa — capturar e aplicar), edições só de registro, defaults
      preservam o comportamento de hoje, segunda implementação falsa testada,
      raio de alcance (arquivos criados vs. editados) no corpo do PR

## Fora de escopo

- Migrar o paper trading para o bundle — `paper_home.rs` já resolveu o home
  dele, e posições simuladas não são arranjo de tela.
- Redesenhar o menu Workspace existente. Save/Save as…/Open/Delete/Save on exit
  continuam como estão; as quatro entradas novas se somam a eles.
- Workspace por aba ou por símbolo. Indicadores e camadas são globais hoje e
  continuam.
