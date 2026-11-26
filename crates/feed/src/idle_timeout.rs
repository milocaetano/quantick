//! How long each venue's trade websocket may stay silent before it is
//! reconnected, as the application's composition root configured it.
//!
//! The application reads `idle_timeout_secs` from every `[[feeds]]` entry
//! once at launch and hands the values in; this crate reads no config file.
//! A feed is spawned by provider, so several feeds on one provider share the
//! shortest window any of them declares.

use std::sync::OnceLock;
use std::time::Duration;

use quantick_sources::config::ProviderKind;
use quantick_sources::reconnect::DEFAULT_IDLE_TIMEOUT_SECS;

static IDLE_TIMEOUTS: OnceLock<Vec<(ProviderKind, Duration)>> = OnceLock::new();

/// Set the per-provider idle timeouts, once, from the configured feeds.
/// Never configured, every provider uses [`DEFAULT_IDLE_TIMEOUT_SECS`].
pub(crate) fn configure_idle_timeouts(feeds: impl IntoIterator<Item = (ProviderKind, Duration)>) {
    let _ = IDLE_TIMEOUTS.set(feeds.into_iter().collect());
}

/// The idle timeout a `provider`'s trade stream runs with.
pub(crate) fn idle_timeout(provider: ProviderKind) -> Duration {
    resolve(IDLE_TIMEOUTS.get().map_or(&[], Vec::as_slice), provider)
}

fn resolve(declared: &[(ProviderKind, Duration)], provider: ProviderKind) -> Duration {
    declared
        .iter()
        .filter(|(kind, _)| *kind == provider)
        .map(|(_, timeout)| *timeout)
        .min()
        .unwrap_or(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shortest_declared_window_per_provider_wins() {
        let secs = Duration::from_secs;
        let declared = [
            (ProviderKind::Binance, secs(60)),
            (ProviderKind::Hyperliquid, secs(45)),
            (ProviderKind::Binance, secs(20)),
        ];
        assert_eq!(resolve(&declared, ProviderKind::Binance), secs(20));
        assert_eq!(resolve(&declared, ProviderKind::Hyperliquid), secs(45));
        assert_eq!(
            resolve(&declared, ProviderKind::BinanceFutures),
            secs(DEFAULT_IDLE_TIMEOUT_SECS)
        );
    }
}
