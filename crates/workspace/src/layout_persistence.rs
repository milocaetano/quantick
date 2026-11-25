//! Reserve allocated IDs without saving pending layout names or configuration.
use super::LayoutBook;

impl LayoutBook {
    pub(crate) fn has_reserved_identities(&self, live: &Self) -> bool {
        self.next_id >= live.next_id
    }

    pub(crate) fn with_reserved_identities(&self, live: &Self) -> Self {
        let mut document = self.clone();
        document.next_id = document.next_id.max(live.next_id);
        document
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{layout_document::parse, layout_store::LayoutStore};
    use std::path::PathBuf;

    #[test]
    fn manual_projection_reserves_ids_but_only_explicit_save_commits_layout_configuration() {
        let mut store = LayoutStore::new(LayoutBook::default(), PathBuf::new(), false);
        assert!(store.identities_reserved());
        let original = store.book().active_id();
        store
            .session_mut()
            .rename(original, "Pending rename")
            .unwrap();
        assert!(
            store.identities_reserved(),
            "renames do not change identity"
        );
        let new = store.session_mut().create(Some("Second")).unwrap();
        let session = store.session_mut();
        session.register(1, Some(original));
        let selection = session.select(1, new, Default::default()).unwrap();
        let committed = session.commit_selection(selection).unwrap();
        session.finish_selection(committed).unwrap();
        assert_eq!(store.book().active_id(), new);
        assert!(!store.identities_reserved());
        let projected = store.persistence_document();
        assert_eq!(projected.active_id(), original);
        assert_eq!(projected.layouts().len(), 1);
        assert_eq!(projected.get(original).unwrap().name, "Layout 1");
        assert!(projected.get(new).is_none());
        let mut reloaded = parse(&toml::to_string(&projected).unwrap()).unwrap();
        assert!(reloaded.by_name("Second").is_none());
        assert!(reloaded.create(None).unwrap().0 > new.0);
        store.note_saved(projected);
        assert!(store.identities_reserved());
        assert!(store.configuration_changed());
        store.note_saved(store.commit_document());
        assert!(!store.configuration_changed());
        let committed = store.persistence_document();
        assert_eq!(committed.active_id(), new);
        assert_eq!(committed.layouts().len(), 2);
        assert_eq!(committed.get(new).unwrap().name, "Second");
        assert_eq!(committed.get(original).unwrap().name, "Pending rename");
    }

    #[test]
    fn private_indicator_copies_reserve_ids_without_becoming_named_layouts() {
        let saved = LayoutBook::default();
        let mut live = saved.clone();
        let copy = live.make_private_copy(live.active_id()).unwrap();
        assert_eq!(live.strip_layout(copy), saved.active_id());
        let mut reopened =
            parse(&toml::to_string(&saved.with_reserved_identities(&live)).unwrap()).unwrap();
        assert_eq!(reopened.layouts().len(), 1);
        assert!(reopened.get(copy).is_none());
        assert!(reopened.create(None).unwrap().0 > copy.0);
    }
}
