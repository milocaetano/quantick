---
name: cargo-test-concorrente-quebra-mt5
description: "Dois `cargo test` ao mesmo tempo derrubam os testes de porta do MT5 — não é flakiness"
metadata: 
  node_type: memory
  type: reference
  originSessionId: c043459e-c654-4c58-a35b-d4e6027bf9d3
  modified: 2026-08-15T23:40:13.354Z
---

Rodar duas invocações de `cargo test --workspace` simultaneamente faz falhar
`feed::metatrader::tests::a_mapped_symbol_listens_on_its_own_port_not_the_shared_one`
e `two_symbols_listen_on_two_ports_at_once`. Sequencialmente passam sempre.

**Why:** esses dois testes fazem bind em portas TCP reais. Dois processos de
teste disputam a mesma porta e o segundo perde o bind — o mesmo mecanismo do
[[mt5-port-conflict-diagnosis]], só que entre testes.

**How to apply:** nunca encadear `cargo test ... ; cargo test ...` num único
comando de shell para "conferir duas vezes", nem rodar um em background com
outro em foreground. Se a suíte falhar exatamente nesses dois nomes, reproduzir
com **uma** execução antes de investigar a mudança — quase certamente o defeito
é do comando, não do código.
