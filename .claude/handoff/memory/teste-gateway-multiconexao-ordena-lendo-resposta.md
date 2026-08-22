---
name: teste-gateway-multiconexao-ordena-lendo-resposta
description: "Teste do gateway com várias conexões só ordena efeitos entre conexões lendo uma resposta por conexão; `send` retorna antes do reader processar e o CI Linux expõe a corrida que o Windows esconde"
metadata: 
  node_type: memory
  type: project
  originSessionId: cd0a7260-1eb8-4d03-a71d-16178fa2b47c
  modified: 2026-08-22T13:51:14.890Z
---

Num teste de integração do gateway de controle (`crates/app/src/app.rs`, `gateway_*`), `client.send(...)` retorna assim que o frame saiu pelo socket — o reader da conexão processa depois, de forma assíncrona. Dois `send` em conexões diferentes não têm ordem garantida entre si; só dentro da mesma conexão a ordem é a do envio.

**Why:** o #223 falhou só no CI (Linux): o cliente "late" mandou seu `events.wait` antes de os 12 waits das outras conexões estarem estacionados, foi estacionado em vez de recusado e o `read()` expirou com `control.instance_gone`. No Windows local passava 3/3.

**How to apply:** para afirmar "X aconteceu antes de Y noutra conexão", leia uma resposta da primeira conexão antes de mandar Y — por exemplo, o overflow do cap por conexão (`CONTROL_MAX_PARKED_WAITERS_PER_CONNECTION`) é recusado na hora e prova que os anteriores já foram processados. Relacionado: [[cargo-test-concorrente-quebra-mt5]], [[flake-hidden-layer-paints-nothing]].
