---
name: preset-de-indicador-casa-por-posicao
description: "Preset de indicador casa valor↔input por POSIÇÃO (só confere o tipo): input novo no meio da lista desloca presets antigos e pode LIGAR um switch sozinho"
metadata: 
  node_type: memory
  type: project
  originSessionId: 43755c6c-e019-4df1-b62f-df35050f7355
  modified: 2026-08-21T17:42:41.199Z
---

`App::load_indicator_preset` (`crates/app/src/app.rs`, ~2874) casa os valores
salvos com os inputs declarados **por índice**, e a única objeção é o
discriminante de tipo. Não há guard de contagem nesse caminho — o guard que
existe é outro, em `IndicatorSource::build_with` (`indicator_worker.rs`), que
recusa quando `values.len() != compiled.inputs.len()`.

Consequência: **inserir um `input.*` no meio de um script desloca todo preset
salvo antes dele**, em silêncio, porque `bool` casa com `bool`. Num caso real
do `exhaustion_reversal.pine`, um `input.bool` de diagnóstico inserido no
índice 4 herdaria o valor salvo de "2 Run: on" (`true` por default) e passaria
a **repintar as velas do trader** a partir de um preset escrito antes de o
switch existir.

**Why:** o sintoma não parece um bug de bind — parece o indicador "ligando
sozinho" ou um parâmetro "que mudou de valor". Ninguém suspeita do preset.

**How to apply:** input novo vai sempre no **fim** da lista (se destoar da
numeração das seções, abra uma seção nova no fim, como a "5 Calibration" do
`exhaustion_reversal.pine`). Pine os títulos dos inputs originais na ordem
num teste — `the_inputs_this_script_shipped_with_keep_their_positions` em
`crates/pine/tests/exhaustion_reversal_semantics.rs` é o modelo. Vale também
para renomear: `InputSpec::name()` é um slug do título e está documentado como
persistence key.

Pendência conhecida: o mesmo loader usa `.filter_map(SavedInput::to_value)`,
que **descarta** célula ilegível em vez de segurar a posição — uma célula
corrompida desloca todas as seguintes. Ver [[pine-input-color-nao-chega-ao-plot]]
para a outra armadilha silenciosa de input no mesmo dialeto.
