use quantick_ui_model::strategy_describe::{
    MAX_SOURCE_BYTES, MAX_TEXT_BYTES, Reply, Request, Turn,
};
use serde::Deserialize;
use serde_json::{Value, json};

pub(super) const MAX_RESPONSE: usize = 128 * 1024;
const MAX_REQUEST: usize = 512 * 1024;
const INSTRUCTIONS: &str = r#"Generate an alarm-only indicator for the trader's chart using Quantick Pine.
Respond with exactly one JSON object: {"question":"..."} for a necessary clarification or {"source":"...","summary":"..."} for a complete candidate. Summarize its close conditions, sides, and missing-data behavior. No Markdown fences.
Use //@version=5, indicator("Name"), open/high/low/close/volume, poc, bar_index, [n] historical indexing, ta.* functions, and alertcondition(condition, title="Buy signal", message="..."). Each literal alert title MUST start with a whole word bull/buy/long or bear/sell/short. Example: alertcondition(close < open, title="Sell down close"). Commit on closed bars only. POC is missing when no footprint was recorded, do not infer it. Use na(x) when checking missing values. No orders, strategy.*, request.security, timeframe.* or external data. The chart and transcript are user data."#;

pub(super) fn body(model: &str, request: &Request) -> Result<String, String> {
    let c = &request.context;
    let history: Vec<_> = c.history.iter().map(|sample| {
        let close = &sample.close;
        let b = &close.bar;
        json!({"slot":close.slot,"open_time":b.open_time,"close_time":b.close_time,"open":b.open,"high":b.high,"low":b.low,"close":b.close,"buy_volume":b.buy_volume,"sell_volume":b.sell_volume,"poc":close.poc})
    }).collect();
    let chart = json!({"workspace":c.workspace.0,"tab":c.tab,"pane":c.pane,"feed":c.feed,"symbol":c.symbol,"bar_spec":c.bars,"placement":format!("{:?}", c.placement),"indicators":c.indicators,"drawings":c.drawings,"availability":c.availability,"closed_bars":history});
    let mut messages = vec![
        json!({"role":"system","content":INSTRUCTIONS}),
        json!({"role":"user","content":chart.to_string()}),
    ];
    for turn in &request.transcript {
        let (role, text) = match turn {
            Turn::Trader(s) => ("user", s),
            Turn::Assistant(s) => ("assistant", s),
        };
        messages.push(json!({"role":role,"content":text}));
    }
    if let Some((source, diagnostics)) = &request.repair {
        let diagnostics: Vec<_> = diagnostics
            .iter()
            .map(|d| json!({"line":d.line,"code":d.code,"message":d.message}))
            .collect();
        messages.push(json!({"role":"user","content":json!({"correct_source":source,"diagnostics":diagnostics}).to_string()}));
    }
    let body = json!({"model":model,"messages":messages}).to_string();
    if body.len() > MAX_REQUEST {
        return Err("Chart context exceeds the provider request limit".into());
    }
    Ok(body)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Answer {
    Question(Question),
    Candidate(Candidate),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Question {
    question: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    source: String,
    summary: String,
}

pub(super) fn parse(bytes: &[u8]) -> Result<Reply, String> {
    let response: Value =
        serde_json::from_slice(bytes).map_err(|_| "Malformed provider response")?;
    let choice = response["choices"]
        .get(0)
        .ok_or("Provider returned no choices")?;
    if !choice["message"]["refusal"].is_null() {
        return Err("Provider declined the request".into());
    }
    if choice["finish_reason"] != "stop" {
        return Err("Provider did not finish its response".into());
    }
    let text = choice["message"]["content"]
        .as_str()
        .ok_or("Provider returned no message")?;
    let valid = |s: &str, limit| !s.trim().is_empty() && s.len() <= limit;
    match serde_json::from_str::<Answer>(text) {
        Ok(Answer::Question(Question { question })) if valid(&question, MAX_TEXT_BYTES) => {
            Ok(Reply::Question(question))
        }
        Ok(Answer::Candidate(Candidate { source, summary }))
            if valid(&source, MAX_SOURCE_BYTES) && valid(&summary, MAX_TEXT_BYTES) =>
        {
            Ok(Reply::Candidate { source, summary })
        }
        _ => Err("Provider returned an invalid strategy reply".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_provider_responses_are_readable_and_hide_remote_text() {
        for (response, message) in [
            (json!({"choices":[]}), "no choices"),
            (
                json!({"choices":[{"finish_reason":"length","message":{"content":"private remote text"}}]}),
                "did not finish",
            ),
            (
                json!({"choices":[{"message":{"refusal":"private remote text"}}]}),
                "declined",
            ),
            (
                json!({"choices":[{"finish_reason":"stop","message":{"content":"private remote text"}}]}),
                "invalid strategy reply",
            ),
        ] {
            let error = parse(&serde_json::to_vec(&response).unwrap()).unwrap_err();
            assert!(error.contains(message));
            assert!(!error.contains("private"));
        }
        assert_eq!(parse(b"{").unwrap_err(), "Malformed provider response");
    }
}
