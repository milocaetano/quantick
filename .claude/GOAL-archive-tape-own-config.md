# Missão — o tape é um painel, e responde por si

**Objetivo em uma frase**: separar a configuração da fita ao vivo (live lane)
da configuração do gráfico — heatmap e bolhas passam a ser ligados por painel,
e o botão direito sobre a fita abre um menu próprio onde a janela de tempo
deixa de ser só um multiplicador e vira **automático (segue as barras) ou um
tempo fixo** (15 s … 5 min, ou custom).

Branch: `feat/tape-own-config` · worktree `../quantick-worktrees/feat-tape-own-config`

## O que já existe (levantado antes de cortar o worktree)

- O render **já distingue os dois painéis por primitiva**:
  `orderflow_render.rs:1652` (`layout.in_lane(trade.x)`), `lane_rect()` /
  `history_rect()` e o campo `AggressionPrimitive::live`. O discriminador
  existe; falta o *switch*.
- A lane **já é um painel com config própria**: `LiveLaneStyle`
  (`orderflow/config.rs:665`) — largura, zoom de tempo, cluster, raio,
  marcas. É a casa natural das chaves novas.
- O "automático" que o usuário descreve **já é o comportamento de hoje**:
  `reserved_span_ms()` (`orderflow/timeline.rs:54`) — mediana das 8 últimas
  barras fechadas, piso de 4 s.
- Hoje **um switch só serve os dois painéis**: `ChartLayer::Heatmap` e
  `ChartLayer::Bubbles` (`pane.rs:1173-1174`) resolvem para
  `config.show_depth` / `config.show_aggressions`, que valem para o corpo do
  gráfico *e* para a fita.
- **Perigo de dois donos**: `time_zoom` já é editado pelo slider do dock
  (`orderflow_view.rs:1027`) e pelo gesto (`zoom_live_lane`,
  `orderflow_view.rs:381`). Uma janela absoluta *ao lado* dele seria um segundo
  dono do mesmo número.

## Decisões do usuário (tomadas antes de cortar o worktree)

1. **Submenu com presets + custom** no botão direito da fita — `auto (≈ 8 s)`,
   15 s, 30 s, 1 min, 2 min, 5 min, `custom…`. O rótulo do auto declara o
   número que está valendo.
2. **Um modo só, um dono só**: a janela vira
   `LaneWindow { Auto { zoom }, Fixed { ms } }`. O gesto de arrasto ajusta o
   zoom em `Auto` e os milissegundos em `Fixed`.
3. **Menu do tape *e* dock**: as chaves novas aparecem nos dois caminhos,
   lendo o mesmo campo — nenhuma cópia de estado.

## Critérios de aceite

### Específicos da missão

1. **Visibilidade por painel.** Quatro chaves onde hoje há duas: heatmap no
   gráfico / heatmap no tape / bolhas no gráfico / bolhas no tape. Desligar no
   gráfico não apaga um pixel da fita, e vice-versa. Provado por teste que
   desliga um lado e conta as primitivas desenhadas do outro.
2. **Nenhum switch mata o dado.** `HeatmapConfig::wants_projection`
   (`orderflow/config.rs:1042`) hoje é
   `depth_visible() || show_aggressions || projection_demand`; passa a
   considerar as quatro chaves. Teste: só o tape ligado ⇒ a projeção continua
   sendo pedida (desligar no gráfico nunca esvazia a fita).
3. **O botão direito sobre a fita abre o menu do tape**, o do corpo abre o
   menu do gráfico. O hit-test usa a geometria que já existe
   (`lane_left_x()` / `lane_rect()`), nunca uma segunda cópia do retângulo.
   Teste que prova qual menu abre de cada lado do divisor, inclusive com a
   lane ausente (sem fita ⇒ menu do gráfico, sempre).
4. **`LaneWindow` com um dono só.** Default `Auto`, e o modo automático
   reproduz **exatamente** o comportamento de hoje — mesmos clamps
   (`MIN/MAX_LIVE_LANE_WINDOW_MS`), mesmos números. Os testes que já existem
   em `orderflow/config.rs:1269-1320` continuam valendo sem alteração de
   expectativa; um teste novo trava `Fixed`.
5. **Presets e custom, com o número à vista.** `auto` mostra a duração vigente
   ao lado do rótulo; os presets são exatos; o custom aceita segundos e é
   clampado na faixa que o renderer suporta. Teste de formatação do rótulo
   (`8 s`, `1 min`, `1 min 30 s`) e de clamp do custom.
6. **O gesto continua funcionando e não troca de modo por acidente.**
   Arrastar/rolar sobre a fita ajusta o zoom em `Auto` e os milissegundos em
   `Fixed`. Teste dos dois caminhos.
7. **O cluster acompanha a janela nos dois modos.** `effective_cluster_ms`
   hoje escala por `time_zoom`; em `Fixed` o fator equivalente vem da razão
   entre a referência automática e os ms fixos — senão uma fita de 2 min vira
   um borrão. Teste provando que 2 min agrupa mais que 8 s.
8. **Compatibilidade para trás.** Um `chart-layers.toml` e um preset de
   order-flow escritos pela versão de hoje abrem com **o mesmo desenho**:
   chave nova ausente significa "como era". Teste de round-trip e teste de
   arquivo antigo.

### Portões injetados pelo tipo de trabalho

| Portão | Por quê | Evidência exigida |
| --- | --- | --- |
| Quatro checks verdes | qualquer código | saída dos quatro comandos após rebase em `main` |
| Performance declarada | toca caminho quente | cada caminho tocado classificado por taxa (per-trade / per-depth / per-frame / raro) **no plano**, não na revisão |
| Performance medida | render per-frame + projeção per-trade | `APP_HEALTH_SUMMARY` (fps / frame_avg) sob fita densa contra o controle em `../quantick-worktrees/perf-control-main`; números no corpo do PR |
| Defaults preservam o hoje | capacidade aditiva (`new-extension`) | teste provando que uma instalação limpa e um arquivo antigo desenham o que desenhavam; blast radius (arquivos criados × editados) no corpo do PR |
| `ui-harness` | superfície nova (menu do tape) | hook de env alcançando o menu do tape, adicionado nesta mesma mudança |
| `visual-qa` | mudança visível | todas as superfícies PASS, ou defeito aceito explicitamente |
| `trader-ux-review` | o trader mexe nisso no meio da sessão | nenhum Blocker em aberto |
| `arch-review` | antes do PR | todo Blocker/Should-fix resolvido, ou adiado com registro no corpo do PR |
| PR aberto | fim da missão | URL do PR impresso. **Merge nunca faz parte** |

## Fora de escopo (dito explicitamente)

- Separar as **demais** camadas de fluxo por painel (gap boundaries, badge de
  status, footprint). O pedido nomeia heatmap e bolhas; o resto fica.
- Mexer no `live strip` (a faixa fina junto ao eixo de preço) — outra coisa,
  outro switch, já tem o seu.
