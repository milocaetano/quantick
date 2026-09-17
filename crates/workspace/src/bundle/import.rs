use super::{Bundle, BundleStore, FORMAT_VERSION, merge_local_keys};
use std::collections::VecDeque;

/// Private progress; only consuming typed replies can reach an install receipt.
pub struct ImportTransaction<'bundle, 'stores, S: BundleStore> {
    bundle: &'bundle Bundle,
    stores: &'stores [S],
    selected: Vec<usize>,
    unknown: std::vec::IntoIter<&'bundle str>,
    staged: usize,
    installed: usize,
    cleanup: VecDeque<usize>,
    failure: Option<ImportFailure>,
}
pub enum ImportStep<'bundle, 'stores, S: BundleStore> {
    Notice(UnknownSection<'bundle, 'stores, S>),
    Prepare(PrepareStore<'bundle, 'stores, S>),
    Stage(StageStore<'bundle, 'stores, S>),
    Install(InstallStore<'bundle, 'stores, S>),
    Cleanup(CleanupTemp<'bundle, 'stores, S>),
    Finished(Result<InstalledStores<'stores>, ImportFailure>),
}
pub enum CurrentText {
    Available(String),
    Unavailable,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImportFailure {
    Version(u32),
    InvalidSection {
        store_index: usize,
        message: String,
    },
    RenderSection {
        store_index: usize,
        message: String,
    },
    Stage {
        store_index: usize,
        message: String,
    },
    Install {
        store_index: usize,
        installed: usize,
        total: usize,
        message: String,
    },
}

/// Keys borrow only the registry, never the source bundle.
///
/// ```compile_fail
/// use quantick_workspace::bundle::InstalledStores;
/// let receipt = InstalledStores { keys: vec!["forged"] };
/// ```
#[derive(Debug)]
pub struct InstalledStores<'stores> {
    keys: Vec<&'stores str>,
}
impl<'stores> InstalledStores<'stores> {
    pub fn keys(&self) -> &[&'stores str] {
        &self.keys
    }
}
impl<'stores> std::ops::Deref for InstalledStores<'stores> {
    type Target = [&'stores str];
    fn deref(&self) -> &Self::Target {
        self.keys()
    }
}

pub struct UnknownSection<'bundle, 'stores, S: BundleStore> {
    transaction: ImportTransaction<'bundle, 'stores, S>,
    key: &'bundle str,
}
pub struct PrepareStore<'bundle, 'stores, S: BundleStore> {
    transaction: ImportTransaction<'bundle, 'stores, S>,
    index: usize,
}
/// A stage reply is consumed exactly once.
///
/// ```compile_fail
/// use quantick_workspace::bundle::{BundleStore, StageStore};
/// fn twice<S: BundleStore>(step: StageStore<'_, '_, S>) {
///     step.complete(Ok(()));
///     step.complete(Ok(()));
/// }
/// ```
pub struct StageStore<'bundle, 'stores, S: BundleStore> {
    transaction: ImportTransaction<'bundle, 'stores, S>,
    index: usize,
    text: String,
}
pub struct InstallStore<'bundle, 'stores, S: BundleStore> {
    transaction: ImportTransaction<'bundle, 'stores, S>,
    index: usize,
}
pub struct CleanupTemp<'bundle, 'stores, S: BundleStore> {
    transaction: ImportTransaction<'bundle, 'stores, S>,
    index: usize,
}

impl<'bundle, 'stores, S: BundleStore> ImportTransaction<'bundle, 'stores, S> {
    pub fn begin(bundle: &'bundle Bundle, stores: &'stores [S]) -> ImportStep<'bundle, 'stores, S> {
        if bundle.version != FORMAT_VERSION {
            return ImportStep::Finished(Err(ImportFailure::Version(bundle.version)));
        }
        let selected = stores
            .iter()
            .enumerate()
            .filter(|(_, store)| store.included() && bundle.sections.contains_key(store.key()))
            .map(|(index, _)| index)
            .collect();
        let unknown = bundle
            .sections
            .keys()
            .filter(|key| !stores.iter().any(|store| store.key() == *key))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .into_iter();
        Self {
            bundle,
            stores,
            selected,
            unknown,
            staged: 0,
            installed: 0,
            cleanup: VecDeque::new(),
            failure: None,
        }
        .notice_or_validate()
    }
    fn notice_or_validate(mut self) -> ImportStep<'bundle, 'stores, S> {
        if let Some(key) = self.unknown.next() {
            return ImportStep::Notice(UnknownSection {
                transaction: self,
                key,
            });
        }
        for &index in &self.selected {
            let store = &self.stores[index];
            let text = match toml::to_string_pretty(&self.bundle.sections[store.key()]) {
                Ok(text) => text,
                Err(error) => {
                    return ImportStep::Finished(Err(ImportFailure::RenderSection {
                        store_index: index,
                        message: error.to_string(),
                    }));
                }
            };
            if let Err(message) = store.validate_text(&text) {
                return ImportStep::Finished(Err(ImportFailure::InvalidSection {
                    store_index: index,
                    message,
                }));
            }
        }
        self.prepare()
    }
    fn prepare(self) -> ImportStep<'bundle, 'stores, S> {
        if let Some(&index) = self.selected.get(self.staged) {
            ImportStep::Prepare(PrepareStore {
                transaction: self,
                index,
            })
        } else {
            self.install()
        }
    }
    fn install(self) -> ImportStep<'bundle, 'stores, S> {
        if let Some(&index) = self.selected.get(self.installed) {
            ImportStep::Install(InstallStore {
                transaction: self,
                index,
            })
        } else {
            ImportStep::Finished(Ok(InstalledStores {
                keys: self
                    .selected
                    .iter()
                    .map(|&index| self.stores[index].key())
                    .collect(),
            }))
        }
    }
    fn fail_before_install(mut self, failure: ImportFailure) -> ImportStep<'bundle, 'stores, S> {
        self.cleanup
            .extend(self.selected.iter().take(self.staged).copied());
        self.failure = Some(failure);
        self.cleanup()
    }
    fn cleanup(mut self) -> ImportStep<'bundle, 'stores, S> {
        if let Some(index) = self.cleanup.pop_front() {
            ImportStep::Cleanup(CleanupTemp {
                transaction: self,
                index,
            })
        } else {
            ImportStep::Finished(Err(self
                .failure
                .take()
                .expect("cleanup retains its terminal failure")))
        }
    }
}
impl<'bundle, 'stores, S: BundleStore> UnknownSection<'bundle, 'stores, S> {
    pub fn key(&self) -> &str {
        self.key
    }
    pub fn acknowledge(self) -> ImportStep<'bundle, 'stores, S> {
        self.transaction.notice_or_validate()
    }
}
impl<'bundle, 'stores, S: BundleStore> PrepareStore<'bundle, 'stores, S> {
    pub fn store_index(&self) -> usize {
        self.index
    }
    pub fn needs_current_text(&self) -> bool {
        !self.transaction.stores[self.index].local_keys().is_empty()
    }
    pub fn complete(self, current: CurrentText) -> ImportStep<'bundle, 'stores, S> {
        let store = &self.transaction.stores[self.index];
        match merge_local_keys(
            &self.transaction.bundle.sections[store.key()],
            current,
            store.local_keys(),
        ) {
            Ok(text) => ImportStep::Stage(StageStore {
                transaction: self.transaction,
                index: self.index,
                text,
            }),
            Err(message) => self
                .transaction
                .fail_before_install(ImportFailure::RenderSection {
                    store_index: self.index,
                    message,
                }),
        }
    }
}
impl<'bundle, 'stores, S: BundleStore> StageStore<'bundle, 'stores, S> {
    pub fn store_index(&self) -> usize {
        self.index
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn complete(mut self, result: Result<(), String>) -> ImportStep<'bundle, 'stores, S> {
        match result {
            Ok(()) => {
                self.transaction.staged += 1;
                self.transaction.prepare()
            }
            Err(message) => self.transaction.fail_before_install(ImportFailure::Stage {
                store_index: self.index,
                message,
            }),
        }
    }
}
impl<'bundle, 'stores, S: BundleStore> InstallStore<'bundle, 'stores, S> {
    pub fn store_index(&self) -> usize {
        self.index
    }
    pub fn complete(mut self, result: Result<(), String>) -> ImportStep<'bundle, 'stores, S> {
        match result {
            Ok(()) => {
                self.transaction.installed += 1;
                self.transaction.install()
            }
            Err(message) => {
                self.transaction.failure = Some(ImportFailure::Install {
                    store_index: self.index,
                    installed: self.transaction.installed,
                    total: self.transaction.selected.len(),
                    message,
                });
                self.transaction.cleanup.push_back(self.index);
                self.transaction.cleanup()
            }
        }
    }
}
impl<'bundle, 'stores, S: BundleStore> CleanupTemp<'bundle, 'stores, S> {
    pub fn store_index(&self) -> usize {
        self.index
    }
    pub fn complete(self) -> ImportStep<'bundle, 'stores, S> {
        self.transaction.cleanup()
    }
}
