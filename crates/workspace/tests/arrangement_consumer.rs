//! A second consumer uses only the public core/document contract, with no app.
use quantick_workspace::arrangement::{
    ArrangementLifecycle, Command, Effect, RestoreEffect, RestoreMode, TabFacts, TabId, Topology,
    Transition,
};
use quantick_workspace::workspace_document::Workspace;

struct Entry {
    id: TabId,
    symbol: String,
}
struct Facts<'a>(&'a [Entry]);
impl Topology for Facts<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn tab(&self, index: usize) -> Option<TabFacts<'_>> {
        self.0.get(index).map(|entry| TabFacts {
            id: entry.id,
            feed: "feed",
            symbol: &entry.symbol,
        })
    }
}
struct CliWorkspace {
    model: ArrangementLifecycle,
    entries: Vec<Entry>,
    events: Vec<String>,
}
impl CliWorkspace {
    fn apply(&mut self, plan: Transition, symbol: &str) {
        self.model
            .validate_transition(&plan, &Facts(&self.entries))
            .unwrap();
        match plan.effect() {
            Effect::Append { id } => {
                self.events.push(format!("open:{}", id.0));
                self.entries.push(Entry {
                    id,
                    symbol: symbol.to_owned(),
                });
            }
            Effect::Remove { index, id } => {
                self.events.push(format!("close:{}", id.0));
                self.entries.remove(index);
            }
            Effect::Select => self.events.push("select".to_owned()),
        }
        self.model.commit(plan, &Facts(&self.entries)).unwrap();
    }
}
#[test]
fn a_cli_host_restores_documents_using_the_same_ordered_core_decisions() {
    let mut host = CliWorkspace {
        model: ArrangementLifecycle::new(TabId(0)),
        entries: vec![Entry {
            id: TabId(0),
            symbol: "boot".to_owned(),
        }],
        events: Vec::new(),
    };
    let opening = host
        .model
        .plan(Command::Open, &Facts(&host.entries))
        .unwrap();
    host.apply(opening, "old");
    host.events.clear();
    let workspace: Workspace = toml::from_str(
        r#"
version = 1
[[saved]]
name = "CLI"
active_tab = 99
[[saved.tabs]]
feed = "feed"
symbol = "X"
layout = "flow"
flow_bars = "tick:50"
[[saved.tabs]]
feed = "feed"
symbol = "Y"
layout = "flow"
flow_bars = "tick:50"
"#,
    )
    .unwrap();
    assert_eq!(workspace.format_version(), 1);
    assert_eq!(workspace.saved[0].name, "CLI");
    let arrangement = &workspace.saved[0];
    let documents = &arrangement.tabs;
    host.model
        .begin_restore(
            RestoreMode::Named,
            documents.len(),
            documents
                .first()
                .map(|tab| (tab.feed.as_str(), tab.symbol.as_str())),
            arrangement.active_tab,
            &Facts(&host.entries),
        )
        .unwrap();
    loop {
        let step = host.model.restore_step(&Facts(&host.entries)).unwrap();
        let effect = step.effect();
        let symbol = match effect {
            RestoreEffect::OpenSaved { index, .. } => documents[index].symbol.as_str(),
            _ => "",
        };
        if let Some(plan) = host
            .model
            .plan_restore(&step, &Facts(&host.entries))
            .unwrap()
        {
            host.apply(plan, symbol);
        }
        match effect {
            RestoreEffect::ArrangeSaved { index, target } => host
                .events
                .push(format!("arrange:{}:{}", target.0, documents[index].symbol)),
            RestoreEffect::RestoreChrome => host.events.push("chrome".to_owned()),
            RestoreEffect::RefreshLabel => host.events.push("label".to_owned()),
            RestoreEffect::Finish => host.events.push("finish".to_owned()),
            RestoreEffect::AdoptSettings => {
                panic!("a named restore never adopts startup preferences")
            }
            _ => {}
        }
        host.model
            .advance_restore(step, &Facts(&host.entries))
            .unwrap();
        if effect == RestoreEffect::Finish {
            break;
        }
    }
    assert_eq!(
        host.events,
        [
            "open:2",
            "arrange:2:X",
            "open:3",
            "arrange:3:Y",
            "close:0",
            "close:1",
            "chrome",
            "select",
            "label",
            "finish"
        ]
    );
    assert_eq!(
        host.entries
            .iter()
            .map(|entry| (entry.id, entry.symbol.as_str()))
            .collect::<Vec<_>>(),
        [(TabId(2), "X"), (TabId(3), "Y")]
    );
    assert_eq!(host.model.active_id(), TabId(3));
    assert_eq!(host.model.active_index(), 1);
}
