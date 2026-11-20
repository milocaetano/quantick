//! The app resolves provider credentials and owns the one bounded worker mailbox.
use std::sync::mpsc::{Receiver, SyncSender};

use quantick_ui_model::strategy_describe::{Reply, Request};
use serde::Deserialize;

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

type Completion = (u64, Result<Reply, String>);

#[derive(Default)]
pub(crate) struct Worker {
    settings: Settings,
    mailbox: Option<(SyncSender<Request>, Receiver<Completion>)>,
}

impl Worker {
    pub(crate) fn configured(
        mut settings: Settings,
        lookup: impl FnOnce(&str) -> Option<String>,
    ) -> Result<Self, &'static str> {
        if settings.enabled {
            if settings.provider != "openai-compatible" || settings.model.trim().is_empty() {
                return Err("Select openai-compatible and specify a model in strategy_describe");
            }
            if let Some(endpoint) = &settings.endpoint {
                let url = reqwest::Url::parse(endpoint)
                    .map_err(|_| "Invalid strategy provider endpoint")?;
                if !matches!(url.scheme(), "https" | "http")
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err("Use an HTTP(S) endpoint without embedded credentials");
                }
            }
            if let Some(name) = settings.key_env.take() {
                if settings.key.is_some() {
                    return Err("Configure only one of key and key_env in strategy_describe");
                }
                settings.key = lookup(&name);
            }
        }
        Ok(Self {
            settings,
            mailbox: None,
        })
    }

    pub(crate) fn available(&self) -> bool {
        self.mailbox.is_some()
            && self.settings.enabled
            && self
                .settings
                .key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty())
    }

    pub(crate) fn submit(&mut self, request: Request) -> Result<(), String> {
        let (send, _) = self
            .mailbox
            .as_ref()
            .ok_or("Strategy description is unavailable")?;
        send.try_send(request)
            .map_err(|_| "Strategy provider is busy or disconnected".into())
    }
    pub(crate) fn poll(&mut self) -> Option<Completion> {
        self.mailbox.as_ref()?.1.try_recv().ok()
    }
}
