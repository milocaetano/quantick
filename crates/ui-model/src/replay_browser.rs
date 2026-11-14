//! Global replay browser presentation. The host supplies IO completions;
//! replay and feed retain parsing, download and playback authority.
use std::path::{Path, PathBuf};

use quantick_replay::{Library, SessionEntry};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BrowserTab {
    #[default]
    Sessions,
    GetData,
}

#[derive(Debug, Default)]
enum ScanSelection {
    #[default]
    First,
    Path(PathBuf),
}

#[derive(Debug)]
pub struct State {
    pub browser_open: bool,
    pub folder: String,
    pub library: Option<Library>,
    pub selected: Option<usize>,
    pub loading: Option<String>,
    pub picker_pending: bool,
    pub error: Option<String>,
    pub show_format: bool,
    pub speed: f32,
    pub autoplay: bool,
    pub day_before: bool,
    pub day_before_dirty: bool,
    pub day_before_stored: Option<bool>,
    pub tab: BrowserTab,
    pub stored: Option<String>,
    pub folder_dirty: bool,
    pub offered_symbols: Vec<String>,
    market_current: Option<String>,
    market_catalogue: Vec<String>,
    scan_selection: ScanSelection,
}

impl Default for State {
    fn default() -> Self {
        Self {
            browser_open: false,
            folder: String::new(),
            library: None,
            selected: None,
            loading: None,
            picker_pending: false,
            error: None,
            show_format: false,
            speed: 1.0,
            autoplay: false,
            day_before: true,
            day_before_dirty: false,
            day_before_stored: None,
            tab: BrowserTab::Sessions,
            stored: None,
            folder_dirty: false,
            offered_symbols: Vec::new(),
            market_current: None,
            market_catalogue: Vec::new(),
            scan_selection: ScanSelection::First,
        }
    }
}

impl State {
    pub fn selected_entry(&self) -> Option<&SessionEntry> {
        self.library.as_ref()?.sessions.get(self.selected?)
    }

    /// Compare the borrowed inputs before allocating or rebuilding offerings.
    pub fn observe_market(&mut self, current: Option<&str>, catalogue: &[&str]) -> bool {
        if self.market_current.as_deref() == current
            && self
                .market_catalogue
                .iter()
                .map(String::as_str)
                .eq(catalogue.iter().copied())
        {
            return false;
        }
        self.market_current = current.map(str::to_owned);
        self.market_catalogue = catalogue
            .iter()
            .map(|symbol| (*symbol).to_owned())
            .collect();
        self.rebuild_offerings();
        true
    }

    fn rebuild_offerings(&mut self) {
        self.offered_symbols.clear();
        let mut push = |symbol: &str| {
            let symbol = symbol.trim();
            if !symbol.is_empty() && !self.offered_symbols.iter().any(|held| held == symbol) {
                self.offered_symbols.push(symbol.to_owned());
            }
        };
        if let Some(current) = self.market_current.as_deref() {
            push(current);
        }
        for symbol in &self.market_catalogue {
            push(symbol);
        }
        if let Some(library) = &self.library {
            for entry in &library.sessions {
                push(&entry.symbol);
            }
        }
    }

    fn select_day(&mut self, wanted: &str) -> bool {
        let wanted = wanted.trim();
        let found = self.library.as_ref().and_then(|library| {
            library.sessions.iter().position(|entry| {
                entry.date.is_some_and(|date| date.label() == wanted)
                    || entry
                        .path
                        .file_stem()
                        .is_some_and(|stem| stem.eq_ignore_ascii_case(wanted))
            })
        });
        if let Some(index) = found {
            self.selected = Some(index);
        }
        found.is_some()
    }

    fn select_path(&mut self, path: &Path) {
        if let Some(library) = &self.library {
            self.selected = library.sessions.iter().position(|entry| entry.path == path);
        }
    }

    fn scan(&mut self, selection: ScanSelection) -> Effects {
        self.scan_selection = selection;
        Effects::local(LocalEffect::Scan {
            folder: self.folder.trim().to_owned(),
        })
    }

    fn choose_folder(&mut self) -> Effects {
        let folder = self.folder.trim();
        self.stored = (!folder.is_empty()).then(|| folder.to_owned());
        self.folder_dirty = true;
        self.scan(ScanSelection::First)
    }

    fn load_selected(&mut self) -> Effects {
        let Some(entry) = self.selected_entry() else {
            return Effects::default();
        };
        let path = entry.path.clone();
        self.loading = Some(entry.label());
        self.error = None;
        Effects {
            local: Some(LocalEffect::Load {
                path,
                day_before: self.day_before,
            }),
            load_started: true,
            ..Effects::default()
        }
    }
}

