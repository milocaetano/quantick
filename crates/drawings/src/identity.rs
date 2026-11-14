//! Identity promotion is metadata, never an undoable geometry edit.
use crate::{DrawingId, Drawings};
use quantick_workspace::drawing_identity::{DurableDrawingRef, IdentityUnavailable};

impl<T: Clone + PartialEq> Drawings<T> {
    /// Attach once, updating existing history so undo cannot strip a binding.
    /// Missing and duplicate runtime IDs are refused before any mutation.
    pub fn ensure_identity(
        &mut self,
        id: DrawingId,
        proposed: DurableDrawingRef,
    ) -> Result<DurableDrawingRef, IdentityUnavailable> {
        let mut matches = self.items.iter().filter(|item| item.id == id);
        let item = matches.next().ok_or(IdentityUnavailable::Missing)?;
        if matches.next().is_some() {
            return Err(IdentityUnavailable::Ambiguous);
        }
        if let Some(identity) = &item.durable_identity {
            return Ok(identity.clone());
        }
        let stamp = |items: &mut [crate::Drawing<T>]| {
            for item in items.iter_mut().filter(|item| item.id == id) {
                item.durable_identity = Some(proposed.clone());
            }
        };
        stamp(&mut self.items);
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            stamp(&mut entry.items);
        }
        if let Some(entry) = &mut self.gesture_baseline {
            stamp(&mut entry.items);
        }
        self.revision += 1;
        Ok(proposed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChartPoint, DRAWING_TOOLS, DrawingTool};

    #[test]
    fn alert_identity_lifecycle() {
        let mut drawings = Drawings::<DrawingTool>::default();
        let tool = DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "horizontal-line")
            .unwrap();
        assert!(drawings.place(tool, ChartPoint::at_time(0.0, 10.0, Some(1000))));
        let id = drawings.items()[0].id;
        drawings.rename_at(0, "Before binding");
        let identity = DurableDrawingRef {
            workspace: "workspace".into(),
            origin: "document".into(),
            pane: "market/flow".into(),
            drawing: id.0,
            incarnation: "unique".into(),
        };
        let stale_inspector = drawings.items()[0].clone();
        let revision = drawings.revision();
        assert_eq!(
            drawings.ensure_identity(id, identity.clone()),
            Ok(identity.clone())
        );
        assert!(drawings.revision() > revision);
        assert!(drawings.undo());
        assert_eq!(drawings.items()[0].durable_identity, Some(identity.clone()));
        assert!(drawings.redo());
        drawings.items_mut()[0].name = Some("Inspector edit".into());
        drawings.record_edit_of(0, stale_inspector);
        assert!(drawings.undo());
        assert_eq!(drawings.items()[0].durable_identity, Some(identity.clone()));
        let copy = drawings.items()[0].clone();
        let copied_id = drawings.paste(&copy, 1.0);
        assert_ne!(copied_id, id);
        assert!(drawings.items()[1].durable_identity.is_none());
        assert!(drawings.remove_by_id(id));
        assert_eq!(
            drawings.ensure_identity(id, identity.clone()),
            Err(IdentityUnavailable::Missing)
        );
        assert!(drawings.undo());
        assert_eq!(drawings.items()[0].durable_identity, Some(identity.clone()));
        let mut restart = Drawings::<DrawingTool>::default();
        restart.adopt(drawings.take_all());
        assert_eq!(restart.ensure_identity(id, identity.clone()), Ok(identity));
    }

    #[test]
    fn alert_identity_lifecycle_mirror_undo_and_ambiguity() {
        let mut drawings = Drawings::<DrawingTool>::default();
        let tool = DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "horizontal-line")
            .unwrap();
        assert!(drawings.place(tool, ChartPoint::at_time(0.0, 10.0, Some(1000))));
        let id = drawings.items()[0].id;
        let mut mirror = Drawings::<DrawingTool>::default();
        mirror.adopt(drawings.items().to_vec());
        mirror.rename_at(0, "Mirror edit");
        drawings.rename_at(0, "Mirror edit");
        let identity = DurableDrawingRef {
            workspace: "workspace".into(),
            origin: "document".into(),
            pane: "market/all".into(),
            drawing: id.0,
            incarnation: "unique".into(),
        };
        drawings.ensure_identity(id, identity.clone()).unwrap();
        mirror.replace_shared(drawings.items().to_vec());
        assert_eq!(mirror.items()[0].durable_identity, Some(identity.clone()));
        assert!(mirror.undo());
        assert_eq!(mirror.items()[0].durable_identity, Some(identity.clone()));
        assert!(mirror.redo());
        assert_eq!(mirror.items()[0].durable_identity, Some(identity.clone()));
        let copy = mirror.items()[0].clone();
        mirror.items.push(copy);
        let revision = mirror.revision();
        assert_eq!(
            mirror.ensure_identity(id, identity),
            Err(IdentityUnavailable::Ambiguous)
        );
        assert_eq!(mirror.revision(), revision);
    }
}
