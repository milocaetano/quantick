//! The provider credential never leaves in a request or comes back in a reply.
use super::{Context, Request, Turn};

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

pub struct Credential(Box<str>);

impl Credential {
    pub fn new(key: &str) -> Self {
        Self(key.into())
    }

    pub fn validate(&self, text: &str) -> Result<(), &'static str> {
        if includes_credential(text, &self.0) {
            return Err("Input contains the provider credential");
        }
        Ok(())
    }

    pub fn chart(&self, context: &Context) -> Result<(), &'static str> {
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

    pub fn request(&self, request: &Request) -> Result<(), &'static str> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy_describe::Diagnostic;
    use quantick_session::script_strategies::ScriptPlacement;
    use std::sync::Arc;

    #[test]
    fn credential_echo_is_distinct_from_an_incidental_letter() {
        assert!(!includes_credential("exit on reversal", "x"));
        assert!(includes_credential("Bearer x.", "x"));
        assert!(includes_credential(
            "credential: (sk-test-only)",
            "sk-test-only"
        ));
    }

    #[test]
    fn outbound_repair_and_chart_fields_are_checked_before_serialization() {
        let credential = Credential::new(r#"fixture-"quoted"-secret"#);
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
}
