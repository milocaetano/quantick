---
name: captura-offscreen-sem-roubar-foco
description: mover a janela para fora da tela com SWP_NOACTIVATE dá screenshot real sem disputar o desktop
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 26a7a39d-026b-40a9-bd71-ea5b07cfce9d
  modified: 2026-08-15T18:48:29.876Z
---

Quando o desktop está ocupado ([[desktop-ocupado-bloqueia-captura]]), a saída
não é desistir da captura nem brigar pelo foco: é **tirar a janela do caminho**.

Lançar normalmente e, assim que houver `MainWindowHandle`, chamar
`SetWindowPos(h, 0, -2200, 40, 0, 0, SWP_NOSIZE|SWP_NOZORDER|SWP_NOACTIVATE)`.
O DWM continua compondo uma janela fora da área visível, então `PrintWindow`
com `PW_RENDERFULLCONTENT` entrega frame real — `fps=59`, centenas de cores
distintas — e nada aparece na frente de quem está usando o computador.

**Why:** foi assim que as 6 telas do visual-qa saíram, depois de duas tentativas
em branco. Sem isso o portão visual fica em aberto por uma razão que não é
técnica.

**How to apply:** script pronto em
`scratchpad/tape-qa/offscreen.ps1` (padrão reaproveitável: launch → mover →
esperar health → `capture_pid.ps1` → matar). Sempre confirmar `fps ≥ 50` e a
contagem de cores antes de acreditar na imagem.

Artefato conhecido: sobra um retângulo não composto no canto inferior esquerdo
(a barra de ferramentas). É da técnica, não do app — não reportar como defeito.

E o mais importante: **as capturas acharam dois bugs que os testes não pegaram**
(entrada de menu desmarcada por copy-on-write agressivo; menu que não abria por
press+release no mesmo frame). Screenshot não é formalidade — ver a tela é uma
classe de verificação própria.
