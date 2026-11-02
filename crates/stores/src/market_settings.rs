//! Market settings persistence and the one-way move of drawings out of layouts.
use crate::layouts;
use quantick_workspace::layout_commit::LayoutSave;
use quantick_workspace::layout_document::LayoutBook;
use quantick_workspace::layout_store::LayoutStore;
pub use quantick_workspace::market_settings::MARKET_SETTINGS_FILE;
use quantick_workspace::market_settings::{AbsorbReport, MarketSettings, parse_settings, to_text};
use quantick_workspace::market_settings_store::{AssetResolver, MarketSettingsStore};
use quantick_workspace::write_refusal::guard_write;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Beside `layouts.toml`: the file as it was before its drawings moved out, so
/// an older build or a change of mind still has them. Written once, never over.
const LAYOUTS_BACKUP_EXTENSION: &str = "toml.pre-market-settings";
const LAYOUTS_BACKUP_TEMP_EXTENSION: &str = "toml.pre-market-settings.tmp";

/// The gate a bundle section goes through.
///
/// # Errors
/// Returns the parser's reason when the document is invalid.
pub fn validate(text: &str) -> Result<(), String> {
    parse_settings(text)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// What [`open`] left behind, for the two stores' commit policies.
#[derive(Debug)]
pub struct Opened {
    pub settings: MarketSettings,
    /// This session's market settings must not replace the file.
    pub blocked: bool,
    /// This session's layouts must not replace the file: it still holds
    /// drawings only it has, because the market store or the backup failed.
    pub layouts_blocked: bool,
    pub report: AbsorbReport,
}

/// Read the file. A file this build refuses is set aside under a `.broken`
/// name, like a refused layouts file; `Err` says that failed too, and then the
/// session must never save over the trader's only copy.
fn load(path: &Path) -> Result<MarketSettings, ()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MarketSettings::default());
        }
        Err(error) => Err(error.to_string()),
    };
    let reason = match text.and_then(|text| parse_settings(&text).map_err(|e| e.to_string())) {
        Ok(settings) => return Ok(settings),
        Err(reason) => reason,
    };
    let aside = path.with_extension("toml.broken");
    let set_aside = guard_write(path).is_ok() && std::fs::rename(path, &aside).is_ok();
    tracing::warn!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "MARKET_SETTINGS_UNREADABLE",
        path = %path.display(),
        set_aside_to = %aside.display(),
        set_aside,
        reason = %reason,
        action = if set_aside { "starting_empty" } else { "starting_empty_and_never_saving_over_the_file" },
        "the market settings file could not be read"
    );
    if set_aside {
        Ok(MarketSettings::default())
    } else {
        Err(())
    }
}

/// Write the document. Temp sibling + rename, like every other store. Returns
/// whether it reached disk.
pub fn save(path: &Path, settings: &MarketSettings) -> bool {
    write_document(path, settings, false)
}

/// `flushed` waits for the disk before the rename: a migration is about to take
/// these drawings out of the only other file holding them. An ordinary save
/// runs on the frame path and has the previous file to fall back on.
fn write_document(path: &Path, settings: &MarketSettings, flushed: bool) -> bool {
    if guard_write(path).is_err() {
        return false;
    }
    let written = to_text(settings)
        .map_err(|error| error.to_string())
        .and_then(|text| {
            let temp = path.with_extension("toml.tmp");
            std::fs::File::create(&temp)
                .and_then(|mut file| {
                    file.write_all(text.as_bytes())?;
                    if flushed { file.sync_all() } else { Ok(()) }
                })
                .and_then(|()| std::fs::rename(&temp, path))
                .map_err(|error| {
                    let _ = std::fs::remove_file(&temp);
                    error.to_string()
                })
        });
    if let Err(error) = &written {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MARKET_SETTINGS_WRITE_FAILED",
            path = %path.display(),
            %error,
            action = "market_settings_not_saved",
            "could not save the market settings"
        );
    }
    written.is_ok()
}

