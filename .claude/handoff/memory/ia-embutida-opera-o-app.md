---
name: ia-embutida-opera-o-app
description: "O quantick vai ganhar um assistente de IA que opera o app como um humano — é premissa de arquitetura, não feature futura solta"
metadata: 
  node_type: memory
  type: project
  originSessionId: dd79b77f-2879-4b9d-a92a-a669ad86ff9c
  modified: 2026-08-17T20:16:09.273Z
---

Decidido em 17/08/2026: o quantick vai ganhar um assistente de IA embutido que
faz pelo trader o que o mouse faz — criar estratégias, analisar o gráfico,
adicionar e ler ferramentas, colocar um trade, travar a plataforma. Nada disso
está sendo construído ainda; o que existe hoje é a porta aberta para ele.

**Why:** capacidade que só existe como gesto (`if response.clicked()` que muta
estado inline) é capacidade que o assistente nunca vai ter, e retrofit custa
reabrir o arquivo. Por isso virou dimensão 7 do `arch-review` ("The second
operator") e um item do `new-extension` e do `mission`, em vez de lembrete.

**How to apply:** toda capacidade que o trader *usa* precisa de três coisas —
**act** (função nomeada que recebe dados; o clique só chama), **read**
(resultado legível como dado, não só pintado) e **discover** (id no mesmo
registry que alimenta a UI, à la `DRAWING_TOOLS`). Conteúdo que o trader varia
(estratégia, indicador, alerta, preset) vai em script/config carregado em
runtime, não em `enum` recompilado — o precedente é o crate `pine`. A camada
de comando roda em taxa humana, nunca por trade ou por frame: performance
continua acima de operabilidade na ordem de prioridade. Ação de mercado ou de
segurança atravessa o mesmo armar/confirmar do humano, e fica registrado quem
agiu. O `copilot.pine` do repo é um indicador Pine, não este assistente — não
reutilizar o nome. Ver [[operacional-mark-i]] e [[roadmap-ferramentas]].
