# GOAL — Barra de contexto discreta + setas de marca + lápis

**Objetivo (uma frase):** selecionar um desenho no gráfico passa a abrir uma
barra de contexto flutuante, discreta, só de ícones numa linha (no molde do
TradingView), com o inspector completo atrás da engrenagem; e o rail ganha
duas ferramentas novas — marcas de seta de um clique (compra/venda) e um
lápis à mão livre.

**Branch:** `feat/drawing-context-bar`
**Worktree:** `../quantick-worktrees/feat-drawing-context-bar`
**Aberto em:** 2026-08-07

## Contexto do usuário

Dois prints do TradingView (DE40 5m). Palavras dele: *"quando clica em algo,
aparece um popup bem discreto para não atrapalhar, com ícones para apagar,
trocar de cor, tamanho… etc. Engrenagem"*, *"adicione setas e um lápis pra eu
poder desenhar"*, *"algo bem simples e funcional"*. Liberdade criativa dada,
com painel de UX + designer + trader consultado antes de implementar.

Hoje: selecionar abre o inspector de ~360 px com abas, checkboxes e botões
textuais (`app.rs: draw_inspector_title_bar / drawing_inspector_body`).
A seta atual (`drawings/arrow.rs`) é de dois pontos. Não existe lápis.

## Critérios de aceite

### Específicos do objetivo
1. Selecionar um desenho abre **só** a barra de contexto — uma linha de
   ícones, sem texto de ação, posicionada perto do objeto sem cobri-lo.
2. A engrenagem (e só ela) abre o inspector de hoje, inalterado no conteúdo.
3. A barra é **dirigida por capacidade**: nenhuma ferramenta mostra um botão
   que ela não suporta; nenhum botão morto/desabilitado sem motivo.
4. Delete resolve num clique com caminho de volta garantido (undo/toast) —
   a regra final vem do painel; nada de glifo destrutivo sem proteção.
5. Ferramenta nova: **marca de seta** para cima e para baixo, um clique,
   uma âncora.
6. Ferramenta nova: **lápis** à mão livre, com o traço ancorado em
   tempo/preço (sobrevive a pan e zoom como qualquer outro desenho).
7. Persistência: os desenhos novos salvam/carregam como os existentes.

### Gates padrão — mudança de código
8. Quatro checks verdes (`fmt`, `clippy -D warnings`, `build`, `test`) após
   rebase na `main` atual.
9. Impacto de performance declarado por caminho tocado (per-frame para o
   paint da barra e do traço do lápis; o resto é raro).
10. `arch-review` rodado sobre `git diff main...HEAD`, todo Blocker e
    Should-fix resolvido ou deferido no corpo do PR.
11. **PR aberto** com CI verde. Merge não faz parte do goal.

### Gates padrão — superfície visível
12. `ui-harness`: toda superfície nova alcançável por env hook, registrada
    na mesma mudança (barra de contexto, cada popover, cada ferramenta nova).
13. `visual-qa`: matriz de estados capturada, todas PASS ou defeito aceito
    explicitamente.
14. `trader-ux-review`: sem Blocker em aberto.

### Gate de capacidade (as duas ferramentas novas)
15. `new-extension`: entram pelo port `DrawingToolImpl` — registro aditivo,
    sem cirurgia no código existente; blast radius (arquivos adicionados vs.
    editados) declarado no PR.

## Fora de escopo (deferido, anotado)

- Edição de texto in-place no canvas (o 2º print do usuário). Entra como
  issue própria se a barra não resolver.
- Reescrever o inspector: ele fica como está, só muda quem o abre.