/// Read the market settings and move in whatever drawings `book` still holds.
pub fn open(
    path: &Path,
    layouts_path: &Path,
    book: &mut LayoutBook,
    asset: impl Fn(&str, &str) -> String,
) -> Opened {
    let (mut settings, blocked) = match load(path) {
        Ok(settings) => (settings, false),
        Err(()) => (MarketSettings::default(), true),
    };
    let (report, layouts_blocked) =
        migrate(&mut settings, blocked, path, layouts_path, book, asset);
    Opened {
        settings,
        blocked,
        layouts_blocked,
        report,
    }
}

/// The layouts and the market store a cockpit opens with, whatever drawings the
/// book still held already moved to their markets. `blocked`: the book's load.
#[must_use]
pub fn open_layout_store(
    mut book: LayoutBook,
    layouts_path: PathBuf,
    blocked: bool,
    market_path: PathBuf,
    assets: AssetResolver,
) -> LayoutStore {
    let opened = open(&market_path, &layouts_path, &mut book, |venue, symbol| {
        assets(venue, symbol)
    });
    let market =
        MarketSettingsStore::new(opened.settings, market_path, opened.blocked).with_assets(assets);
    LayoutStore::new(book, layouts_path, blocked || opened.layouts_blocked).with_market(market)
}

/// A workspace import landed. Take the market file it brought, when it brought
/// one, then whatever drawings the book it brought still holds, as an old
/// export's do. Returns whether the layouts file must stay as it is.
pub fn reload(
    market: &mut MarketSettingsStore,
    brought_market_file: bool,
    layouts_path: &Path,
    book: &mut LayoutBook,
) -> bool {
    let path = market.path().to_owned();
    let mut blocked = market.is_blocked();
    if brought_market_file {
        blocked = false;
        match load(&path) {
            Ok(settings) => market.set_settings(settings),
            Err(()) => blocked = true,
        }
        market.set_blocked(blocked);
    }
    let assets = market.assets();
    let (_, kept_back) = migrate(
        market.settings_mut(),
        blocked,
        &path,
        layouts_path,
        book,
        |venue, symbol| assets(venue, symbol),
    );
    kept_back
}

/// Carry out what the commit policy decided; see [`crate::layouts::act_on`].
pub fn act_on(market: &MarketSettingsStore, decision: LayoutSave) {
    match decision {
        LayoutSave::Wait => {}
        LayoutSave::Write => {
            save(market.path(), market.settings());
        }
        LayoutSave::Blocked => tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MARKET_SETTINGS_SAVE_BLOCKED",
            path = %market.path().display(),
            action = "file_left_untouched",
            "the market settings file could not be read at launch and was not set aside; this session's are not written over it"
        ),
    }
}

/// Move `book`'s drawings into `settings` and persist the move in the only safe
/// order: the old file kept, the market store written, then the layouts written
/// without them. A crash between the last two re-runs harmlessly, since
/// [`MarketSettings::absorb`] does not take an entry twice. Returns the report
/// and whether the layouts must stay as they are on disk.
pub fn migrate(
    settings: &mut MarketSettings,
    market_blocked: bool,
    path: &Path,
    layouts_path: &Path,
    book: &mut LayoutBook,
    asset: impl Fn(&str, &str) -> String,
) -> (AbsorbReport, bool) {
    if !book.has_legacy_drawings() {
        return (AbsorbReport::default(), false);
    }
    let report = settings.absorb(book, asset);
    let durable = !market_blocked
        && keep_old_layouts(layouts_path)
        && write_document(path, settings, true)
        && layouts::save(layouts_path, book);
    tracing::info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "MARKET_SETTINGS_MIGRATED",
        moved = report.moved,
        already_held = report.already_held,
        durable,
        action = if durable { "drawings_moved_out_of_layouts" } else { "layouts_file_left_untouched" },
        "drawings moved from the layouts to their markets"
    );
    (report, !durable)
}

fn backup_path(layouts_path: &Path) -> PathBuf {
    layouts_path.with_extension(LAYOUTS_BACKUP_EXTENSION)
}

