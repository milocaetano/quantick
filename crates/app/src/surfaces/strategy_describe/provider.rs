//! The credential and HTTP client live on one worker with bounded mailboxes.
use quantick_ui_model::strategy_describe::{
    Context, Reply, Request,
    conversation::{Completion, Provider},
    credential::Credential,
    protocol,
};
use serde::Deserialize;
use std::{
    sync::Arc,
    sync::mpsc::{Receiver, SyncSender, sync_channel},
    time::Duration,
};

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
        let credential = Arc::new(Credential::new(key.trim()));
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
}

impl Provider for Worker {
    fn available(&self) -> bool {
        self.mailbox.is_some()
    }
    fn validate_input(&self, text: &str) -> Result<(), &'static str> {
        self.mailbox
            .as_ref()
            .ok_or("Strategy description is unavailable")?
            .credential
            .validate(text)
    }
    fn validate_chart(&self, context: &Context) -> Result<(), &'static str> {
        self.mailbox
            .as_ref()
            .ok_or("Strategy description is unavailable")?
            .credential
            .chart(context)
    }
    fn submit(&mut self, request: Request) -> Result<(), String> {
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
    fn invalidate(&self) {
        if let Some(mailbox) = &self.mailbox {
            mailbox.current.send_replace(None);
        }
    }
    fn poll(&mut self) -> Option<Completion> {
        self.mailbox.as_ref()?.receive.try_recv().ok()
    }
}
