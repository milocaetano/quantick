//! A second consumer of the public port, with its own indicator runtime.
use quantick_workspace::{
    indicator_document::{SavedIndicator, SavedKind},
    layout_document::{LayoutBook, LayoutId},
    session::{LayoutEffect, LayoutSession, LayoutView, PaneFacts},
};
use std::collections::BTreeMap;

struct Pane {
    view: LayoutView,
    indicators: Vec<SavedIndicator>,
}
struct Consumer {
    session: LayoutSession,
    panes: BTreeMap<u64, Pane>,
    effects: Vec<LayoutEffect>,
}
impl Consumer {
    fn select(&mut self, pane: u64, layout: LayoutId) {
        let selection = self
            .session
            .select(pane, layout, PaneFacts::default())
            .unwrap();
        let mut committed = None;
        for effect in selection.effects() {
            self.effects.push(*effect);
            let runtime = self.panes.get_mut(&pane).unwrap();
            match effect {
                LayoutEffect::DetachIndicators => runtime.indicators.clear(),
                LayoutEffect::CommitMembership => {
                    committed = Some(self.session.commit_selection(selection).unwrap())
                }
                LayoutEffect::MaterializeIndicators => {
                    runtime.indicators = self.session.book().get(layout).unwrap().indicators.clone()
                }
                LayoutEffect::CommitDefault => self
                    .session
                    .finish_selection(committed.take().unwrap())
                    .unwrap(),
                LayoutEffect::RefreshLabel | LayoutEffect::MarkDirty => {}
            }
        }
    }
}
#[test]
fn panes_share_definitions_and_switching_touches_only_indicators() {
    let indicator = SavedIndicator {
        kind: SavedKind::native("native.ema"),
        hidden: false,
        mouse_vertical_line: true,
        inputs: Vec::new(),
        plot_styles: Vec::new(),
    };
    let mut session = LayoutSession::new(LayoutBook::starter(vec![indicator.clone()]));
    let second = session.create(Some("levels")).unwrap();
    let mut panes = BTreeMap::new();
    for id in [1, 2] {
        let view = session.register(id, Some(LayoutId(1)));
        session.seed(id, None, None);
        panes.insert(
            id,
            Pane {
                view,
                indicators: vec![indicator.clone()],
            },
        );
    }
    let mut consumer = Consumer {
        session,
        panes,
        effects: Vec::new(),
    };
    consumer.select(1, second);
    assert_eq!(
        consumer.effects,
        vec![
            LayoutEffect::DetachIndicators,
            LayoutEffect::CommitMembership,
            LayoutEffect::MaterializeIndicators,
            LayoutEffect::RefreshLabel,
            LayoutEffect::CommitDefault,
            LayoutEffect::MarkDirty
        ]
    );
    assert_eq!(consumer.panes[&2].view.layout(), Some(LayoutId(1)));
    consumer.select(2, second);
    consumer.select(1, LayoutId(1));
    consumer.select(2, LayoutId(1));
    assert_eq!(consumer.panes[&1].indicators, consumer.panes[&2].indicators);
    assert!(consumer.panes[&1].indicators[0].mouse_vertical_line);
}
