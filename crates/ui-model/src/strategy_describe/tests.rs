use super::*;
use quantick_session::script_strategies::ScriptPlacement;

fn context() -> Arc<Context> {
    Arc::new(Context {
        workspace: quantick_workspace::lifetime::WorkspaceId(7),
        tab: 11,
        pane: 24,
        feed: "recorded".into(),
        symbol: "TEST".into(),
        bars: "renko:1".into(),
        placement: ScriptPlacement::Anywhere,
        indicators: vec![],
        drawings: vec![],
        availability: vec!["POC optional".into()],
        history: vec![],
    })
}
fn submit(state: &mut State, text: &str) -> Request {
    let revision = state.status.revision;
    let Effects::Request(request) = update(
        state,
        Intent::Command(Command::Submit {
            revision,
            text: text.into(),
        }),
    ) else {
        panic!("expected request")
    };
    request
}
fn candidate(state: &mut State, revision: u64) -> Draft {
    let draft = Draft {
        source: "script".into(),
        summary: "A tested alarm".into(),
        markers: vec![],
    };
    assert!(matches!(
        update(
            state,
            Intent::Replied {
                revision,
                reply: Ok(Reply::Candidate {
                    source: draft.source.clone(),
                    summary: draft.summary.clone()
                })
            }
        ),
        Effects::Preview { .. }
    ));
    draft
}
#[test]
fn clarification_retains_chart_context() {
    let chart = context();
    let mut state = State::default();
    update(&mut state, Intent::Opened(chart.clone()));
    let initial = submit(&mut state, "Alert on reversal");
    update(
        &mut state,
        Intent::Replied {
            revision: initial.revision,
            reply: Ok(Reply::Question("Both sides?".into())),
        },
    );
    let next = submit(&mut state, "Yes");
    assert!(Arc::ptr_eq(&initial.context, &next.context));
    assert!(Arc::ptr_eq(&chart, &next.context));
    assert_ne!(initial.revision, next.revision);
    assert_eq!(
        next.transcript,
        [
            Turn::Trader("Alert on reversal".into()),
            Turn::Assistant("Both sides?".into()),
            Turn::Trader("Yes".into())
        ]
    );
    assert_eq!(
        update(
            &mut state,
            Intent::Replied {
                revision: initial.revision,
                reply: Err("stale".into())
            }
        ),
        Effects::None
    );
    assert_eq!(state.status.phase, Phase::Waiting);
}
#[test]
fn compile_feedback_stops_after_three_repairs() {
    let mut state = State::default();
    update(&mut state, Intent::Opened(context()));
    let mut request = submit(&mut state, "Alert me");
    let problem = Diagnostic {
        line: Some(3),
        code: "PINE_TYPE".into(),
        message: "Invalid type".into(),
    };
    for attempt in 0..=MAX_REPAIRS {
        let draft = candidate(&mut state, request.revision);
        let effect = update(
            &mut state,
            Intent::Previewed {
                revision: request.revision,
                result: Err(vec![problem.clone()]),
            },
        );
        if attempt == MAX_REPAIRS {
            assert_eq!(effect, Effects::None);
            break;
        }
        let Effects::Request(next) = effect else {
            panic!("expected repair")
        };
        assert_eq!(next.repair, Some((draft.source, vec![problem.clone()])));
        assert!(Arc::ptr_eq(&request.context, &next.context));
        assert_eq!(next.transcript, request.transcript);
        request = next;
    }
    assert_eq!(state.status.phase, Phase::Failed);
    assert!(state.status.message.contains("PINE_TYPE"));
    assert!(state.status.draft.is_none());
}
#[test]
fn only_current_valid_drafts_are_saveable() {
    let mut state = State::default();
    update(&mut state, Intent::Opened(context()));
    let first = submit(&mut state, "alert");
    let draft = candidate(&mut state, first.revision);
    update(&mut state, Intent::TargetChanged);
    assert_eq!(
        update(
            &mut state,
            Intent::Previewed {
                revision: first.revision,
                result: Ok(draft.clone())
            }
        ),
        Effects::None
    );
    assert!(state.status.draft.is_none());
    update(&mut state, Intent::Opened(context()));
    let second = submit(&mut state, "alert again");
    let valid = candidate(&mut state, second.revision);
    update(
        &mut state,
        Intent::Previewed {
            revision: second.revision,
            result: Ok(valid.clone()),
        },
    );
    assert_eq!(state.status.draft, Some(valid));
    let pinned = state.context().unwrap();
    let mut live = pinned.clone();
    let trade = quantick_engine::Trade {
        agg_id: 4,
        timestamp_ms: 2000,
        price: 10.into(),
        quantity: 1.into(),
        side: quantick_engine::Side::Buy,
    };
    live.history.push(HistoricalClose {
        close: quantick_session::strategies::LiveClose {
            slot: 12,
            bar: quantick_engine::Bar::opened_by(&trade),
            poc: None,
        },
        region: None,
    });
    let matches = |chart: &Context| {
        pinned.matches_chart(
            chart.workspace,
            chart.tab,
            chart.pane,
            (&chart.feed, &chart.symbol),
            &chart.bars,
        )
    };
    assert!(matches(&live));
    assert!(pinned.history.is_empty());
    live.pane += 1;
    assert!(!matches(&live));
    let save = Command::Save {
        revision: second.revision,
        id: "mine.pine".into(),
    };
    assert!(matches!(
        update(&mut state, Intent::Command(save.clone())),
        Effects::Save { .. }
    ));
    assert_eq!(
        update(&mut state, Intent::Command(save.clone())),
        Effects::None
    );
    update(
        &mut state,
        Intent::Saved {
            revision: second.revision,
            result: Ok("mine.pine".into()),
        },
    );
    assert_eq!(update(&mut state, Intent::Command(save)), Effects::None);
    assert_eq!(state.status.saved_id.as_deref(), Some("mine.pine"));
    update(&mut state, Intent::Opened(context()));
    let third = submit(&mut state, "cancel");
    update(
        &mut state,
        Intent::Command(Command::Cancel {
            revision: third.revision,
        }),
    );
    update(
        &mut state,
        Intent::Replied {
            revision: third.revision,
            reply: Err("obsolete".into()),
        },
    );
    assert_eq!(state.status.phase, Phase::Cancelled);
    assert!(!state.status.message.contains("obsolete"));
}
