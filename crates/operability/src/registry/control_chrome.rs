use super::{ExclusionClass, Mapping, Source, UiBehaviour};

pub(super) const BINDING: (usize, &[UiBehaviour]) = (13, ROWS);
const ROWS: &[UiBehaviour] = &[
    UiBehaviour {
        id: "window.menu.open",
        title: "Open the window menu",
        reach: "the kebab button on the title strip, F10",
        keys: &[(Source::Hotkey, "WINDOW_MENU_SHORTCUT")],
        mapping: excluded!(
            UiOnlyByDecision,
            "the menu holds rows, and every row names its own behaviour; an operator calls \
             those directly, and a transient popup is not state worth driving"
        ),
    },
    UiBehaviour {
        id: "settings.open",
        title: "Open the Settings window on a section",
        reach: "window menu, Ctrl+Comma, the status-bar clock; the Agent on pill opens the \
                section a call is refused",
        keys: &[
            (Source::Hotkey, "SETTINGS_SHORTCUT"),
            (Source::MenuEntry, "Settings…"),
        ],
        mapping: Mapping::Capabilities(&["settings.open"]),
    },
    UiBehaviour {
        id: "window.help.open",
        title: "Read a help page: keyboard shortcuts, workspaces, the version",
        reach: "window menu, Help",
        keys: &[
            (Source::MenuEntry, "Keyboard shortcuts…"),
            (Source::MenuEntry, "About workspaces…"),
            (Source::MenuEntry, "About quantick"),
        ],
        mapping: excluded!(
            UiOnlyByDecision,
            "reference text for a human; the shortcuts and the workspace rules are the \
             contract an operator already reads"
        ),
    },
    UiBehaviour {
        id: "window.command_palette.open",
        title: "Open the command palette",
        reach: "window menu, disabled until the market palette ships",
        keys: &[(Source::MenuEntry, "Command palette…")],
        mapping: excluded!(
            UiOnlyByDecision,
            "not built: the row is greyed out and says so, and nothing runs behind it"
        ),
    },
    UiBehaviour {
        id: "control.access.open",
        title: "Open the local agent access panel",
        reach: "Settings, Agent access, Manage clients…",
        keys: &[
            (Source::MenuEntry, "Manage clients…"),
            (Source::MenuEntry, "Local agent access"),
        ],
        mapping: excluded!(
            Authority,
            "the panel is where the trader grants and revokes an operator's access. A \
             capability that opened it would let a connection widen its own authority, which \
             `docs/control-plane/observer-threat-model.md` exists to prevent"
        ),
    },
    UiBehaviour {
        id: "view.perf_readings.toggle",
        title: "Show fps, frame time and trade count on the status bar",
        reach: "Settings window",
        keys: &[(Source::MenuEntry, "Perf readings")],
        mapping: capability!("settings.set"),
    },
    UiBehaviour {
        id: "view.timezone.set",
        title: "Change the timezone the time axis is labelled in",
        reach: "Settings window, Timezone",
        keys: &[(Source::MenuEntry, "Timezone")],
        mapping: capability!("settings.set"),
    },
];
