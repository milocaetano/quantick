---
name: roadmap-ferramentas
description: O plano de construção das ferramentas do operacional vive em .claude/roadmap/ como 14 pacotes despacháveis; a Onda 0 (medir edge) gateia todo o resto
metadata: 
  node_type: memory
  type: project
  originSessionId: 8179880a-326f-42a2-b2ce-cc3d67090832
  modified: 2026-08-14T05:30:57.722Z
---

Em 2026-08-14 criamos o roadmap de construção das ferramentas do [[operacional-mark-i]]. Arquivos em `C:\src\quantick\.claude\roadmap\`: `README.md` (índice + ondas + portão de decisão), `DISPATCH.md` (regras de execução para agentes) e `WP-01..WP-14.md` (pacotes no formato GOAL da casa, com critérios de aceite citando arquivo e linha). Versão visual: https://claude.ai/code/artifact/1ab709d8-0c06-4a4a-8be0-5da1f0f14dca

**Princípio de ordenação: valor de informação primeiro.** Não existe evidência de edge ainda — por isso a Onda 0 é o instrumento de medição (WP-01 harness de backtest → WP-02 classificador de regime → WP-03 Setup A mecanizado), e um **portão de decisão** gateia as Ondas 1–5. Expectância < −0,10R consistente = o roadmap para e volta ao desenho do operacional.

Para despachar: `Execute .claude/roadmap/WP-XX-*.md seguindo .claude/roadmap/DISPATCH.md` — um pacote, um worktree, um agente.

Achados de reconhecimento que valem além do roadmap:
- **`crates/indicators/tests/bot_readiness.rs` já é o harness de backtest em miniatura** — o doc do arquivo diz "this test *is* the future backtest/bot access path".
- **`strategy.*` é rejeitado pelo dialeto Pine** → regras de trading têm que ser Rust, nunca `.pine`.
- **Criar crate novo exige 3 edições fora dele**: `members` no Cargo.toml raiz, parágrafo no CLAUDE.md (teste `claude_md_lists_every_crate` falha sem), e whitelist em `crates/pine/tests/workspace_deps.rs` — esta última é erro *silencioso*, o loop itera sobre a whitelist, não sobre o diretório.
- **Não existe clap no workspace** (nem no Cargo.lock) — CLI se parseia à mão, padrão de `crates/replay/examples/import_mt5_ndjson.rs`, com `ExitCode` e diagnóstico JSON `event_code` no stderr.
- **A convenção de dado inferido já existe e é `· side inferred`**, não `~` — corrigido no operacional.
- **O stop planejado não sobrevive ao trade no `sim`**: sobrescrito por `SetBracket` (botão Breakeven) e nunca copiado para `ClosedTrade`. É o que falta para R-múltiplo.
