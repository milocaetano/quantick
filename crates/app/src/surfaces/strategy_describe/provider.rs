//! The credential and HTTP client live on one worker with bounded mailboxes.
use quantick_ui_model::strategy_describe::{Context, Reply, Request, Turn};
use serde::Deserialize;
use std::{
    sync::Arc,
    sync::mpsc::{Receiver, SyncSender, sync_channel},
    time::Duration,
};

use super::protocol;

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Settings {
    enabled: bool,
    provider: String,
    model: String,
    endpoint: Option<String>,
    key: Option<String>,
    key_env: Option<String>,
}

/// A short local-provider token can also occur inside an ordinary word.
fn includes_credential(text: &str, key: &str) -> bool {
    const MIN_DISTINCT_TOKEN_BYTES: usize = 8;
    if key.len() >= MIN_DISTINCT_TOKEN_BYTES {
        return text.contains(key);
    }
    text.match_indices(key).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + key.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

struct Credential(Box<str>);

impl Credential {
    fn validate(&self, text: &str) -> Result<(), &'static str> {
        if includes_credential(text, &self.0) {
            return Err("Input contains the provider credential");
        }
        Ok(())
    }

    fn chart(&self, context: &Context) -> Result<(), &'static str> {
        for text in [&context.feed, &context.symbol, &context.bars]
            .into_iter()
            .chain(&context.indicators)
            .chain(&context.drawings)
            .chain(&context.availability)
        {
            self.validate(text)?;
        }
        Ok(())
    }

    fn request(&self, request: &Request) -> Result<(), &'static str> {
        self.chart(&request.context)?;
        for turn in &request.transcript {
            let (Turn::Trader(text) | Turn::Assistant(text)) = turn;
            self.validate(text)?;
        }
        if let Some((source, diagnostics)) = &request.repair {
            self.validate(source)?;
            for diagnostic in diagnostics {
                self.validate(&diagnostic.code)?;
                self.validate(&diagnostic.message)?;
            }
        }
        Ok(())
    }
}

type Completion = (u64, Result<Reply, String>);

#[cfg(test)]
#[test]
fn credential_echo_is_distinct_from_an_incidental_letter() {
    assert!(!includes_credential("exit on reversal", "x"));
    assert!(includes_credential("Bearer x.", "x"));
    assert!(includes_credential(
        "credential: (sk-test-only)",
        "sk-test-only"
    ));
}

