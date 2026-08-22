# WP-03 — Setup A mecanizado e a primeira medição de edge

**Missão**: implementar o Setup A (fade de extremo absorvido) como estratégia
do harness e **medir**. Este é o pacote que produz o número que decide o resto
do roadmap.

Branch: `feat/setup-a-strategy` · worktree
`../quantick-worktrees/feat-setup-a-strategy`

Depende de: WP-01, WP-02. Bloqueia: o portão de decisão do roadmap inteiro.

## As regras, como o operacional as define

Pré-condições: dia classificado range (WP-02) · janela 10:15–12:30 · largura do
range ≥ 4× stop planejado · extremo **já rejeitado ao menos uma vez** · preço a
≤ 0,5×ATR(14) do extremo · veto C2 inativo no nível · menos de 3 trades no dia.

Armação: divergência de CVD contra o toque anterior **e/ou** absorção por
esforço-sem-resultado (volume-por-ponto ≥ 1,5× a média de 100 barras, avanço
além do nível ≤ 2 ticks, close voltando ≥ 50% para dentro).

Gatilho (lado-agnóstico): primeira barra de imbalance **fechando de volta para
dentro do range**. Entrada a mercado no fechamento — que o simulador executa no
próximo print, exatamente como o dedo humano sofreria.

Stop: extremo do teste + 0,5×ATR(14), teto 140 pts. Alvo: meio do range, único,
≥ 2R. Aborto: barra seguinte devolve > 50% do corpo da entrada · BEI contrária
fecha além do fechamento da entrada · 3 barras contra sem o preço se afastar 1R.

## O que é honestamente mecanizável — e o que não é

Esta seção é obrigatória no PR, e o agente deve **preenchê-la com o que
descobriu**, não presumir. Alguns itens já se sabe que exigem decisão explícita:

- **"extremo já rejeitado ao menos uma vez"** — precisa de detecção de pivô
  sobre as barras de imbalance ou de 5 min. Qual, com que parâmetros, e por quê.
- **"o extremo do range"** — no manual o trader desenha; aqui precisa de
  definição operacional (high/low do dia? do OB estendido? do zigzag?). A
  escolha muda o resultado e tem de estar declarada.
- **"veto C2"** — depende da BEI, que é sinal por barra e portanto
  mecanizável; mas o "pelo resto da perna" exige definir onde a perna termina.
- **Janela horária** — vem do fuso declarado no header, como no WP-02.

Onde a mecanização for mais frouxa que o manual, **declarar**: o backtest mede
uma versão *aproximada* do setup, e a diferença entre ela e o que o trader faria
é uma fonte de erro que o relatório precisa nomear. O contrário — apertar a
regra até o backtest ficar bonito — é o modo de falha clássico.

## Critérios de aceite

1. Estratégia implementando a porta do WP-01, consumindo classificação do
   WP-02, com **todos** os parâmetros vindos de uma estrutura de configuração
   nomeada — zero número mágico no meio da lógica.
2. **Varredura walk-forward**, não otimização in-sample: calibrar num bloco de
   ~12 sessões, **congelar**, validar em 6+ sessões nunca vistas, e reportar os
   dois resultados lado a lado. Um relatório que mostra só o in-sample é
   propaganda, não medição.
3. **Limite de superfície de parâmetros: no máximo 5 livres.** Mais que isso,
   com uma biblioteca de dezenas de sessões, é curve-fitting com verniz de
   rigor — foi um achado explícito da crítica adversarial ao operacional.
4. **Teste de estresse de custo obrigatório**: rodar com C = 12,5 pts (vigente)
   **e** C = 22,5 pts (um tick extra de slippage por perna). Se a expectância
   não sobrevive ao segundo, o resultado do primeiro não conta.
5. **Relatório de honestidade estatística junto do número**: com n trades, o
   intervalo de confiança da taxa de acerto é ±(algo em torno de) 13 pontos
   percentuais para n = 60. O relatório imprime n e a margem, para que ninguém
   leia "expectância +0,2R" como fato estabelecido.
6. **Contagem de eventos anômalos**: rejeições do simulador, brackets
   descartados, trades abortados por cada regra. Um setup cujo edge vem de
   1.000 trades dos quais 300 foram rejeitados não é o setup que se pensa estar
   medindo.
7. Determinismo: mesma biblioteca → mesmo relatório, por teste.

## A entrega real deste pacote é uma decisão, não código

Ao terminar, o PR traz o veredito na primeira linha do corpo, contra o portão
do `README.md` do roadmap:

- **≥ +0,15R com os gates** → a hipótese sobreviveu; Onda 1 liberada.
- **entre −0,10R e +0,15R** → indeciso (o resultado mais provável com poucas
  sessões). Ampliar a biblioteca e re-medir **antes** de construir UI.
- **< −0,10R consistente** → a hipótese morreu como está. O roadmap para e a
  conversa volta ao desenho do operacional. Isso é o instrumento funcionando,
  não o projeto falhando.

Nenhum desses três resultados é fracasso do pacote. Fracasso seria entregar um
número sem dizer quantas sessões o produziram e qual a margem de erro.

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto de performance declarado: **offline**, com o tempo de uma
      varredura completa reportado.
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] Walk-forward com in-sample e out-of-sample reportados separadamente.
- [ ] Estresse de custo C = 22,5 rodado.
- [ ] Seção "o que não foi honestamente mecanizado" preenchida no PR.
- [ ] PR aberto com CI verde. Merge não faz parte.
