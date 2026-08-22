# Roadmap — as ferramentas que faltam para o Operacional Mark I

Índice dos pacotes de trabalho. Cada `WP-*.md` é uma missão fechada, no formato
GOAL da casa, pronta para virar um agente. As regras de execução estão em
[DISPATCH.md](DISPATCH.md) — leia antes de despachar qualquer coisa.

O operacional que este roadmap serve está em
`https://claude.ai/code/artifact/65214f4d-716b-4aa4-b8b7-7864a4880046`
(cópia local em `~/Documents/operacional-mark-i.html`). A §04 daquele documento
é a fonte única de números; nenhum pacote redefine constante por conta própria.

## O princípio de sequenciamento: valor de informação primeiro

A ordem natural seria construir do mais barato ao mais caro. Está errada aqui.

**Hoje não existe nenhuma evidência de que o operacional tem edge.** O que
existe é uma hipótese coerente: a aritmética de custos fecha, o dado necessário
existe na plataforma, as regras não se contradizem. Nada disso é medição. As
doze ferramentas de UI da lista original assumem que o Setup A funciona — e se
ele não funcionar, todas viram enfeite caro.

Por isso a Onda 0 é o **instrumento de medição**, não uma ferramenta de trading:
um harness que carrega as sessões gravadas, mecaniza as regras, e devolve
expectância, profit factor e drawdown. Ele é o único item do roadmap capaz de
dizer "pare, a hipótese está errada" — e é o mais barato de todos os jeitos de
descobrir isso. Um mês construindo painéis para um setup que acerta 30% é o
desperdício que este ordenamento existe para evitar.

A arquitetura já previa isso: o `CLAUDE.md` diz **"one engine, three consumers:
chart, backtest and bot"**. O backtest é o consumidor que nunca foi construído.

## Ondas

| Onda | Pacotes | Pergunta que responde |
| --- | --- | --- |
| **0 · Instrumento** | WP-01, WP-02, WP-03 | O Setup A tem edge mensurável nas sessões gravadas? |
| **1 · Olhos** | WP-04, WP-05 | O trader consegue ver os gates na tela, com o dado rotulado honestamente? |
| **2 · Régua** | WP-06, WP-07, WP-08 | O trader consegue medir o próprio desempenho em R e ser barrado quando viola critério? |
| **3 · Fita** | WP-09, WP-10 | Os sinais que o indicador não enxerga (prints, velocidade) existem? |
| **4 · Contexto** | WP-11, WP-12, WP-13 | As bordas do dia são medidas em vez de desenhadas no olho? |
| **5 · Integração** | WP-14 | O operacional inteiro cabe numa tela que bloqueia o erro? |

### O portão de decisão (entre a Onda 0 e a Onda 1)

Ao fim da Onda 0, com o relatório do harness na mão, uma das três:

1. **Expectância ≥ +0,15R com os gates da §12** → a hipótese sobreviveu. Segue
   para a Onda 1 e para o funil de validação (replay 1× manual → paper → real).