#[derive(Debug)]
pub enum Intent {
    Initialize {
        folder: String,
        stored: Option<String>,
        day_before: Option<bool>,
    },
    OpenBrowser,
    OpenFormatHelp,
    SetOpen(bool),
    SetFolderDraft(String),
    ChooseFolder,
    Rescan,
    SetDayBefore(bool),
    StageDayBefore(bool),
    TakeFolderChange,
    TakeDayBeforeChange,
    Select(usize),
    SelectDay(String),
    SelectPath(PathBuf),
    AutostartSelection {
        speed: f32,
        day: Option<String>,
        play: bool,
    },
    LoadSelected,
    EnterDownload {
        symbol: Option<String>,
        explicit: bool,
    },
    SelectSessions,
    SetSpeed(f32),
    SetAutoplay(bool),
    SetShowFormat(bool),
    PickFolder,
    PickerCompleted(Option<String>),
    PickerStarted,
    LoadStarted,
    DownloadEntered,
    LoadCompleted(Result<(), String>),
    ScanCompleted(Library),
    Downloaded(PathBuf),
    DownloadRefreshed,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LocalEffect {
    Scan {
        folder: String,
    },
    PickFolder {
        folder: String,
    },
    Load {
        path: PathBuf,
        day_before: bool,
    },
    EnterDownload {
        folder: String,
        symbol: Option<String>,
        explicit: bool,
    },
}

#[derive(Debug, Default, PartialEq)]
pub struct Effects {
    pub local: Option<LocalEffect>,
    pub open_loaded: Option<(f32, bool)>,
    pub folder_change: Option<Option<String>>,
    pub day_before_change: Option<bool>,
    pub selected_day: Option<bool>,
    pub load_started: bool,
}

impl Effects {
    fn local(local: LocalEffect) -> Self {
        Self {
            local: Some(local),
            ..Self::default()
        }
    }
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;

    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::Initialize {
                folder,
                stored,
                day_before,
            } => {
                *state = State {
                    folder,
                    stored,
                    day_before: day_before.unwrap_or(true),
                    day_before_stored: day_before,
                    ..State::default()
                };
            }
            Intent::OpenBrowser => {
                state.browser_open = true;
                if state.library.is_none() && !state.folder.trim().is_empty() {
                    return state.scan(ScanSelection::First);
                }
            }
            Intent::OpenFormatHelp => {
                state.browser_open = true;
                state.show_format = true;
            }
            Intent::SetOpen(open) => state.browser_open = open,
            Intent::SetFolderDraft(folder) => state.folder = folder,
            Intent::ChooseFolder => return state.choose_folder(),
            Intent::Rescan => return state.scan(ScanSelection::First),
            Intent::SetDayBefore(enabled) => {
                if state.day_before != enabled || state.day_before_stored != Some(enabled) {
                    state.day_before = enabled;
                    state.day_before_stored = Some(enabled);
                    state.day_before_dirty = true;
                }
            }
            Intent::StageDayBefore(enabled) => state.day_before = enabled,
            Intent::TakeFolderChange => {
                if std::mem::take(&mut state.folder_dirty) {
                    return Effects {
                        folder_change: Some(state.stored.clone()),
                        ..Effects::default()
                    };
                }
            }
            Intent::TakeDayBeforeChange => {
                if std::mem::take(&mut state.day_before_dirty) {
                    return Effects {
                        day_before_change: state.day_before_stored,
                        ..Effects::default()
                    };
                }
            }
            Intent::Select(index) => state.selected = Some(index),
            Intent::SelectDay(day) => {
                return Effects {
                    selected_day: Some(state.select_day(&day)),
                    ..Effects::default()
                };
            }
            Intent::SelectPath(path) => state.select_path(&path),
            Intent::AutostartSelection { speed, day, play } => {
                state.speed = speed;
                state.autoplay = play;
                if day.is_some_and(|day| !state.select_day(&day)) {
                    state.selected = None;
                    state.browser_open = true;
                } else if state.selected_entry().is_none() {
                    state.browser_open = true;
                } else {
                    return state.load_selected();
                }
            }
            Intent::LoadSelected => return state.load_selected(),
            Intent::EnterDownload { symbol, explicit } => {
                let entered = state.tab != BrowserTab::GetData;
                state.tab = BrowserTab::GetData;
                if entered || explicit {
                    let symbol = symbol
                        .or_else(|| (!explicit).then(|| state.market_current.clone()).flatten());
                    return Effects::local(LocalEffect::EnterDownload {
                        folder: state.folder.clone(),
                        symbol,
                        explicit,
                    });
                }
            }
            Intent::SelectSessions => state.tab = BrowserTab::Sessions,
            Intent::SetSpeed(speed) => state.speed = speed,
            Intent::SetAutoplay(autoplay) => state.autoplay = autoplay,
            Intent::SetShowFormat(show) => state.show_format = show,
            Intent::PickFolder => {
                if !state.picker_pending {
                    return Effects::local(LocalEffect::PickFolder {
                        folder: state.folder.trim().to_owned(),
                    });
                }
            }
            Intent::PickerCompleted(folder) => {
                state.picker_pending = false;
                if let Some(folder) = folder {
                    state.folder = folder;
                    return state.choose_folder();
                }
            }
            Intent::PickerStarted => state.picker_pending = true,
            Intent::LoadStarted | Intent::DownloadEntered => {}
            Intent::LoadCompleted(result) => {
                state.loading = None;
                match result {
                    Ok(()) => {
                        state.browser_open = false;
                        state.error = None;
                        return Effects {
                            open_loaded: Some((state.speed, state.autoplay)),
                            ..Effects::default()
                        };
                    }
                    Err(error) => state.error = Some(error),
                }
            }
            Intent::ScanCompleted(library) => {
                state.selected = (!library.sessions.is_empty()).then_some(0);
                state.library = Some(library);
                if let ScanSelection::Path(path) = std::mem::take(&mut state.scan_selection) {
                    state.select_path(&path);
                }
                state.error = None;
                state.rebuild_offerings();
            }
            Intent::Downloaded(path) => {
                state.tab = BrowserTab::Sessions;
                return state.scan(ScanSelection::Path(path));
            }
            Intent::DownloadRefreshed => {
                let selection = state
                    .selected_entry()
                    .map_or(ScanSelection::First, |entry| {
                        ScanSelection::Path(entry.path.clone())
                    });
                return state.scan(selection);
            }
        }
        Effects::default()
    }
}

#[cfg(test)]
#[path = "replay_browser/tests.rs"]
mod tests;
