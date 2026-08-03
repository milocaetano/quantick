# Ideias de fluxo e execução entre corretoras

Status: nota de pesquisa, não é recomendação de investimento nem plano de
implementação aprovado.

Informações de mercado verificadas em: 2026-08-03.

## Resumo executivo

A principal ideia discutida foi separar:

- **mercado de sinal:** onde o Quantick lê o fluxo e a formação de preço;
- **mercado de execução:** onde uma estratégia encontra o menor custo efetivo.

O primeiro experimento seria analisar trades e book de BTC na Binance e testar
se existe tempo para executar na Aster BTC/USD1, que atualmente cobra 0% maker
e 0,005% taker. A Bitfinex também merece prioridade: atualmente anuncia taxa
zero e seu book bruto `R0` expõe IDs das ordens visíveis.

A tese é plausível, mas ainda não está provada. Um desequilíbrio no book da
Binance não pode disparar uma ordem cegamente na Aster. O Quantick precisa
confirmar que a Aster ainda não acompanhou o movimento e que existe vantagem
líquida depois de taxas, spread, slippage, impacto, latência, funding e diferença
entre USD1 e USDT.

O diferencial do Quantick não seria apenas outra visualização de heatmap. Seria
um laboratório determinístico de **lead/lag e qualidade de execução entre
corretoras**.

## Custos, bps e alavancagem

Um basis point (`bp`; plural `bps`) equivale a 0,01%:

| Bps | Percentual |
| ---: | ---: |
| 1 bp | 0,01% |
| 5 bps | 0,05% |
| 10 bps | 0,10% |
| 40 bps | 0,40% |
| 80 bps | 0,80% |

Em USD 100.000 de valor nocional, 1 bp representa USD 10.

A taxa normalmente incide sobre o **valor total da posição**, não sobre a
margem depositada. A alavancagem não precisa aumentar o percentual da taxa,
mas permite controlar um nocional maior com o mesmo capital e, por isso,
aumenta muito o custo relativo à margem.

Exemplo: USD 1.000 de margem controlando USD 10.000 com alavancagem de 10x e
taxa taker de 5 bps por lado geram aproximadamente USD 10 de taxa na entrada e
saída. São 10 bps da posição, mas 1% da margem, antes de spread, slippage e
funding.

Operar curto prazo não exige necessariamente uma taxa de acerto absurda. O que
precisa ser positivo é:

```text
expectativa líquida =
    probabilidade de ganho  * ganho médio
  - probabilidade de perda * perda média
  - custo médio completo
```

Quando o alvo é de poucos bps, porém, duas ordens taker podem consumir todo o
edge. Taxa de acerto isolada não resolve; payoff, fills e custo completo são
decisivos.

## Corretoras consideradas

As taxas abaixo são as tabelas públicas verificadas na data do documento. Elas
podem mudar conforme país, produto, nível da conta, staking, promoção e tipo de
ordem. No Quantick, precisam ser configuração versionada — nunca números
escondidos no código da estratégia.

| Corretora/produto | Maker / taker base | Ponto forte | Limitação principal |
| --- | ---: | --- | --- |
| Bitfinex spot, margem e derivativos | 0 / 0 | Taxa zero e book `R0` por ordem | Medir liquidez e spread de cada produto |
| Aster BTC/USD1 perp | 0 / 0,005% (0,5 bp) | Só 1 bp de taxa num round trip taker | Book menor e risco de basis USD1/USDT |
| Aster USDT perps | 0 / 0,04% (4 bps) | Livros mais ativos | 8 bps num round trip taker |
| Lighter Standard | 0 / 0 | IDs de contas e ordens nos trades | Speed bump: cerca de 200–300 ms |
| Paradex Retail | 0 / 0 | Separa liquidez interativa e de API | Speed bump e limites baixos de ordens |
| Extended perps | 0 / 0,025% (2,5 bps) | Alternativa de custo intermediário | Menor adoção e dados menos diferenciados |
| Hyperliquid perps | 0,015% / 0,045% | Endereços das contrapartes e fluxo on-chain | Cerca de 9 bps no round trip taker base |
| Binance, OKX e Bybit perps | Depende do nível | Liquidez e descoberta de preço | Taker mais caro que as alternativas acima |

