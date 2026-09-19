//! A second consumer of the public port, with its own drawing/indicator runtime.
use quantick_workspace::{
    indicator_document::{SavedIndicator, SavedKind},
    layout_document::{DrawingKey, LayoutBook, LayoutId, SavedBand, SavedDrawing, SavedPoint},
    session::{LayoutEffect, LayoutSession, LayoutView, PaneFacts},
};
use std::collections::BTreeMap;

struct Pane {
    view: LayoutView,
    market: DrawingKey,
    drawings: Vec<SavedDrawing>,
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
                LayoutEffect::StoreOutgoingDrawings => self.session.store_drawings(
                    selection.from(),
                    &runtime.market,
                    runtime.drawings.clone(),
                    |tool| tool == "level",
                ),
                LayoutEffect::DetachIndicators => runtime.indicators.clear(),
                LayoutEffect::CommitMembership => {
                    committed = Some(self.session.commit_selection(selection).unwrap())
                }
                LayoutEffect::MaterializeIndicators => {
                    runtime.indicators = self.session.book().get(layout).unwrap().indicators.clone()
                }
                LayoutEffect::RestoreDrawings => {
                    runtime.drawings = self
                        .session
                        .book()
                        .get(layout)
                        .unwrap()
                        .drawings(&runtime.market)
                        .unwrap_or_default()
                        .to_vec()
                }
                LayoutEffect::CommitDefault => self
                    .session
                    .finish_selection(committed.take().unwrap())
                    .unwrap(),
                LayoutEffect::LeaveGestures
                | LayoutEffect::PersistChangedDrawings
                | LayoutEffect::RefreshLabel
                | LayoutEffect::MarkDirty => {}
            }
        }
    }
}
fn level(price: f64) -> SavedDrawing {
    SavedDrawing {
        id: Some(7),
        tool: "level".into(),
        name: None,
        author: None,
        points: vec![SavedPoint {
            time_ms: Some(100),
            price,
            bar: 0.0,
        }],
        band: SavedBand::Price,
        color: [1, 2, 3, 255],
        width_px: 1.0,
        fill_alpha: 0,
        locked: false,
        hidden: false,
        shared: false,
        payload: None,
        text: None,
    }
}
#[test]
fn two_markets_share_definitions_but_keep_separate_drawings_through_the_same_effect_port() {
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
    for (id, symbol, price) in [(1, "BTC", 100.0), (2, "ETH", 20.0)] {
        let view = session.register(id, Some(LayoutId(1)));
        session.seed(id, None, None);
        panes.insert(
            id,
            Pane {
                view,
                market: DrawingKey {
                    feed: "fixture".into(),
                    symbol: symbol.into(),
                    pane: 0,
                },
                drawings: vec![level(price)],
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
            LayoutEffect::LeaveGestures,
            LayoutEffect::PersistChangedDrawings,
            LayoutEffect::StoreOutgoingDrawings,
            LayoutEffect::DetachIndicators,
            LayoutEffect::CommitMembership,
            LayoutEffect::MaterializeIndicators,
            LayoutEffect::RestoreDrawings,
            LayoutEffect::RefreshLabel,
            LayoutEffect::CommitDefault,
            LayoutEffect::MarkDirty
        ]
    );
    assert_eq!(consumer.panes[&2].view.layout(), Some(LayoutId(1)));
    assert_eq!(consumer.panes[&2].drawings[0].points[0].price, 20.0);
    consumer.select(2, second);
    consumer.select(1, LayoutId(1));
    consumer.select(2, LayoutId(1));
    assert_eq!(consumer.panes[&1].drawings[0].points[0].price, 100.0);
    assert_eq!(consumer.panes[&2].drawings[0].points[0].price, 20.0);
    assert_eq!(consumer.panes[&1].indicators, consumer.panes[&2].indicators);
    assert!(consumer.panes[&1].indicators[0].mouse_vertical_line);
}