/// Whether the layouts file as it stands is safe to rewrite: its first copy is
/// on disk, or there is no file to lose.
fn keep_old_layouts(layouts_path: &Path) -> bool {
    let backup = backup_path(layouts_path);
    if guard_write(&backup).is_err() {
        return false;
    }
    if backup.exists() || !layouts_path.exists() {
        return true;
    }
    // Through a temp sibling: this file is trusted for merely existing, so a
    // copy cut short must never carry its name.
    let temp = layouts_path.with_extension(LAYOUTS_BACKUP_TEMP_EXTENSION);
    let copied = std::fs::copy(layouts_path, &temp)
        .and_then(|_| std::fs::OpenOptions::new().write(true).open(&temp))
        .and_then(|file| file.sync_all())
        .and_then(|()| std::fs::rename(&temp, &backup));
    match copied {
        Ok(()) => true,
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MARKET_SETTINGS_BACKUP_FAILED",
                path = %backup.display(),
                %error,
                action = "layouts_file_left_untouched",
                "could not keep the layouts file before moving its drawings"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::ScratchDir;
    use quantick_workspace::layout_document::parse;
    use quantick_workspace::market_settings::{DrawingKey, MarketKey, PaneRole};

    const LEGACY: &str =
        include_str!("../../workspace/fixtures/market-settings/legacy-layouts.toml");

    fn asset(_venue: &str, symbol: &str) -> String {
        if symbol.starts_with("WIN") {
            "WIN*".to_owned()
        } else {
            symbol.to_owned()
        }
    }

    struct Home {
        _dir: ScratchDir,
        market: PathBuf,
        layouts: PathBuf,
    }

    fn home(name: &str) -> Home {
        let dir = ScratchDir::new(name);
        let market = dir.join(MARKET_SETTINGS_FILE);
        let layouts = dir.join(layouts::LAYOUTS_FILE);
        std::fs::write(&layouts, LEGACY).unwrap();
        Home {
            _dir: dir,
            market,
            layouts,
        }
    }

    #[test]
    fn a_first_open_keeps_the_old_file_then_moves_the_drawings_to_disk() {
        let home = home("market-first-open");
        let mut book = parse(LEGACY).unwrap();
        let opened = open(&home.market, &home.layouts, &mut book, asset);
        assert_eq!(opened.report.moved, 8);
        assert!(!opened.blocked && !opened.layouts_blocked);
        assert_eq!(
            std::fs::read_to_string(backup_path(&home.layouts)).unwrap(),
            LEGACY,
            "the first copy, byte for byte"
        );
        let on_disk = parse(&std::fs::read_to_string(&home.layouts).unwrap()).unwrap();
        assert!(!on_disk.has_legacy_drawings());
        let reread = parse_settings(&std::fs::read_to_string(&home.market).unwrap()).unwrap();
        assert_eq!(reread, opened.settings);
    }

    #[test]
    fn a_second_open_finds_nothing_to_move() {
        let home = home("market-second-open");
        let mut book = parse(LEGACY).unwrap();
        let first = open(&home.market, &home.layouts, &mut book, asset);
        let mut again = parse(&std::fs::read_to_string(&home.layouts).unwrap()).unwrap();
        let second = open(&home.market, &home.layouts, &mut again, asset);
        assert_eq!(second.report, AbsorbReport::default());
        assert_eq!(second.settings, first.settings);
    }

    /// The launch that wrote the store and died before rewriting the layouts.
    #[test]
    fn a_launch_that_died_between_the_writes_duplicates_nothing() {
        let home = home("market-crash");
        let mut book = parse(LEGACY).unwrap();
        let first = open(&home.market, &home.layouts, &mut book, asset);
        std::fs::write(&home.layouts, LEGACY).unwrap();
        let mut again = parse(LEGACY).unwrap();
        let second = open(&home.market, &home.layouts, &mut again, asset);
        assert_eq!(second.settings, first.settings);
        assert_eq!((second.report.moved, second.report.already_held), (0, 8));
    }

    /// The store cannot be written: the layouts keep the only durable copy.
    #[test]
    fn a_market_file_that_cannot_be_written_leaves_the_layouts_alone() {
        let home = home("market-unwritable");
        let nowhere = home
            .market
            .with_file_name("no-such-folder")
            .join(MARKET_SETTINGS_FILE);
        let mut book = parse(LEGACY).unwrap();
        let opened = open(&nowhere, &home.layouts, &mut book, asset);
        assert!(opened.layouts_blocked);
        assert_eq!(std::fs::read_to_string(&home.layouts).unwrap(), LEGACY);
        assert_eq!(
            opened.settings.drawing_count(),
            8,
            "still shown this session"
        );
    }

    #[test]
    fn a_broken_market_file_is_set_aside_and_never_written_over() {
        let home = home("market-broken");
        std::fs::write(&home.market, "not toml [").unwrap();
        let mut book = LayoutBook::default();
        let opened = open(&home.market, &home.layouts, &mut book, asset);
        assert!(!opened.blocked);
        assert_eq!(
            std::fs::read_to_string(home.market.with_extension("toml.broken")).unwrap(),
            "not toml ["
        );
        assert_eq!(opened.settings, MarketSettings::default());
    }

    /// An old export landing on a cockpit already migrated adds its marks to
    /// the markets instead of replacing them.
    #[test]
    fn an_old_export_lands_in_the_markets_and_the_first_backup_stands() {
        let home = home("market-old-export");
        let mut settings = MarketSettings::default();
        let key = DrawingKey {
            market: MarketKey {
                venue: "binance".to_owned(),
                asset: "ETHUSDT".to_owned(),
            },
            role: PaneRole::Flow,
        };
        let mut own = parse(LEGACY).unwrap();
        settings.absorb(&mut own, asset);
        let before = settings.drawing_count();
        assert!(settings.drawings(&key).is_empty());
        std::fs::write(backup_path(&home.layouts), "first").unwrap();
        let exported = LEGACY.replace("Force Bar Only", "From the export");
        let mut book = parse(&exported).unwrap();
        let (report, blocked) = migrate(
            &mut settings,
            false,
            &home.market,
            &home.layouts,
            &mut book,
            asset,
        );
        assert!(!blocked);
        assert_eq!(report.moved, 3, "only the renamed layout's entries are new");
        assert_eq!(settings.drawing_count(), before + 3);
        assert_eq!(
            std::fs::read_to_string(backup_path(&home.layouts)).unwrap(),
            "first"
        );
    }

    /// A store that may not be written stays unwritten when an old export lands.
    #[test]
    fn an_import_never_writes_over_a_blocked_market_file() {
        let home = home("market-blocked-import");
        std::fs::write(&home.market, "the trader's only copy").unwrap();
        let mut market =
            MarketSettingsStore::new(MarketSettings::default(), home.market.clone(), true);
        let mut book = parse(LEGACY).unwrap();
        assert!(reload(&mut market, false, &home.layouts, &mut book));
        assert_eq!(
            std::fs::read_to_string(&home.market).unwrap(),
            "the trader's only copy"
        );
        assert_eq!(std::fs::read_to_string(&home.layouts).unwrap(), LEGACY);
        assert!(market.is_blocked(), "and it stays blocked");
    }

    /// The layouts cannot be rewritten: not durable, and this session says so.
    #[test]
    fn layouts_that_cannot_be_rewritten_block_the_session() {
        let home = home("market-layouts-unwritable");
        let nowhere = home
            .layouts
            .with_file_name("no-such-folder")
            .join(layouts::LAYOUTS_FILE);
        let mut book = parse(LEGACY).unwrap();
        let opened = open(&home.market, &nowhere, &mut book, asset);
        assert!(opened.layouts_blocked);
        assert!(!opened.blocked);
    }

    #[test]
    fn no_temp_file_is_left_beside_the_backup() {
        let home = home("market-backup-temp");
        let mut book = parse(LEGACY).unwrap();
        open(&home.market, &home.layouts, &mut book, asset);
        assert!(backup_path(&home.layouts).exists());
        assert!(
            !home
                .layouts
                .with_extension(LAYOUTS_BACKUP_TEMP_EXTENSION)
                .exists()
        );
    }

    #[test]
    fn validation_is_the_parser_s() {
        assert!(validate("version = 1\n").is_ok());
        assert!(validate("version = 99\n").unwrap_err().contains("99"));
    }
}