### Bitfinex

- A tabela atual anuncia 0% maker e 0% taker em spot, margem e derivativos.
- O book WebSocket `R0` publica ID, preço e quantidade por ordem visível.
- Isso permite estudar vida da ordem, churn e persistência de fila.
- Bookmap e ATAS já conectam à Bitfinex; apenas reproduzir um heatmap não seria
  um diferencial.
- O diferencial possível seria replay determinístico, análise por ordem,
  comparação entre corretoras e custo efetivo por tamanho.
- Taxa de negociação zero não elimina funding, financiamento, risco de
  custódia, contraparte ou restrições de jurisdição.

### Aster

A Aster possui duas rotas de BTC economicamente diferentes:

- BTC/USD1: 0 maker e 0,005% taker;
- BTC/USDT: 0 maker e 0,04% taker.

Na coleta pontual feita durante a pesquisa, BTC/USD1 tinha aproximadamente
1,11 bps de spread e USD 3,46 milhões de volume nocional em 24 horas. BTC/USDT
tinha aproximadamente 0,016 bp de spread e USD 708 milhões. Isso é apenas uma
fotografia, não um benchmark, mas mostra que a rota de menor taxa possui book
muito menor.

A Aster publica L2 agregado por preço e permite ordens escondidas. Portanto, o
book visível é incompleto por definição e deve ser identificado assim no
Quantick.

### Lighter, Paradex e Hyperliquid

Esses mercados oferecem dados incomuns em plataformas tradicionais:

- a Lighter expõe IDs das duas contas e das ordens envolvidas nos trades;
- a Hyperliquid expõe endereços de comprador e vendedor;
- a Paradex publica melhores preços separados para liquidez interativa e API.

Isso permitiria estudar persistência de participantes, fluxo tóxico, quem
antecipa movimentos e diferenças entre fluxo retail e profissional.

O Quantick já possui trades públicos e L2 visível da Hyperliquid. Lighter e
Paradex acrescentariam dados realmente novos, mas as rotas gratuitas possuem
atrasos artificiais. Taxa zero não significa API irrestrita de baixa latência.

### Comparações enganosas

- Promoções da MEXC no site/app não valem necessariamente para API. A tabela de
  junho de 2026 anunciou 0,06% maker e 0,08% taker na API de futuros.
- Variational Omni oferece taxa zero, mas usa RFQ com provedor interno, não um
  livro central de ofertas. Não serve para análise tradicional de book.
- Maker zero não garante execução gratuita: posição na fila, ordens não
  executadas e seleção adversa podem custar mais que a taxa exibida.
- Volume divulgado pode ser influenciado por incentivos. Profundidade
  executável, spread estável e impacto por tamanho são medidas melhores.

## Ideia central: Binance como sinal e Aster como execução

### Hipótese

A Binance pode funcionar como fonte de descoberta de preço. Se um evento de
fluxo ocorrer nela e a Aster responder com atraso estável, uma estratégia pode
tentar executar na Aster antes de ela terminar de acompanhar o movimento.

Isso é uma hipótese de lead/lag, não uma arbitragem garantida.

### Sinais candidatos na Binance

O sinal deve combinar trades confirmados e dinâmica do book:

- order-flow imbalance (OFI);
- deslocamento do microprice em relação ao mid-price;
- volume comprador/vendedor agressivo e velocidade dos sweeps;
- esgotamento e reposição das primeiras filas;
- persistência das ordens e intensidade das retiradas;
- volatilidade e regime de spread;
- concordância entre agressões executadas e movimento posterior do book.

Um imbalance estático é frágil: paredes podem ser canceladas. Trades
executados, esgotamento persistente e falta de reposição merecem mais peso.

### Confirmação obrigatória na Aster

Antes de emitir uma oportunidade, o sistema precisa verificar:

- a Aster já acompanhou o movimento?
- qual é o preço médio realmente executável para o tamanho pretendido?
- há profundidade suficiente sem atravessar muitos níveis?
- o spread está dentro do limite?
- o feed está sincronizado e recente?
- USD1/USDT está estável?
- mark price, index price e funding estão normais?

A conta correta é:

```text
edge líquido executável =
    movimento previsto na Aster
  - taxas de entrada e saída
  - spread executável
  - impacto no book
  - slippage esperado
  - incerteza do basis USD1/USDT
  - funding
  - margem para latência e erro do modelo
```

Só existe oportunidade quando esse valor supera uma margem de segurança. O
último preço negociado nunca deve substituir o preço executável no book.

### Fluxo proposto

```text
Trades + L2 sincronizado da Binance
                  ↓
       features determinísticas de fluxo
                  ↓
      direção e movimento provável
                  ↓
     L2/trades Aster + basis USD1/USDT
                  ↓
 custo completo e profundidade executável
                  ↓
       evento de oportunidade / nada
```

## Funcionalidades diferenciadas para o Quantick

### Monitor de lead/lag entre corretoras

- atraso medido por horizonte e regime de mercado;
- probabilidade e tempo mediano para o destino acompanhar;
- movimento restante depois de a Aster começar a responder;
- intervalo de confiança, quantidade de amostras e falsos sinais;
- alerta quando a relação histórica se deteriorar.

### Roteador de custo efetivo

```text
custo efetivo = taxa + spread + impacto + funding + basis + risco de latência
```

O resultado deve ser uma curva por tamanho. Uma corretora pode ser a mais
barata para USD 500 e a pior para USD 50.000.

### Toxicidade e seleção adversa

- movimento do preço 50, 100, 250, 500 e 1.000 ms depois do evento;
- chance de um fill passivo ficar imediatamente contra a posição;
- resposta de cancelamentos e reposição em torno de sweeps;
- meia-vida da reposição depois que um nível é consumido;
- resultado maker versus taker em cada regime.

### Persistência de participantes

Onde a corretora publica identificadores honestamente:

- contas ou endereços agressores recorrentes;
- persistência de tamanho e direção;
- participantes que antecedem outras corretoras;
- score de fluxo tóxico por identificador anônimo.

O sistema não pode afirmar que um endereço corresponde a uma identidade real;
uma mesma entidade também pode operar várias contas.

### Indicador de confiança dos dados

Todo resultado precisa informar:

- feed sincronizado, atrasado, recuperando ou com gap;
- cobertura visível do book;
- book agregado por preço ou por ordem;
- presença possível de liquidez escondida;
- timestamp da corretora e timestamp local de recebimento;
- lado agressor declarado pela venue ou inferido.

Um sinal deve se desativar durante gaps, sem estender silenciosamente um estado
antigo do book.

## Roteiro de validação

### Fase 0 — gravador sincronizado

Gravar Binance e Aster simultaneamente, incluindo:

- trades e atualizações necessárias para reconstruir o L2;
- timestamp da venue e relógio monotônico local;
- IDs de sequência e geração da conexão;
- gaps e ressincronizações;
- USD1/USDT, mark/index price e funding;
- tabela exata de taxas assumida na sessão.

Nenhuma ordem real é necessária nessa fase.

### Fase 1 — replay e estudo determinístico

Executar a mesma lógica planejada para o modo ao vivo. Simular fills contra o
book histórico da Aster, nunca contra preço da Binance ou fechamento de candle.

Medir:

- expectativa bruta e líquida em bps;
- taxa de acerto e ganho/perda médios;
- excursão favorável e adversa;
- frequência de oportunidades executáveis;
- fills, sinais sem fill, slippage e impacto por tamanho;
- distribuição da latência Binance → decisão → Aster, inclusive p95/p99;
- resultados por volatilidade, spread e regime de liquidez;
- sensibilidade a custos, atraso adicional e parâmetros próximos.

Usar períodos fora da amostra e walk-forward. Rejeitar resultados que só
funcionam num dia ou num parâmetro exato.

### Fase 2 — shadow mode ao vivo

Gerar sinais e ordens simuladas, sem enviá-las. Comparar o fill previsto com o
book observado depois da decisão. Essa fase revela atrasos e estados obsoletos
que um replay pode não representar bem.

### Fase 3 — consumidor de execução com tamanho mínimo

Somente se as fases anteriores sobreviverem, um bot externo pode testar tamanho
mínimo com:

- limites de posição e perda diária;
- kill switch para feed atrasado, gap ou desconexão;
- limites de spread, slippage e idade da ordem;
- cancelamento em desconexão, quando disponível;
- reconciliação de ordens e fills com a corretora;
- bloqueio por basis USD1/USDT ou funding fora do limite.

## Encaixe na arquitetura do Quantick

- Uma integração pública da Aster deve ficar num `feed-aster` independente.
- `engine` e `orderbook` continuam sem rede, relógio ou credenciais.
- Features de lead/lag devem viver num módulo de domínio determinístico e
  reutilizável por gráfico, replay/backtest e bot.
- Um adaptador autenticado de execução deve ser consumidor separado; não pode
  criar dependência reversa para o núcleo.
- Primeiro o Quantick deve emitir um evento de oportunidade com evidências, não
  virar uma plataforma completa de envio de ordens.

## Critério para continuar ou abandonar

A ideia só avança se, fora da amostra:

1. a Binance anteceder a Aster por mais tempo que a latência ponta a ponta;
2. a expectativa continuar positiva após todos os custos e slippage estressado;
3. o resultado sobreviver a vários tamanhos e parâmetros próximos;
4. basis de USD1 e baixa liquidez não dominarem o movimento previsto;
5. os fills do shadow mode forem parecidos com os simulados;
6. degradação e gaps forem detectados e bloquearem sinais.

Se o lead desaparecer depois dos custos ou exigir latência irreal, a hipótese
deve ser descartada. Descobrir isso por gravação e replay já é um resultado útil.

## Prioridade sugerida

1. Gravador sincronizado Binance/Aster e estudo de lead/lag.
2. Captura do book bruto da Bitfinex para análise por ordem e comparação de
   execução sem taxa.
3. Evoluir a integração existente da Hyperliquid para análise de participantes.
4. Avaliar Lighter para análise de contas/ordens, modelando seus speed bumps.
5. Avaliar Paradex pelo fluxo retail/API, não como execução gratuita de baixa
   latência.

## Fontes verificadas

- [Bitfinex: Zero Fees Q&A](https://blog.bitfinex.com/products/zero-fees-qa/)
- [Bitfinex: WebSocket raw books](https://docs.bitfinex.com/reference/ws-public-raw-books)
- [Aster: taxas dos perpétuos](https://docs.asterdex.com/trading/perpetuals/fees-and-specs/fees)
- [Aster: documentação da API](https://docs.asterdex.com/product/aster-pro/api/api-documentation)
- [Aster: hidden orders](https://docs.asterdex.com/trading/perpetuals/order-types/hidden-order)
- [Lighter: taxas e latência](https://docs.lighter.xyz/trading/trading-fees)
- [Lighter: WebSocket](https://apidocs.lighter.xyz/docs/websocket-reference)
- [Lighter: limites da API](https://apidocs.lighter.xyz/docs/rate-limits)
- [Paradex: taxas](https://docs.paradex.trade/trading/trading-fees)
- [Paradex: perfis retail e pro](https://docs.paradex.trade/trading/trader-profiles)
- [Paradex: FastFill](https://docs.paradex.trade/trading/fastfill)
- [Hyperliquid: taxas](https://hyperliquid.gitbook.io/hyperliquid-docs/trading/fees)
- [Hyperliquid: WebSocket](https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/websocket/subscriptions)
- [Extended: taxas e rebates](https://docs.extended.exchange/extended-resources/trading/trading-fees-and-rebates)
- [MEXC: mudança nas taxas da API de futuros](https://www.mexc.com/es/announcements/article/updates-to-api-futures-trading-fees-jun-1-2026-17827791535742)
- [Variational: Omni](https://docs.variational.io/omni/about-omni)
- [Variational: modelo RFQ](https://docs.variational.io/variational-protocol/key-concepts/trading-via-rfq)
- [Bookmap: conectividade cripto](https://bookmap.com/knowledgebase/docs/KB-IntroductionToBookmap-Connectivity#crypto-connectivity)
- [ATAS: conexões cripto](https://help.atas.net/en/support/solutions/articles/72000602619-which-account-for-trading-and-quotes-is-better-to-choose-)
- [CoinGecko: State of Crypto Perpetuals 2026](https://www.coingecko.com/research/publications/state-of-crypto-perpetuals-report-2026)
