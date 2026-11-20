# Describe a strategy

The conversation surface and its five named calls are reserved in this build. The provider worker is disabled. On a normal launch the Panels menu has no strategy-description entry.

The `[strategy_describe]` section of the feeds configuration accepts `enabled`, `provider`, `model`, an optional `endpoint`, and either `key_env` or a literal `key`. The provider selection is `openai-compatible`. Only the local app resolves credentials. `strategy.describe.read` reports the disabled state.
