use super::{Bundle, BundleStore, FORMAT_VERSION, strip_local_keys};
use std::collections::BTreeMap;

/// Capture stops on the first read/parse failure, before exposing the next store.
pub struct CaptureTransaction<'stores, S: BundleStore> {
    stores: &'stores [S],
    next: usize,
    bundle: Bundle,
}
pub enum CaptureStep<'stores, S: BundleStore> {
    Read(CaptureRead<'stores, S>),
    Finished(Result<Bundle, CaptureFailure>),
}
pub struct CaptureRead<'stores, S: BundleStore> {
    transaction: CaptureTransaction<'stores, S>,
    index: usize,
}
pub enum CaptureText {
    Missing,
    Text(String),
    Failed(String),
}
#[derive(Debug, PartialEq, Eq)]
pub enum CaptureFailure {
    Read { store_index: usize, message: String },
    Parse { store_index: usize, message: String },
}
impl<'stores, S: BundleStore> CaptureTransaction<'stores, S> {
    pub fn begin(name: &str, stores: &'stores [S]) -> CaptureStep<'stores, S> {
        Self {
            stores,
            next: 0,
            bundle: Bundle {
                version: FORMAT_VERSION,
                name: name.to_owned(),
                sections: BTreeMap::new(),
            },
        }
        .advance()
    }
    fn advance(mut self) -> CaptureStep<'stores, S> {
        while self.next < self.stores.len() {
            let index = self.next;
            self.next += 1;
            if self.stores[index].included() {
                return CaptureStep::Read(CaptureRead {
                    transaction: self,
                    index,
                });
            }
        }
        CaptureStep::Finished(Ok(self.bundle))
    }
}
impl<'stores, S: BundleStore> CaptureRead<'stores, S> {
    pub fn store_index(&self) -> usize {
        self.index
    }
    pub fn complete(mut self, result: CaptureText) -> CaptureStep<'stores, S> {
        match result {
            CaptureText::Missing => {}
            CaptureText::Failed(message) => {
                return CaptureStep::Finished(Err(CaptureFailure::Read {
                    store_index: self.index,
                    message,
                }));
            }
            CaptureText::Text(text) => {
                let mut value = match toml::from_str(&text) {
                    Ok(value) => value,
                    Err(error) => {
                        return CaptureStep::Finished(Err(CaptureFailure::Parse {
                            store_index: self.index,
                            message: error.to_string(),
                        }));
                    }
                };
                let store = &self.transaction.stores[self.index];
                strip_local_keys(&mut value, store.local_keys());
                self.transaction
                    .bundle
                    .sections
                    .insert(store.key().to_owned(), value);
            }
        }
        self.transaction.advance()
    }
}
