use quantick_alerts::*;
use quantick_workspace::drawing_identity::{IdentityUnavailable, resolve};
use quantick_workspace::layout_document::SavedDrawing;

#[derive(Default)]
struct FakeConsumer {
    delivered: usize,
}

impl AlertEventSink for FakeConsumer {
    fn try_push(&mut self, _event: AlertEvent) -> bool {
        self.delivered += 1;
        true
    }
}

#[test]
fn contract_second_consumer() {
    let mut book = AlertBook::default();
    let market = MarketRef {
        source: SourceContext::Live {
            feed: "fixture".into(),
        },
        symbol: "TEST".into(),
        aggregation: "tick:2".into(),
    };
    let input = AlertInput {
        market: &market,
        utc_ms: 100,
        session_ms: 100,
        bar: BarIdentity {
            epoch: 1,
            sequence: 0,
        },
        kind: ObservationKind::CommittedClose,
        coordinate: 0.0,
        price: 10.0,
        availability: Ok(()),
        drawings: &[],
    };
    let mut fake = FakeConsumer::default();
    let mut bounded = BoundedEvents::new(1);
    book.evaluate(&input, &mut fake);
    book.evaluate(&input, &mut bounded);
    assert_eq!(fake.delivered, 0);
    assert!(bounded.is_empty());
    assert_eq!(
        book.command(AlertCommand::ClearHistory, 100),
        Err(AlertError::Unavailable)
    );
    // The sound is the same public value consumed by the original audio sink.
    let cue = sound::Cue::whole(sound::AlertSound::Information);
    assert_eq!(cue.sound.token(), "information");
}

#[test]
fn durable_target_contract() {
    let first = DurableDrawingRef {
        workspace: "desk".into(),
        origin: "document-incarnation-1".into(),
        pane: "flow".into(),
        drawing: 1,
        incarnation: "mark-incarnation-1".into(),
    };
    let second = DurableDrawingRef {
        pane: "context:time:1m".into(),
        ..first.clone()
    };
    let encoded = toml::to_string(&TargetRef::Drawing(second.clone())).unwrap();
    let decoded: TargetRef = toml::from_str(&encoded).unwrap();
    assert_eq!(decoded, TargetRef::Drawing(second.clone()));
    assert_eq!(resolve(&second, [&first, &second]), Ok(&second));
    assert_eq!(
        resolve(&second, [&first]),
        Err(IdentityUnavailable::Missing)
    );
    assert_eq!(
        resolve(&second, [&second, &second]),
        Err(IdentityUnavailable::Ambiguous)
    );
    let legacy = r#"
id = 1
tool = "trendline"
points = []
band = { axis = "price" }
color = [1, 2, 3, 255]
width_px = 1.0
fill_alpha = 0
"#;
    let mut drawing: SavedDrawing = toml::from_str(legacy).unwrap();
    assert_eq!(drawing.durable_identity, None);
    assert_eq!(
        resolve(&first, drawing.durable_identity.iter()),
        Err(IdentityUnavailable::Missing)
    );
    drawing.durable_identity = Some(first);
    let roundtrip: SavedDrawing = toml::from_str(&toml::to_string(&drawing).unwrap()).unwrap();
    assert_eq!(roundtrip, drawing);
}
