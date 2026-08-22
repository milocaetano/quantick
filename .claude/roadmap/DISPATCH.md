# Protocolo de despacho — como um pacote de trabalho vira um agente

Este arquivo é lido **pelo agente executor**, não pelo humano. Cada pacote em
`.claude/roadmap/WP-*.md` é uma missão fechada; este documento diz como
executá-la sem quebrar as regras da casa.

## Regra zero: um pacote, um worktree, um agente

Agentes paralelos **nunca** compartilham working tree. O hook `worktree-guard`
nega qualquer escrita no checkout principal enquanto ele estiver em `main`, e
essa negação é a rede — não a memória.

```sh
git fetch origin
git worktree add -b <prefixo>/<slug> ../quantick-worktrees/<prefixo>-<slug> origin/main
```

O prefixo vem do pacote (`feat/`, `fix/`, `docs/`). Todo o trabalho acontece
dentro desse diretório. Limpeza depois do merge, a partir do checkout principal:

```sh
git worktree remove ../quantick-worktrees/<prefixo>-<slug>
git branch -d <prefixo>/<slug>
```

## Os quatro checks (obrigatórios, sem exceção)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
```

CI roda os mesmos quatro. PR com CI vermelho não é mergeado — depois de abrir,
acompanhe com `gh pr checks <n> --watch` e conserte antes de pedir revisão.

## Portões padrão por tipo de pacote

Todo pacote injeta os portões que o tipo de trabalho exige. Marcados no corpo
do PR com evidência, não com promessa.

| Portão | Quando se aplica | O que satisfaz |
| --- | --- | --- |
| Quatro checks | sempre | saída verde dos quatro comandos |
| Impacto de performance declarado | sempre | uma frase no PR dizendo se o código é per-frame, per-trade, per-depth ou offline, e o que isso custa |
| `arch-review` | sempre, **antes** do PR | skill rodada sobre `git diff main...HEAD`; todo Blocker e Should-fix resolvido ou deferido por escrito no corpo do PR |
| `new-extension` | capacidade nova | docar por porta nomeada; edição de registro em vez de cirurgia; defaults preservam o comportamento de hoje; raio de impacto declarado |
| `ui-harness` | superfície de UI nova ou alterada | toda superfície alcançável por env hook, com o hook **adicionado nesta mudança** |
| `visual-qa` | UI | matriz de estados capturada, PASS ou defeito aceito por escrito |
| `trader-ux-review` | UI que o trader toca em sessão | sem Blocker em aberto |
| Golden test | qualquer código no `engine` ou em crate puro novo | fixture CSV + `assert_golden` (roda duas vezes e exige runs idênticos) |

### Registrando a arch-review

O hook `pr-gate` nega `gh pr create` até existir revisão registrada para o
**sha exato** de HEAD:

```sh
git rev-parse HEAD > "$(git rev-parse --absolute-git-dir)/arch-review-ok"
```

Commitou de novo depois da revisão? O sha muda e o portão nega outra vez — é
assim que ele é honesto. Rode a revisão de novo e regrave.

## O que o agente executor NÃO faz

- **Não faz merge.** `gh pr merge` é de outra pessoa. Entrega o PR verde e para.
- **Não abre instâncias do app** para validação visual sem autorização explícita
  do Camilo; sem ela, o portão `visual-qa` é reportado como BLOCKED, não como
  PASS presumido.
- **Não relaxa parâmetro do operacional por conta própria.** Os números vêm da
  §04 do documento do operacional; um pacote que precise mudar um deles reporta
  a divergência em vez de decidir sozinho.
- **Não inventa dado.** Regra da casa: dado inferido ou incompleto é rotulado,
  nunca silenciosamente remendado. Vale para todo pacote deste roadmap.

## Formato de entrega

O PR traz, no corpo: o que mudou e por quê; a declaração de performance; a
lista de portões com evidência; findings de arch-review deferidos, se houver; e
`Closes #<issue>` quando o pacote tiver issue. O commit segue conventional
commits em inglês, imperativo (`feat: ...`, `fix: ...`).

## Ordem importa

Pacotes têm dependências declaradas no próprio arquivo. Um pacote bloqueado por
outro **espera** — despachar os dois em paralelo produz dois worktrees que
divergem no mesmo arquivo. A ordem canônica está em `.claude/roadmap/README.md`.
