# WP-02 — classificador de regime e etiquetagem da biblioteca

**Missão**: decidir, sem opinião humana, se um dia gravado é **range**,
**tendência rotacional** ou **drive**. Esta classificação é a pré-condição de
todo setup do operacional e, mais importante, é o que permite montar as listas
de treino **às cegas quanto ao resultado** — a única defesa real contra
escolher os dias que renderam.

Branch: `feat/regime-classifier` · worktree
`../quantick-worktrees/feat-regime-classifier`

Depende de: WP-01. Bloqueia: WP-03.

## Por que este pacote vem antes das regras de trade

O `Setup A` exige "dia classificado range" como pré-condição. Se o
classificador for escrito **junto** com as regras, a tentação é ajustá-lo até
os trades ficarem bons — que é curve-fitting com aparência de método. Escrito
antes e congelado, ele vira a régua independente.

E ele resolve o problema logístico do roadmap: sem etiquetagem automática, a
"biblioteca de sessões de tendência rotacional" que o Módulo T precisa é
montada à mão, dia a dia, no olho.

## O que classificar (números da §04 do operacional)

Sobre barras de **5 minutos** derivadas da mesma fita (o harness constrói com
`TimeBarBuilder::new(300_000)`), mais o Opening Balance 09:00–09:30 no fuso
declarado no header da sessão (`# timezone=`, exposto por `Session::timezone()`):

**Estágio 1 — range × tendência** (avaliado às 10:00–10:15 de mercado):
1. duas barras de 5 min consecutivas fechando fora do OB com extensão ≥ 50% da
   largura do OB → tendência;
2. persistência de CVD `|ΔCVD| / volume da janela` ≥ 0,30 → tendência;
   ≤ 0,15 → range;
3. cruzamentos do AVWAP ancorado às 09:00: ≥ 3 e fechamento próximo → range.

**Estágio 2 — rotacional × drive** (10:30–11:00), 3 de 4:
1. razão pullback/perna da última perna ≥ 0,33 (drive: < 0,20);
2. ≥ 1 toque/reentrada na faixa ±1σ do AVWAP por hora (drive: zero);
3. sobreposição média das últimas 12 barras de 5 min ≥ 0,40 (drive: < 0,25);
4. ≥ 4 das últimas 12 barras violaram a mínima da anterior (≤ 2 = drive).

## Critérios de aceite

1. **Sub-comando do harness** — `classify` ao lado do `run`, mesma CLI à mão,
   mesmo padrão de `ExitCode`, mesmo diagnóstico JSON com `event_code`.
2. **Saída em duas formas**: (a) uma linha por sessão, legível, com o veredito
   **e os quatro números que o produziram** — um classificador que só diz
   "range" é inauditável; (b) um arquivo de etiquetas determinístico que o
   WP-03 consome para montar as listas de treino.
3. **Reuso, não reimplementação.** AVWAP tem kernel nativo pronto e
   golden-testado (`native::AnchoredVwap::new(anchor_ms, source, bands)`, com
   `AVWAP_BAND_MULTS = [1.0, 2.0, 3.0]`); CVD tem `native::Cvd::new()` e o host
   mantém a série (`host.cvd()`). Nada de recalcular à mão — a regra
   "one engine, three consumers" vale também para os indicadores.
4. **Fuso e sessão vêm do arquivo, nunca do relógio.** O header declara o
   offset (`# timezone=-03:00`); sessão sem declaração é lida como UTC, e o
   classificador **precisa** recusar-se a classificar nesse caso em vez de
   assumir Brasília. O engine não tem noção de sessão/pregão — o recorte
   horário é do harness.
5. **Indeciso é um veredito válido.** Dia que não atinge 3 de 4, ou cuja fita
   não cobre o OB inteiro, sai como `indeterminado` com o motivo. Forçar todo
   dia num dos três rótulos produziria uma base de treino contaminada.
6. **Honestidade sobre cobertura**: sessão cuja fita começa depois das 09:00
   (backfill parcial) não tem OB válido. Reportar "OB parcial: fita desde
   HH:MM" e classificar como indeterminado, no espírito do rótulo que o app já
   usa para dado incompleto.
7. **Determinismo**: mesma sessão → mesmo veredito, provado por teste. Sem
   relógio, sem `HashMap` na agregação.
8. **Parâmetros congelados após calibração.** Os cortes acima são valores de
   partida da §04; o pacote os expõe como constantes nomeadas num só lugar,
   não espalhadas pelo código. Mudar um deles é editar a §04 e re-rodar tudo.

## Fora de escopo

- Qualquer decisão de trade (WP-03).
- Calibrar os cortes: o pacote entrega o instrumento e os valores de partida.
  A calibração é trabalho de rodar sobre a biblioteca, e ela **precisa** ser
  walk-forward (ajustar em ~12 sessões, congelar, validar em 6+ nunca vistas).

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto de performance declarado: **offline**.
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] Teste de determinismo por sessão.
- [ ] Teste que prova que sessão sem OB completo sai `indeterminado`, não
      classificada por chute.
- [ ] PR aberto com CI verde. Merge não faz parte.
