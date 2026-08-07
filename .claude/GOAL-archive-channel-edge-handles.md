# Goal — channel resize handles on both edges

Um canal paralelo selecionado ganha alças de redimensionamento centradas em
cada trilho (superior **e** inferior), no lugar da única bolinha de canto que
hoje é a âncora de largura crua.

Branch: `feat/channel-edge-handles` — worktree
`../quantick-worktrees/feat-channel-edge-handles`.

## Acceptance criteria

1. Canal selecionado mostra 4 alças: as duas pontas da linha de tendência mais
   uma alça no **centro** de cada trilho.
2. Arrastar a alça central do trilho oposto muda a largura movendo só aquele
   trilho — a linha-base fica onde está.
3. Arrastar a alça central do trilho da base move só a base — o trilho oposto
   fica travado, então o canal cresce para o outro lado.
4. A bolinha da âncora crua `points[2]` não aparece mais; a alça acompanha o
   centro do trecho ancorado mesmo com extend ligado.
5. As alças entram por uma porta do trait `DrawingToolImpl` com implementação
   padrão = âncoras cruas → nenhuma outra ferramenta muda de comportamento.
6. Testes headless: largura pelo trilho oposto preserva a base; largura pela
   base preserva o trilho oposto; alça sempre no ponto médio do trecho
   ancorado.

## Injected gates

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo build --workspace`
- [ ] `cargo test --workspace`
- [ ] impacto de performance declarado por caminho (per-frame: paint/hit-test
      do canal; nada per-trade nem per-depth)
- [ ] `new-extension`: porta nomeada, edições de registro apenas, defaults
      preservam o comportamento de hoje, blast radius no corpo do PR
- [ ] `ui-harness`: hook de env que abre a superfície (canal selecionado)
- [ ] `visual-qa` PASS ou defeitos aceitos explicitamente
- [ ] `trader-ux-review` sem Blocker em aberto
- [ ] `arch-review` com Blocker/Should-fix resolvidos ou deferidos no PR
- [ ] PR aberto (merge não faz parte do goal)
