//! The sole in-flight workspace dialog and its terminal answer.
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspacePick {
    Export,
    Import,
}

pub(crate) enum PickerOutcome {
    Idle,
    Cancelled,
    Lost,
    Chosen {
        intent: WorkspacePick,
        path: PathBuf,
    },
}

#[derive(Default)]
pub(crate) struct WorkspacePickerHost {
    pending: Option<(WorkspacePick, Receiver<Option<PathBuf>>)>,
}

impl WorkspacePickerHost {
    /// Render the two controls that launch this owner's native chooser.
    pub(crate) fn show_open_actions(
        &mut self,
        ui: &mut eframe::egui::Ui,
        symbol: &str,
        spec: &crate::state::BarConfiguration,
    ) {
        if ui
            .button("Export to file…")
            .on_hover_text(
                "Save the whole cockpit — tabs, indicators, layers, drawing \
             colours, footprint and added symbols — as one file in your \
             documents",
            )
            .clicked()
        {
            self.open_export(symbol, spec);
            ui.close_menu();
        }
        if ui
            .button("Open from file…")
            .on_hover_text(
                "Open a workspace file. It replaces the cockpit on screen; a \
             file that cannot be read changes nothing.",
            )
            .clicked()
        {
            self.open_import();
            ui.close_menu();
        }
    }
    pub(crate) fn is_open(&self) -> bool {
        self.pending.is_some()
    }
    pub(crate) fn poll(&mut self) -> PickerOutcome {
        let Some((intent, receiver)) = &self.pending else {
            return PickerOutcome::Idle;
        };
        let outcome = match receiver.try_recv() {
            Err(TryRecvError::Empty) => return PickerOutcome::Idle,
            Err(TryRecvError::Disconnected) => PickerOutcome::Lost,
            Ok(None) => PickerOutcome::Cancelled,
            Ok(Some(path)) => PickerOutcome::Chosen {
                intent: *intent,
                path,
            },
        };
        self.pending = None;
        outcome
    }
    pub(crate) fn open_export(&mut self, symbol: &str, spec: &crate::state::BarConfiguration) {
        if self.is_open() {
            return;
        }
        let suggested = format!("{} {}", symbol, spec.to_config_string());
        self.open(WorkspacePick::Export, Some(&suggested));
    }
    pub(crate) fn open_import(&mut self) {
        self.open(WorkspacePick::Import, None);
    }
    fn open(&mut self, intent: WorkspacePick, suggested: Option<&str>) {
        if self.is_open() {
            return;
        }
        let start = crate::workspace_bundle::default_dir();
        let suggested = suggested.map(crate::workspace_bundle::file_name_for);
        let (title, thread, failure) = match intent {
            WorkspacePick::Export => (
                "Export workspace",
                "quantick-workspace-export-picker",
                "spawn workspace export picker thread",
            ),
            WorkspacePick::Import => (
                "Open workspace",
                "quantick-workspace-import-picker",
                "spawn workspace import picker thread",
            ),
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name(thread.into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new()
                    .set_title(title)
                    .add_filter("quantick workspace", &["toml"]);
                if let Some(suggested) = suggested {
                    dialog = dialog.set_file_name(suggested);
                }
                if let Some(start) = start {
                    match intent {
                        WorkspacePick::Export => {
                            let _ = std::fs::create_dir_all(&start);
                            dialog = dialog.set_directory(&start);
                        }
                        WorkspacePick::Import if start.is_dir() => {
                            dialog = dialog.set_directory(&start)
                        }
                        WorkspacePick::Import => {}
                    }
                }
                let choice = match intent {
                    WorkspacePick::Export => dialog.save_file(),
                    WorkspacePick::Import => dialog.pick_file(),
                };
                let _ = sender.send(choice);
            })
            .expect(failure);
        self.pending = Some((intent, receiver));
    }
}

#[cfg(test)]
impl WorkspacePickerHost {
    pub(crate) fn set_pending_for_test(
        &mut self,
        intent: WorkspacePick,
        receiver: Receiver<Option<PathBuf>>,
    ) {
        self.pending = Some((intent, receiver));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_answer_clears_before_the_caller_can_dispatch() {
        for choice in [None, Some(PathBuf::from("chosen.qws.toml"))] {
            let mut host = WorkspacePickerHost::default();
            let (sender, receiver) = std::sync::mpsc::channel();
            host.set_pending_for_test(WorkspacePick::Import, receiver);
            sender.send(choice.clone()).unwrap();
            let outcome = host.poll();
            assert!(!host.is_open());
            match (outcome, choice) {
                (PickerOutcome::Cancelled, None) => {}
                (
                    PickerOutcome::Chosen {
                        intent: WorkspacePick::Import,
                        path,
                    },
                    Some(expected),
                ) => assert_eq!(path, expected),
                _ => panic!("the terminal outcome preserves the original answer"),
            }
            assert!(matches!(host.poll(), PickerOutcome::Idle));
        }
        let mut host = WorkspacePickerHost::default();
        let (sender, receiver) = std::sync::mpsc::channel();
        host.set_pending_for_test(WorkspacePick::Export, receiver);
        drop(sender);
        assert!(matches!(host.poll(), PickerOutcome::Lost));
        assert!(!host.is_open());
    }
}
