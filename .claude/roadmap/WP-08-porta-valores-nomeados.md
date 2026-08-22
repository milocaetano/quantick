# WP-08 — porta de valores nomeados dos indicadores

**Missão**: hoje um indicador só sabe **desenhar**. Para que um gate do
checklist possa vir de um script `.pine` de usuário — "o BEI disparou nas
últimas 6 barras?", "a persistência de CVD está em 0,22" — o indicador precisa
poder **publicar um número com nome**, e não só uma série para plotar.

Branch: `feat/indicator-values-port` · worktree
`../quantick-worktrees/feat-indicator-values-port`

Depende de: nada. Bloqueia: WP-14 (e torna o WP-07 extensível a scripts).

## O buraco, com precisão

- Tudo que atravessa o canal do worker é **posicional**: `columns[i]`,
  `row[i]`, `PreviewFrame.values[i]` — "one value per declared plot, **in
  descriptor order**" (`indicator_worker.rs:44-46`, `:239`) e "in `PlotId`
  order" (`indicators/output.rs:296-299`).
- Nome existe só no descriptor: `PlotSpec { id, title, style, base_color,
  width, offset, marker }` (`indicators/output.rs:148-164`). A UI reconstrói
  nome→número por índice (`indicator_legend.rs:154-168`).
- A trait `Indicator` (`indicators/indicator.rs:105-165`) expõe `descriptor`,
  `plots`, `on_close`, `preview`, `reset`, `objects`, `input_values`. **Nenhum
  método devolve escalar nomeado.**
- `ObjectSnapshot` carrega `LabelObj` com **texto**, não número com semântica
  (`indicators/objects.rs:283-290`).

Ler o valor "por título do plot" é a alternativa barata e **rejeitada**: o
título anda com os inputs (`descriptor.title` "moves with the inputs",
`indicator_worker.rs:146-148`), então o gate quebraria em silêncio quando o
usuário mexesse num parâmetro. Um gate que quebra em silêncio é pior que gate
nenhum.

## O desenho

Porta aditiva, no molde exato de `objects()` — que já é um método com default
`None` na trait e custa zero para quem não implementa
(`indicators/indicator.rs:145-147`):

1. **Trait**: método novo com default vazio, devolvendo pares (nome, valor)
   com semântica declarada. Nome estável e independente do título exibido.
2. **Worker**: evento novo no `IndicatorEvent` (`indicator_worker.rs:221-263`),
   ao lado de `Objects`, seguindo o mesmo coalescing e a mesma disciplina de
   batch (`:434-441`).
3. **Pine**: uma forma de o script declarar um valor nomeado. Esta é a parte
   com maior risco de projeto — o dialeto rejeita explicitamente várias
   famílias (`request.*`, `strategy.*`, `array/matrix/map/table`) e a adição
   precisa caber no idioma sem virar porta dos fundos para estado arbitrário.
   Proposta a avaliar no PR: uma função de export com nome literal constante
   (resolvido em tempo de compilação, como os call-sites de `ta.*` já são),
   com cap de N valores por script.
4. **Consumo**: o agregador de gates do WP-07 passa a poder ler valores de
   script, além das fontes de estado que já lê.

## Critérios de aceite

1. Indicador que não publica valores continua compilando e rodando sem
   mudança — o default é vazio, não `unimplemented!()`.
2. Nome de valor é **estável** e não muda quando o usuário altera inputs. Um
   teste prova isso mexendo num input e conferindo que o nome sobreviveu.
3. Cap declarado de valores por indicador, no espírito dos caps existentes
   (500 objetos por tipo, 10.000 iterações por barra) — sem cap, um script
   pode inundar o canal a cada barra.
4. **Segunda implementação fake testada**: exigência literal do
   `new-extension` §5 — uma porta com um único implementador não é porta, é
   acoplamento com nome bonito. Um indicador de teste que publica valores
   conhecidos prova o contrato.
5. `preview` respeita o contrato commit/preview com rollback, como todo o
   resto do crate: valor publicado em preview não pode contaminar o estado
   commitado.
6. Determinismo preservado: mesma sequência de barras → mesmos valores. Se o
   crate `indicators` tiver golden test tocado, ele é atualizado.
7. Nomes de teste no idioma da casa; testes inline no `#[cfg(test)] mod tests`
   do próprio arquivo.

## Risco declarado

Este é o pacote com maior chance de virar discussão de arquitetura em vez de
código. Se o `arch-review` apontar que a extensão do dialeto pine não cabe,
o fallback aceitável é **restringir a porta aos indicadores nativos** na v1
(que já são Rust e podem implementar o método diretamente), deixando scripts de
usuário para depois. O WP-07 continua funcionando de qualquer forma, porque os
gates da v0 não dependem desta porta.

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto de performance declarado: **por barra fechada**, no worker, fora
      da thread de UI. Cap provado por teste.
- [ ] `arch-review` — este pacote merece atenção extra na revisão de porta.
- [ ] `new-extension`: porta nomeada + segunda implementação fake testada +
      defaults preservam o hoje.
- [ ] Golden/determinismo: se o contrato de execução do indicador foi tocado,
      teste que prove que duas execuções idênticas produzem saída idêntica.
- [ ] PR aberto com CI verde. Merge não faz parte.
