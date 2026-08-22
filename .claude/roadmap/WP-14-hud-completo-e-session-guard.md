# WP-14 — HUD completo e session guard

**Missão**: o HUD do WP-07 passa a ler gates de scripts de usuário (pela porta
do WP-08), e o app ganha o único mecanismo que **impede** o erro em vez de
apenas informá-lo: bloquear BUY/SELL fora de janela operável ou com o cap de
trades batido.

Branch: `feat/session-guard` · worktree `../quantick-worktrees/feat-session-guard`

Depende de: WP-07, WP-08. É o último pacote do roadmap por consumir todos os
outros.

## Por que isto é diferente de tudo que veio antes

Todo o resto do roadmap **mostra**. Este pacote **impede** — ele muda o
contrato de execução do app. Um botão que se recusa a executar é, do ponto de
vista do trader no meio de uma sessão ruim, a diferença entre o operacional e a
vontade. Também é a mudança com maior potencial de irritar em produção: um
guard que bloqueia errado, uma vez, destrói a confiança na ferramenta.

Por isso, três princípios que o PR precisa honrar:

1. **O guard nunca impede a SAÍDA.** Fechar posição, cancelar ordem, flatten e
   arrastar o stop a favor funcionam sempre, em qualquer estado. A heurística
   do `trader-ux-review` é explícita: confirmação só para ato destrutivo
   irreversível, **nunca no caminho de saída de um trade perdedor**.
2. **O bloqueio se explica no ponto.** Botão desabilitado sem motivo legível é
   defeito de honestidade de estado no checklist do `visual-qa`. O motivo é o
   gate que falhou, com o número.
3. **Desligável, e o estado é visível.** O trader precisa poder operar sem o
   guard — e precisa ver que está sem ele. Guard silenciosamente desligado é
   pior que guard nenhum.

## Critérios de aceite

1. Gates de script chegam ao HUD pela porta do WP-08 (valores nomeados), com
   o mapeamento gate→valor declarado em configuração, não hardcoded.
2. Bloqueio de entrada nova quando: fora de janela operável, cap de trades do
   dia batido, ou circuit breaker pessoal disparado (2 stops seguidos). Os
   três vêm da §04 do operacional.
3. Saída, cancelamento e flatten **nunca** bloqueados — teste dedicado que
   prova isso para cada caminho, incluindo os atalhos (`Shift+F` flatten,
   `Shift+X` cancelar tudo).
4. Motivo do bloqueio visível no ponto de ação, com o número que o produziu.
5. Toggle do guard com estado visível na tela e persistido no sidecar
   (`paper_state.rs`, campo `#[serde(default)]` é aditivo e compatível).
6. Hook de harness no mesmo commit, permitindo ligar o guard e forçar cada
   estado de bloqueio sem esperar o relógio — sem isso, `visual-qa` não
   consegue cobrir a matriz.
7. Testes headless provando: bloqueia o que deve, libera o que deve, e a
   mensagem chega aos pixels.

## Risco declarado

Se o WP-03 tiver medido edge fraco ou indeciso, **este pacote deve esperar**.
Um guard que força disciplina em torno de um setup não validado apenas
mecaniza a perda com mais rigor. A ordem certa é: medir → validar → só então
automatizar a disciplina.

## Portões

- [ ] Quatro checks verdes.
- [ ] Impacto declarado: **per-frame** (gates) + verificação no caminho de
      comando (nunca per-trade).
- [ ] `arch-review` sobre `git diff main...HEAD`.
- [ ] `new-extension`: defaults preservam o comportamento de hoje — com o
      guard desligado, o app se comporta exatamente como antes.
- [ ] `ui-harness`: hook no mesmo commit + linha na tabela.
- [ ] `visual-qa` cobrindo cada estado de bloqueio e o estado desligado.
- [ ] `trader-ux-review` **sem Blocker** — este pacote toca o caminho crítico
      de execução; um Blocker aqui custa dinheiro de verdade.
- [ ] PR aberto com CI verde. Merge não faz parte.
