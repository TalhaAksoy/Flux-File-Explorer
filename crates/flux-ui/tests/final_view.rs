//! Enumeration, metadata fill and UI event pumping must converge to a
//! globally sorted/filtered view even when an earlier compute went stale.
use flux_ui::app::FluxApp;
use std::time::{Duration, Instant};

#[test]
fn metadata_completion_preserves_final_name_sort() {
    let dir = tempfile::tempdir().unwrap();
    for i in (0..512).rev() {
        std::fs::write(
            dir.path().join(format!("report_{i}.txt")),
            "metadata fixture",
        )
        .unwrap();
    }
    let ctx = egui::Context::default();
    let mut app = FluxApp::from_settings(flux_settings::Settings::default());
    app.navigate(dir.path().to_owned());
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut stable_since = None;
    loop {
        app.pump(&ctx);
        let complete = {
            let snap = app.current_snap().unwrap();
            let s = snap.read().unwrap();
            !s.state.is_loading()
                && s.entry_count() == 512
                && s.entries()
                    .iter()
                    .all(|e| e.size != flux_core::UNKNOWN_SIZE)
                && s.view().iter().enumerate().all(|(row, &idx)| {
                    s.entry(idx).unwrap().name
                        == std::ffi::OsString::from(format!("report_{row}.txt"))
                })
        };
        if complete {
            let since = stable_since.get_or_insert_with(Instant::now);
            if since.elapsed() > Duration::from_millis(100) {
                break;
            }
        } else {
            stable_since = None;
        }
        assert!(
            Instant::now() < deadline,
            "metadata completed without a stable natural Name sort"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