2. **Expectância entre −0,10R e +0,15R** → indeciso, que é o resultado mais
   provável com poucas sessões. Amplia a biblioteca (mais sessões via "Get
   data") e re-mede **antes** de construir a Onda 1. Não se constrói UI para
   um setup indeciso.
3. **Expectância < −0,10R de forma consistente** → a hipótese morreu como
   está. O roadmap **para** e a conversa volta para o desenho do operacional.
   Isso é sucesso do instrumento, não fracasso do projeto: descobriu-se em
   semanas o que custaria meses e dinheiro real.

Nenhum pacote da Onda 1 em diante é despachado antes desse portão. A exceção
declarada é o WP-05 (rotulagem de honestidade), que é infraestrutura de
verdade sobre o dado e vale independentemente do resultado.

## Índice dos pacotes

| # | Pacote | Onde vive | Esforço | Depende de |
| --- | --- | --- | --- | --- |
| WP-01 | Harness de backtest headless | `crates/backtest` (novo) | M | — |
| WP-02 | Classificador de regime + etiquetagem da biblioteca | `crates/backtest` | P | WP-01 |
| WP-03 | Setup A mecanizado + varredura walk-forward | `crates/backtest` | M | WP-01, WP-02 |
| WP-04 | Os quatro scripts `.pine` do dia 1 | `crates/app/scripts` + `crates/pine/tests` | M | — |
| WP-05 | Honestidade do lado inferido: estender a cobertura | `crates/app` | P | — |
| WP-06 | R-múltiplo e custo por trade no relatório | `crates/sim` + `crates/app` | M | — |
| WP-07 | HUD checklist determinístico v0 | `crates/app` (painel) | M | — |
| WP-08 | Porta de valores nomeados dos indicadores | `crates/indicators` + `crates/app` | M | — |
| WP-09 | Crate `quantick-tape` (fita pura) | `crates/tape` (novo) | M | — |
| WP-10 | Prints elefante + velocímetro no app | `crates/app` | P | WP-09 |
| WP-11 | Perfil da sessão com POC/VAH/VAL | `crates/app` | M | — |
| WP-12 | Absorção no footprint | `crates/engine` + `crates/app` | M | — |
| WP-13 | Assistente de Opening Balance nativo | `crates/app` (desenho) | M | — |
| WP-14 | HUD completo + session guard | `crates/app` | G | WP-07, WP-08 |

### Duas ordens que o reconhecimento corrigiu

- **O classificador de regime (WP-02) vem antes das regras (WP-03).** Escrito
  junto com elas, a tentação é ajustá-lo até os trades ficarem bons — que é
  curve-fitting com aparência de método. Escrito antes e congelado, vira régua
  independente.
- **O HUD v0 (WP-07) não depende da porta de valores nomeados (WP-08).** Os
  gates da v0 saem de `ChartState` + `PaperTrading`, que o app já tem no mesmo
  frame; a porta só é necessária quando um gate vier de script de usuário. Isso
  destrava o HUD para existir **antes** do paper ao vivo, que é onde ele
  importa — é a única defesa real contra violação de critério sob adrenalina.

Esforço: **P** ≈ um dia de agente, **M** ≈ dois a três, **G** ≈ mais que isso
com risco de arquitetura.

## Paralelismo seguro

Pacotes que **não** compartilham arquivo podem ser despachados juntos, cada um
no seu worktree. Os conflitos conhecidos:

- WP-01, WP-02 e WP-03 vivem no mesmo crate novo → **sequenciais**.
- WP-05, WP-06 e WP-07 tocam `crates/app` em regiões diferentes (legenda e
  footprint; paper_trading; dock/painel novo) → paralelizáveis, com rebase de
  quem chegar depois.
- WP-11, WP-12 e WP-13 tocam a família de desenhos e o registro
  (`register_drawing_tools!`) → **sequenciais entre si**, para não brigar no
  registro.
- WP-04 é isolado (scripts + testes de pine) → paralelo com qualquer coisa.
- WP-01 e WP-09 criam crates e **ambos** editam os mesmos três arquivos de
  contrato (`Cargo.toml` raiz, `CLAUDE.md`, `workspace_deps.rs`) → se rodarem
  juntos, o segundo rebaseia.

## A armadilha dos três arquivos de contrato

Criar um crate novo (WP-01, WP-09) exige **três** edições fora do crate, e duas
delas são testes que quebram — ou, pior, ficam cegos:

1. `Cargo.toml` raiz: uma linha em `members`.
2. `CLAUDE.md`: parágrafo do crate com o nome entre crases **e** a linha de
   direção de dependências. O teste `claude_md_lists_every_crate` falha sem
   isso — `cargo test --workspace` fica vermelho.
3. `crates/pine/tests/workspace_deps.rs`: entrada na whitelist hardcoded de
   `the_domain_crates_never_depend_upwards`. **O loop itera sobre a whitelist,
   não sobre o diretório** — esquecer não quebra o teste, apenas deixa o crate
   novo sem guarda de dependência. É o erro silencioso mais fácil de cometer
   neste roadmap.
