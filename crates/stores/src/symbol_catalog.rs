//! Validated edits to the feed catalogue and its user-added sidecar.
use crate::{config::AppConfig, symbols_file};

/// The instruments the picker can add to a feed: the running config, the
/// user's own additions kept apart from it, and the sidecar they persist in.
///
/// The config file itself is never written: it is hand-written, comments
/// and all, and a program that rewrote it would eat them. An addition lives
/// in its own sidecar, which the next launch folds back in before the config
/// is validated (see [`crate::symbols_file`]).
pub struct SymbolCatalog<'a> {
    pub config: &'a mut AppConfig,
    pub added: &'a mut symbols_file::AddedSymbols,
    pub path: &'a std::path::Path,
}

impl SymbolCatalog<'_> {
    /// Put `symbol` in feed `feed_id`'s catalog and remember it across
    /// restarts. Reports whether the catalog took it.
    pub fn add(&mut self, feed_id: &str, symbol: &str) -> Result<(), String> {
        // Against the *whole* config, on a copy. A symbol is not just a name
        // in a list: it takes part in every cross-check the config has, and
        // the MetaTrader port map is one where a single mapped symbol offered
        // by two feeds is a configuration the app refuses to load. Persisting
        // one of those would write a file that kills the next launch — and the
        // error would name the config, which is not the file that broke.
        let mut candidate = self.config.clone();
        if !candidate.add_symbol(feed_id, symbol) {
            return Err(format!(
                "{} already offers {symbol}",
                self.config.feed_name(feed_id)
            ));
        }
        candidate.validate()?;
        *self.config = candidate;
        self.added.add(feed_id, symbol);
        if let Err(error) = symbols_file::save(self.path, self.added) {
            // The catalog took it for this session either way; what is lost is
            // the next launch, and the user is told which file did not take it.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_WRITE_FAILED",
                path = %self.path.display(),
                error = %error,
                action = "addition_is_session_only",
                "cannot write the added-symbols file"
            );
        }
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "SYMBOL_ADDED",
            feed = %feed_id,
            symbol = %symbol,
            path = %self.path.display(),
            action = "open_in_new_tab",
            "a symbol was added from the source picker"
        );
        Ok(())
    }

    /// Take a user-added `symbol` back out of feed `feed_id`'s catalog.
    ///
    /// Only ever a catalog edit: a tab already showing that market keeps
    /// streaming it. The picker will not offer this for a market a tab is on,
    /// which is what stops the selection correction from retargeting it.
    pub fn remove(&mut self, feed_id: &str, symbol: &str) {
        if !self.config.remove_symbol(feed_id, symbol) {
            return;
        }
        self.added.remove(feed_id, symbol);
        if let Err(error) = symbols_file::save(self.path, self.added) {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_WRITE_FAILED",
                path = %self.path.display(),
                error = %error,
                action = "removal_is_session_only",
                "cannot write the added-symbols file"
            );
        }
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "SYMBOL_REMOVED",
            feed = %feed_id,
            symbol = %symbol,
            path = %self.path.display(),
            action = "leave_open_tabs_alone",
            "a user-added symbol left the catalog"
        );
    }
}
