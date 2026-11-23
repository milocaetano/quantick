# Describe a strategy

Choose **Panels → Describe a strategy…** on the chart you want to watch. Send a sentence describing when the alarm should ring and answer any clarification. Quantick compiles the returned Pine, checks its alert titles and can send the compiler diagnostics for up to three corrections.

The preview names the captured range and lists the signal direction, absolute chart slot and recorded close timestamp. It uses up to the last 512 loaded closed bars. Footprint POC is missing on bars without recorded footprints. New closes on the same chart leave the preview pinned. The dialog follows that tab when you detach it or return it to a market window. Switching charts or bar rules cancels the conversation. A selected armable rectangle constrains the alarm to its region. Otherwise the alarm watches the pane anywhere.

Enter a new `.pine` file name and choose **Save and arm alarms** to warm the admitted sides on the current live series, create the exact source in the strategy library and register the alarms. Script names are case-insensitive. Concurrent saves from Describe can publish only one spelling of a name. Alarms run locally on closed bars and place no orders.

## Provider configuration

Set the existing strategy provider table in the chart configuration as shown below. Add the table if the configuration omits it, then restart Quantick. The endpoint is the full Chat Completions URL. Use the model ID your provider serves.

```toml
[strategy_describe]
enabled = true
provider = "openai-compatible"
model = "your-model-id"
endpoint = "https://api.openai.com/v1/chat/completions"
key_env = "STRATEGY_PROVIDER_KEY"
```

Alternatively set `key` to the credential in your private configuration. Configure one key source. Missing or blank credentials hide the menu entry. A submitted conversation sends the chart identifiers, bar specification, indicator and drawing context, closed bars and conversation text to that endpoint. Compiler corrections also send the candidate source and diagnostics. Quantick rejects a chart label, message or filename containing the configured credential before opening, submitting or saving that input. Transport failures end the request. Open a new conversation to try again.

## Local agent access

Discover `strategy.describe.open`, `.read`, `.submit`, `.cancel` and `.save` with the local MCP client. The read operation exposes the same conversation and preview as the window. Mutations require the window's relevant annotation and user-text grants. Supply an idempotency key on mutations, and pass the revision from the latest status on submit, cancel and save.
