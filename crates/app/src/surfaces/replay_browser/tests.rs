#[cfg(test)]
mod lifecycle {
    use super::super::*;
    use crate::surfaces::View as _;
    use quantick_feed::replay_download::{
        DownloadError, DownloadEvent, DownloadJob, DownloadRequest, SessionSource,
    };
    use quantick_feed::{FeedCommand, ReplayControl};
    use quantick_ui_model::{Slot, replay_browser::BrowserTab};
    use std::{
        path::Path,
        sync::Mutex,
        time::{Duration, Instant},
    };

    fn csv(root: &Path, symbol: &str, day: &str) -> PathBuf {
        let folder = root.join(symbol);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(format!("{}.csv", day.replace('-', "")));
        std::fs::write(&path, format!("# quantick-replay 1\n# symbol={symbol}\n# timezone=+00:00\nDate,Time,Price,Volume,Side\n{day},10:00:00.000,100,1,B\n{day},11:00:00.000,101,2,S\n")).unwrap();
        path
    }
    fn browser(root: &Path) -> ReplayBrowser {
        let mut browser = ReplayBrowser::new_browser(None, Some(false));
        browser.dispatch(Intent::SetFolderDraft(root.display().to_string()));
        browser
    }
    fn bind(browser: &mut ReplayBrowser, link: Option<&ReplayLink>) {
        browser.bind_pass(
            link,
            &MarketMenu {
                current: Some("WINJ26"),
                catalogue: vec!["WINJ26"],
            },
        );
    }
    fn frame(
        browser: &mut ReplayBrowser,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        focused: bool,
    ) -> (SurfaceResponse, egui::FullOutput, egui::Rect) {
        let mut response = SurfaceResponse::default();
        let mut canvas = egui::Rect::NOTHING;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                time: Some(ctx.input(|input| input.time) + 1.0 / 60.0),
                events,
                ..Default::default()
            },
            |ctx| {
                if focused {
                    egui::Window::new("Text focus").show(ctx, |ui| {
                        ui.text_edit_singleline(&mut String::from("draft"))
                            .request_focus();
                    });
                }
                browser
                    .with_pass(
                        Slot::EarlyEdgePanels,
                        |effect| response.merge(effect),
                        |view, session| {
                            view.paint_updated(session, &mut ViewContext { ctx, ui: None }, None);
                        },
                    )
                    .expect("registered early pass");
                egui::CentralPanel::default()
                    .frame(egui::Frame::none())
                    .show(ctx, |ui| canvas = ui.max_rect());
            },
        );
        (response, output, canvas)
    }
    fn key(pressed: bool) -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Space,
            physical_key: Some(egui::Key::Space),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }
    fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    fn text_position(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|clipped| {
            if let egui::Shape::Text(text) = &clipped.shape
                && text.galley.job.text == needle
            {
                Some(
                    text.galley
                        .rect
                        .translate(text.pos.to_vec2())
                        .intersect(clipped.clip_rect)
                        .center(),
                )
            } else {
                None
            }
        })
    }
    fn wait_open(browser: &mut ReplayBrowser, ctx: &egui::Context) -> Box<ReplayRequest> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let (response, _, _) = frame(browser, ctx, vec![], false);
            if let Some(ReplayAction::Open(request)) = take_action(browser, response) {
                return request;
            }
            assert!(
                Instant::now() < deadline,
                "actual CSV loader must complete: {:?}",
                browser.model_state.error
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn replay_real_loader_delivers_one_open_and_clears_wait() {
        let root = crate::scratch::ScratchDir::new("registered-replay-loader");
        csv(&root, "FIRST", "2026-03-13");
        csv(&root, "SECOND", "2026-03-16");
        let mut browser = browser(&root);
        let ctx = egui::Context::default();
        browser.open_browser();
        browser.dispatch(Intent::Select(0));
        assert!(browser.dispatch(Intent::LoadSelected).load_started);
        assert!(browser.is_loading());
        // Hold an unfinished receiver while a real double-click chooses another
        // CSV. This exercises the list even though loading hides Play session.
        let (abandoned_sender, receiver) = std::sync::mpsc::channel();
        browser.view.io.loading = Some(receiver);
        frame(&mut browser, &ctx, vec![], false);
        let (_, output, _) = frame(&mut browser, &ctx, vec![], false);
        assert!(text_position(&output, "Play session").is_none());
        let entry = &browser.model_state.library.as_ref().unwrap().sessions[1];
        let row = format!("{}      {}", entry.label(), entry.size_label());
        let position =
            text_position(&output, &row).expect("second recording remains clickable while loading");
        for _ in 0..2 {
            for pressed in [true, false] {
                frame(&mut browser, &ctx, pointer(position, pressed), false);
            }
        }
        assert_eq!(browser.model_state.selected, Some(1));
        assert!(
            abandoned_sender
                .send(Ok(Session::load(
                    &root.join("FIRST/20260313.csv"),
                    ParseOptions::default()
                )
                .unwrap()))
                .is_err(),
            "the double-click replaces and drops the abandoned receiver"
        );
        assert!(
            browser.is_loading(),
            "the replacement starts the real CSV worker"
        );
        browser.dispatch(Intent::SetSpeed(4.0));
        browser.dispatch(Intent::SetAutoplay(true));
        let loaded = wait_open(&mut browser, &ctx);
        assert_eq!(
            loaded.session.symbol, "SECOND",
            "an abandoned first receiver cannot publish stale data"
        );
        assert_eq!(loaded.options.speed, 4.0);
        assert!(loaded.options.autoplay);
        assert!(!browser.is_loading());
        assert!(!browser.browser_is_open());
        assert!(browser.view.io.loading.is_none());
        assert!(
            browser.view.io.loaded.is_none(),
            "Open consumes the Session payload"
        );
        for _ in 0..3 {
            let (response, _, _) = frame(&mut browser, &ctx, vec![], false);
            assert!(
                take_action(&mut browser, response).is_none(),
                "one load produces one Open"
            );
        }
        // Real receiver disconnect clears the wait and exposes the original advice.
        browser.open_browser();
        browser.dispatch(Intent::LoadSelected);
        let (sender, receiver) = std::sync::mpsc::channel();
        browser.view.io.loading = Some(receiver);
        drop(sender);
        let (response, _, _) = frame(&mut browser, &ctx, vec![], false);
        assert!(take_action(&mut browser, response).is_none());
        assert!(!browser.is_loading());
        assert!(browser.browser_is_open());
        assert_eq!(
            browser.model_state.error.as_deref(),
            Some("the loader stopped before finishing")
        );
        // A malformed real CSV retains its file/line diagnostic instead of opening.
        let invalid = root.join("SECOND/20260316.csv");
        std::fs::write(&invalid, "this is not a replay file").unwrap();
        browser.dispatch(Intent::LoadSelected);
        let deadline = Instant::now() + Duration::from_secs(10);
        while browser.is_loading() {
            let (response, _, _) = frame(&mut browser, &ctx, vec![], false);
            assert!(take_action(&mut browser, response).is_none());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            browser
                .model_state
                .error
                .as_ref()
                .is_some_and(|error| error.contains("20260316.csv")),
            "real parser errors retain their subject"
        );
    }

    #[derive(Default)]
    struct Source {
        requests: Arc<Mutex<Vec<DownloadRequest>>>,
    }
    impl SessionSource for Source {
        fn label(&self) -> String {
            "Replay adapter fixture".into()
        }
        fn provider(&self) -> crate::config::ProviderKind {
            crate::config::ProviderKind::MetaTrader
        }
        fn commands(
            &self,
            request: &DownloadRequest,
        ) -> Result<Vec<std::process::Command>, DownloadError> {
            self.requests.lock().unwrap().push(request.clone());
            Ok(Vec::new())
        }
    }
    fn source(browser: &mut ReplayBrowser) -> Arc<Mutex<Vec<DownloadRequest>>> {
        let requests = Arc::default();
        browser.view.io.get_data.source = Box::new(Source {
            requests: Arc::clone(&requests),
        });
        requests
    }
    fn land(browser: &mut ReplayBrowser, path: PathBuf) {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(DownloadEvent::Done { tape: path }).unwrap();
        drop(sender);
        browser.view.io.get_data.job = Some(DownloadJob::stopped_for_test(receiver));
    }

    #[test]
    fn replay_download_pump_survives_hidden_browser_and_tab_switch() {
        let root = crate::scratch::ScratchDir::new("registered-replay-download");
        let original = csv(&root, "WINJ26", "2026-03-12");
        let mut browser = browser(&root);
        let requests = source(&mut browser);
        browser.open_browser();
        browser.dispatch(Intent::SetOpen(false));
        let folder = root.display().to_string();
        let available = ["2026-03-13".to_owned(), "2026-03-16".to_owned()]
            .into_iter()
            .collect();
        let panel = &mut browser.view.io.get_data;
        panel.set_symbol("WINJ26");
        panel.available = Some(available);
        panel.available_for = "WINJ26".into();
        for day in ["2026-03-13", "2026-03-16"] {
            panel
                .days
                .selection
                .click(day, panel.available.as_ref().unwrap(), false);
        }
        panel.download(&folder);
        assert_eq!(
            requests.lock().unwrap()[0].day.as_deref(),
            Some("2026-03-13")
        );
        let ctx = egui::Context::default();
        land(&mut browser, csv(&root, "WINJ26", "2026-03-13"));
        let (response, _, _) = frame(&mut browser, &ctx, vec![], false);
        assert!(
            take_action(&mut browser, response).is_none(),
            "downloads do not start playback"
        );
        assert!(
            !browser.browser_is_open(),
            "the pump survives a closed browser"
        );
        assert_eq!(
            browser.model_state.library.as_ref().unwrap().sessions.len(),
            2
        );
        assert_eq!(
            browser.model_state.selected_entry().unwrap().path,
            original,
            "batch refresh preserves selection"
        );
        assert_eq!(
            requests.lock().unwrap()[1].day.as_deref(),
            Some("2026-03-16"),
            "feed's queue launches the next job"
        );
        browser.open_browser();
        browser.dispatch(Intent::EnterDownload {
            symbol: None,
            explicit: false,
        });
        browser.dispatch(Intent::SelectSessions);
        land(&mut browser, csv(&root, "WINJ26", "2026-03-16"));
        let (response, _, _) = frame(&mut browser, &ctx, vec![], false);
        assert!(take_action(&mut browser, response).is_none());
        assert_eq!(
            browser.model_state.library.as_ref().unwrap().sessions.len(),
            3
        );
        assert_eq!(browser.model_state.selected_entry().unwrap().path, original);
        assert!(!browser.view.io.get_data.is_busy());
        // A single completed download lands on its own day, unlike a batch refresh.
        let chosen = csv(&root, "WINJ26", "2026-03-17");
        let panel = &mut browser.view.io.get_data;
        let request = panel.request(Some("2026-03-17".into()), 0, root.to_path_buf());
        panel.start(
            request,
            quantick_feed::replay_desk::Phase::Running { ticks: 0 },
        );
        browser.dispatch(Intent::SetOpen(false));
        land(&mut browser, chosen.clone());
        let (response, _, _) = frame(&mut browser, &ctx, vec![], false);
        assert!(take_action(&mut browser, response).is_none());
        assert_eq!(browser.model_state.selected_entry().unwrap().path, chosen);
        assert_eq!(browser.model_state.tab, BrowserTab::Sessions);
    }

    #[test]
    fn replay_local_effects_settle_before_dependent_paint() {
        let root = crate::scratch::ScratchDir::new("registered-replay-scan");
        csv(&root, "WINJ26", "2026-03-13");
        let mut registry = crate::surfaces::Surfaces::default();
        let browser = &mut registry.replay_browser;
        browser.dispatch(Intent::Initialize {
            folder: root.display().to_string(),
            stored: None,
            day_before: Some(false),
        });
        source(browser);
        browser.open_browser();
        assert_eq!(
            browser.model_state.library.as_ref().unwrap().sessions.len(),
            1,
            "menu entry settles its scan before returning"
        );
        let ctx = egui::Context::default();
        let mut response = SurfaceResponse::default();
        let output = ctx.run(Default::default(), |ctx| {
            response = registry
                .draw_slot_unscoped(Slot::EarlyEdgePanels, &mut ViewContext { ctx, ui: None });
        });
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("WINJ26") && text.galley.job.text.contains("2026-03-13"))), "completed scan is visible to the same frame's list");
        assert!(
        response.model_effects.iter().all(
            |effect| !matches!(effect, HostEffect::ReplayBrowser(effect) if effect.local.is_some())
        ),
        "local effects are consumed before registered conversion"
    );
        let browser = &mut registry.replay_browser;
        let held = browser
            .model_state
            .library
            .as_ref()
            .unwrap()
            .sessions
            .as_ptr();
        csv(&root, "WINJ26", "2026-03-16");
        frame(browser, &ctx, vec![], false);
        assert_eq!(
            browser.model_state.library.as_ref().unwrap().sessions.len(),
            1,
            "unchanged paint does not rescan disk"
        );
        assert_eq!(
            browser
                .model_state
                .library
                .as_ref()
                .unwrap()
                .sessions
                .as_ptr(),
            held
        );
        // Focus the actual folder TextEdit, then press Enter. Its lost-focus
        // handler must finish the scan before the later list paints this frame.
        let (_, output, _) = frame(browser, &ctx, vec![], false);
        let position =
            text_position(&output, &root.display().to_string()).expect("folder draft is painted");
        for pressed in [true, false] {
            frame(browser, &ctx, pointer(position, pressed), false);
        }
        assert!(
            ctx.memory(|memory| memory.focused().is_some()),
            "the folder field received actual pointer focus"
        );
        let (_, output, _) = frame(
            browser,
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: Some(egui::Key::Enter),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            false,
        );
        assert_eq!(
            browser.model_state.library.as_ref().unwrap().sessions.len(),
            2,
            "folder Enter settles the actual disk scan synchronously"
        );
        let entry = &browser.model_state.library.as_ref().unwrap().sessions[1];
        let row = format!("{}      {}", entry.label(), entry.size_label());
        assert!(
            text_position(&output, &row).is_some(),
            "the newly scanned row paints in the Enter frame"
        );
        browser.dispatch(Intent::ChooseFolder);
        assert_eq!(
            browser.model_state.library.as_ref().unwrap().sessions.len(),
            2,
            "folder commit settles before its caller paints"
        );
        assert_eq!(
            browser.take_folder_change(),
            Some(Some(root.display().to_string()))
        );
        assert_eq!(browser.take_folder_change(), None);
        browser.open_get_data(Some("WINJ26"));
        assert_eq!(browser.model_state.tab, BrowserTab::GetData);
        assert_eq!(browser.view.io.get_data.symbol, "WINJ26");
        assert!(
            browser.autostart(2.0, Some("2026-03-16"), false),
            "two-stage hook scans before selecting"
        );
        assert_eq!(
            browser
                .model_state
                .selected_entry()
                .unwrap()
                .date
                .unwrap()
                .label(),
            "2026-03-16"
        );
        let request = wait_open(browser, &ctx);
        assert_eq!(request.session.symbol, "WINJ26");
        let (sender, receiver) = std::sync::mpsc::channel();
        browser.view.io.picker = Some(receiver);
        browser.dispatch(Intent::PickerStarted);
        sender.send(Some(root.to_path_buf())).unwrap();
        drop(sender);
        frame(browser, &ctx, vec![], false);
        assert!(!browser.model_state.picker_pending);
        assert_eq!(
            browser.take_folder_change(),
            Some(Some(root.display().to_string())),
            "native picker completion chooses and scans once"
        );
        assert_eq!(browser.take_folder_change(), None);
    }

    #[test]
    fn replay_load_completion_and_transport_keep_existing_precedence() {
        let root = crate::scratch::ScratchDir::new("registered-replay-precedence");
        let path = csv(&root, "WINJ26", "2026-03-13");
        let mut browser = browser(&root);
        let ctx = egui::Context::default();
        let link = quantick_feed::replay::test_support::detached_link(
            Session::load(&path, ParseOptions::default()).unwrap(),
        );
        browser.open_browser();
        browser.dispatch(Intent::LoadSelected);
        // Wait on the actual worker receiver, then put that same result through
        // a ready receiver so completion and keyboard input share one exact pass.
        let outcome = browser
            .view
            .io
            .loading
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(outcome).unwrap();
        browser.view.io.loading = Some(receiver);
        bind(&mut browser, Some(&link));
        let (response, _, _) = frame(&mut browser, &ctx, vec![key(true)], false);
        assert!(
            !browser.browser_is_open(),
            "ready load closes before browser paint"
        );
        assert!(matches!(
            browser.view.transport,
            Some(ReplayAction::Control(ReplayControl::TogglePlay))
        ));
        assert!(
            matches!(
                take_action(&mut browser, response),
                Some(ReplayAction::Open(_))
            ),
            "successful load wins over simultaneous transport"
        );
        let (response, _, _) = frame(&mut browser, &ctx, vec![key(false)], false);
        assert!(
            take_action(&mut browser, response).is_none(),
            "losing transport never escapes in a later frame"
        );
    }

    #[test]
    fn replay_transport_input_and_focus_survive_model_extraction() {
        let root = crate::scratch::ScratchDir::new("registered-replay-transport");
        let path = csv(&root, "WINJ26", "2026-03-13");
        let session = Session::load(&path, ParseOptions::default()).unwrap();
        let handle = quantick_feed::replay::spawn(ReplayRequest {
            session: Arc::new(session),
            options: ReplayOptions {
                skip_idle_over_ms: None,
                ..Default::default()
            },
        });
        let link = handle.replay.as_ref().unwrap();
        let mut browser = browser(&root);
        let ctx = egui::Context::default();
        bind(&mut browser, Some(link));
        let (_, output, canvas) = frame(&mut browser, &ctx, vec![], false);
        assert!(
            canvas.bottom() < 800.0,
            "transport reserves height before CentralPanel"
        );
        let button = text_position(&output, egui_phosphor::regular::PLAY)
            .expect("paused transport shows Play");
        bind(&mut browser, Some(link));
        frame(&mut browser, &ctx, pointer(button, true), false);
        // Bind while paused, then change the live atomics before paint. A cached
        // button-mode fact would incorrectly return Play from the release.
        bind(&mut browser, Some(link));
        handle
            .commands
            .try_send(FeedCommand::Replay(ReplayControl::Play))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !link.status.is_playing() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let (response, _, _) = frame(&mut browser, &ctx, pointer(button, false), false);
        assert!(
            matches!(
                take_action(&mut browser, response),
                Some(ReplayAction::Control(ReplayControl::Pause))
            ),
            "paint reads the current shared playback state"
        );
        bind(&mut browser, Some(link));
        let (response, _, _) = frame(&mut browser, &ctx, vec![key(true)], true);
        assert!(
            take_action(&mut browser, response).is_none(),
            "a focused text field owns Space"
        );
        ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        bind(&mut browser, Some(link));
        frame(&mut browser, &ctx, vec![key(false)], false);
        bind(&mut browser, Some(link));
        let (response, _, _) = frame(&mut browser, &ctx, vec![key(true)], false);
        assert!(matches!(
            take_action(&mut browser, response),
            Some(ReplayAction::Control(ReplayControl::TogglePlay))
        ));
    }

    #[test]
    fn replay_pass_input_is_consumed_and_rebind_keeps_browser() {
        let root = crate::scratch::ScratchDir::new("registered-replay-pass");
        let first = csv(&root, "FIRST", "2026-03-13");
        let second = csv(&root, "SECOND", "2026-03-16");
        let first = quantick_feed::replay::test_support::detached_link(
            Session::load(&first, ParseOptions::default()).unwrap(),
        );
        let second = quantick_feed::replay::test_support::detached_link(
            Session::load(&second, ParseOptions::default()).unwrap(),
        );
        let mut browser = browser(&root);
        browser.open_browser();
        browser.dispatch(Intent::Select(1));
        let ctx = egui::Context::default();
        bind(&mut browser, Some(&first));
        let (_, _, with_transport) = frame(&mut browser, &ctx, vec![], false);
        assert!(
            browser.view.pass_link.is_none(),
            "pass input is consumed on entry"
        );
        let (response, _, without_transport) = frame(&mut browser, &ctx, vec![key(true)], false);
        assert!(
            take_action(&mut browser, response).is_none(),
            "missing input cannot replay the prior tab's transport"
        );
        assert!(
            without_transport.bottom() > with_transport.bottom(),
            "missing transport releases its reservation"
        );
        browser.bind_pass(
            Some(&second),
            &MarketMenu {
                current: Some("SECOND"),
                catalogue: vec!["SECOND", "FIRST"],
            },
        );
        let (_, output, _) = frame(&mut browser, &ctx, vec![key(false)], false);
        assert!(
            text_position(&output, &second.label()).is_some(),
            "the rebound active tab supplies transport"
        );
        assert!(browser.browser_is_open());
        assert_eq!(
            browser.model_state.selected,
            Some(1),
            "global browser selection survives active-tab changes"
        );
        assert_eq!(browser.model_state.offered_symbols, ["SECOND", "FIRST"]);
    }
}
