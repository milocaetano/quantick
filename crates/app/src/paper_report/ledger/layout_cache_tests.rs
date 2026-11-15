use super::*;
use rust_decimal::Decimal;

pub(super) fn record(ui: &mut egui::Ui) {
    ui.data_mut(|data| {
        let id = egui::Id::new("ledger_rows_visited");
        data.insert_temp(id, data.get_temp::<usize>(id).unwrap_or_default() + 1);
    });
}

#[test]
fn measured_ledger_warm_frames_only_visit_the_viewport() {
    for count in [100, 1000] {
        let ctx = egui::Context::default();
        let rows: Vec<_> = (0..count)
            .map(|i| {
                if i % 2 == 0 {
                    LedgerRow::Header("Session", i)
                } else {
                    LedgerRow::Day(
                        CivilDate::from_ymd(2026, 10, 21),
                        i,
                        (
                            Decimal::from(i),
                            format!("+{i} BRL and a complete long calculation with neutral units"),
                        ),
                        false,
                    )
                }
            })
            .collect();
        let counter = egui::Id::new("ledger_rows_visited");
        for (frame, width, scale, revision) in [
            (0, 320.0, 1.0, 0),
            (1, 320.0, 1.0, 0),
            (2, 400.0, 1.0, 0),
            (3, 400.0, 1.0, 0),
            (4, 400.0, 1.5, 0),
            (5, 400.0, 1.5, 0),
            (6, 400.0, 1.5, 1),
            (7, 400.0, 1.5, 1),
        ] {
            ctx.data_mut(|data| data.insert_temp(counter, 0_usize));
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 300.0),
                )),
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .native_pixels_per_point = Some(scale);
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    RowList {
                        layout_key: egui::Id::new(revision),
                        rows: &rows,
                        selected: None,
                        selected_saved: None,
                        chart: "",
                        tz: TzOffset::new(0),
                        deletes: false,
                        edit: &mut None,
                        focus_edit: &mut false,
                    }
                    .show(ui, 220.0);
                });
            });
            let visited = ctx.data(|data| data.get_temp::<usize>(counter).unwrap());
            if frame % 2 == 0 {
                assert_eq!(visited, count, "content/layout change remeasures");
            } else {
                assert!(visited < 20, "warm viewport visited {visited} of 1000 rows");
            }
        }
    }
}