#[cfg(test)]
#[test]
fn outbound_repair_and_chart_fields_are_checked_before_serialization() {
    use quantick_session::script_strategies::ScriptPlacement;
    use quantick_ui_model::strategy_describe::Diagnostic;
    let credential = Credential(r#"fixture-"quoted"-secret"#.into());
    let context = Context {
        workspace: quantick_workspace::lifetime::WorkspaceId(1),
        tab: 3,
        pane: 7,
        feed: "test".into(),
        symbol: "TEST".into(),
        bars: "renko:3".into(),
        placement: ScriptPlacement::Anywhere,
        indicators: vec![],
        drawings: vec![],
        availability: vec![],
        history: vec![],
    };
    let mut request = Request {
        revision: 1,
        context: Arc::new(context),
        transcript: vec![],
        repair: None,
    };
    assert!(credential.request(&request).is_ok());
    Arc::make_mut(&mut request.context)
        .indicators
        .push(credential.0.to_string());
    assert!(credential.request(&request).is_err());
    Arc::make_mut(&mut request.context).indicators.clear();
    request.repair = Some((
        "source".into(),
        vec![Diagnostic {
            line: Some(1),
            code: "PINE_TEST".into(),
            message: credential.0.to_string(),
        }],
    ));
    assert!(credential.request(&request).is_err());
}

#[derive(Default)]
pub(crate) struct Worker {
    mailbox: Option<Mailbox>,
}
struct Mailbox {
    credential: Arc<Credential>,
    send: SyncSender<Request>,
    receive: Receiver<Completion>,
    current: tokio::sync::watch::Sender<Option<u64>>,
}
impl Worker {
    pub(crate) fn configured(
        settings: Settings,
        lookup: impl FnOnce(&str) -> Option<String>,
    ) -> Result<Self, &'static str> {
        if !settings.enabled {
            return Ok(Self::default());
        }
        if settings.provider != "openai-compatible" || settings.model.trim().is_empty() {
            return Err("Select openai-compatible and specify a model in strategy_describe");
        }
        let key = match (settings.key, settings.key_env) {
            (Some(_), Some(_)) => {
                return Err("Configure only one of key and key_env in strategy_describe");
            }
            (None, Some(name)) => lookup(&name),
            (key, None) => key,
        };
        let Some(key) = key.filter(|key| !key.trim().is_empty()) else {
            return Ok(Self::default());
        };
        let endpoint = reqwest::Url::parse(
            settings
                .endpoint
                .as_deref()
                .unwrap_or("https://api.openai.com/v1/chat/completions"),
        )
        .map_err(|_| "Invalid strategy provider endpoint")?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
        {
            return Err("Use an HTTP(S) endpoint without embedded credentials or a fragment");
        }
        let mut authorization =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", key.trim()))
                .map_err(|_| "Invalid strategy provider credential")?;
        authorization.set_sensitive(true);
        let credential = Arc::new(Credential(key.trim().into()));
        credential.validate(&settings.model)?;
        let transport = Arc::clone(&credential);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Cannot start strategy provider runtime")?;
        let client = reqwest::Client::builder()
            .retry(reqwest::retry::never())
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| "Cannot initialize strategy provider")?;
        let (send, requests) = sync_channel::<Request>(1);
        let (finished, receive) = sync_channel(1);
        let (current, mut revision) = tokio::sync::watch::channel(None);
        std::thread::Builder::new()
            .name("strategy-provider".into())
            .spawn(move || {
                for request in requests {
                    if *revision.borrow_and_update() != Some(request.revision) {
                        continue;
                    }
                    let exchange = async {
                        transport.request(&request)?;
                        let body = protocol::body(&settings.model, &request)?;
                        let mut response = client
                            .post(endpoint.clone())
                            .header(reqwest::header::AUTHORIZATION, authorization.clone())
                            .header(reqwest::header::CONTENT_TYPE, "application/json")
                            .body(body)
                            .send()
                            .await
                            .map_err(|error| {
                                if error.is_timeout() {
                                    "Strategy provider timed out"
                                } else {
                                    "Could not reach strategy provider"
                                }
                            })?;
                        if !response.status().is_success() {
                            return Err(format!(
                                "Strategy provider returned HTTP {}",
                                response.status().as_u16()
                            ));
                        }
                        let mut bytes = Vec::new();
                        while let Some(chunk) = response
                            .chunk()
                            .await
                            .map_err(|_| "Incomplete strategy provider response")?
                        {
                            if bytes.len() + chunk.len() > protocol::MAX_RESPONSE {
                                return Err("Provider response exceeds size limit".into());
                            }
                            bytes.extend_from_slice(&chunk);
                        }
                        let reply = protocol::parse(&bytes)?;
                        match &reply {
                            Reply::Question(text) => transport.validate(text),
                            Reply::Candidate { source, summary } => transport
                                .validate(source)
                                .and_then(|()| transport.validate(summary)),
                        }
                        .map_err(|_| "Provider response contained a credential")?;
                        Ok(reply)
                    };
                    let result = runtime.block_on(async {
                        tokio::select! {
                            biased;
                            _ = revision.changed() => None,
                            result = exchange => Some(result),
                        }
                    });
                    if let Some(result) = result
                        && finished.send((request.revision, result)).is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|_| "Cannot start strategy provider worker")?;
        Ok(Self {
            mailbox: Some(Mailbox {
                credential,
                send,
                receive,
                current,
            }),
        })
    }
    pub(crate) fn available(&self) -> bool {
        self.mailbox.is_some()
    }
    pub(crate) fn validate_input(&self, text: &str) -> Result<(), &'static str> {
        self.mailbox
            .as_ref()
            .ok_or("Strategy description is unavailable")?
            .credential
            .validate(text)
    }

    pub(crate) fn validate_chart(&self, context: &Context) -> Result<(), &'static str> {
        self.mailbox
            .as_ref()
            .ok_or("Strategy description is unavailable")?
            .credential
            .chart(context)
    }

    pub(crate) fn submit(&mut self, request: Request) -> Result<(), String> {
        let mailbox = self
            .mailbox
            .as_ref()
            .ok_or("Strategy description is unavailable")?;
        mailbox.current.send_replace(Some(request.revision));
        mailbox
            .send
            .try_send(request)
            .map_err(|_| "Strategy provider is busy or disconnected".into())
    }
    pub(crate) fn invalidate(&self) {
        if let Some(mailbox) = &self.mailbox {
            mailbox.current.send_replace(None);
        }
    }
    pub(crate) fn poll(&mut self) -> Option<Completion> {
        self.mailbox.as_ref()?.receive.try_recv().ok()
    }
}
