---
name: continuacao-de-string-vira-espacos-apos-fmt
description: "Mensagem Rust quebrada com `\\` no fim da linha vira uma linha só com a indentação como espaços literais depois do cargo fmt — escreva em linha única"
metadata: 
  node_type: memory
  type: project
  originSessionId: 43755c6c-e019-4df1-b62f-df35050f7355
  modified: 2026-08-21T20:23:01.776Z
---

Ao escrever mensagens longas (`tracing::warn!`, `assert_eq!`, `panic!`) neste
repo, **não quebre a string com `\` no fim da linha**:

```rust
"the script's input list changed since these \
 settings were saved"
```

Depois de `cargo fmt --all`, isso vira uma linha só com a indentação da
segunda linha **preservada como espaços literais** dentro do valor:
`"...since these                    settings were saved"`. A mensagem chega
assim ao log JSON e à falha de teste que alguém vai ler.

**Why:** a regra da linguagem diz o contrário (`\` + newline come o newline e o
whitespace inicial — testei isolado com `rustc` e sai correto), então a
refutação "isso é falso-positivo, Rust remove os espaços" é tentadora e está
errada para o arquivo **depois do fmt**. Aconteceu duas vezes no mesmo PR:
corrigi seis mensagens, e uma nova recém-escrita voltou com o artefato.

**How to apply:** escreva a mensagem em **uma linha só**, por mais longa que
fique (o rustfmt não quebra strings), ou use `concat!("...", "...")`. E
confira sempre **depois** de rodar o fmt, não antes:

```sh
git diff | grep -E "^\+" | grep -E '[a-z,;—.] {3,}[a-zA-Z]'
```

Zero linhas = limpo. Vale para o diff commitado e para a árvore de trabalho.
