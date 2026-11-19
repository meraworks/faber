use super::*;
use faber_wiring::viewer_link::{self, ViewDirection, ViewerAction};

/// Steps the viewer until the state file reports the sequence number.
fn applied(
    h: &mut egui_kittest::Harness<'_, Shell>,
    root: &std::path::Path,
    seq: u64,
) -> viewer_link::ViewerState {
    let start = Instant::now();
    loop {
        h.step();
        if let Some(state) = viewer_link::read_state(root)
            && state.applied >= seq
        {
            return state;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "The viewer did not apply {seq}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn link_hides_selects_and_reports_the_state() {
    let (dir, mut h) = super::controls_fixture::two_blocks();
    let seq = viewer_link::send(
        dir.path(),
        vec![
            ViewerAction::Hide {
                parts: vec!["right".into(), "missing".into()],
            },
            ViewerAction::Select {
                parts: vec!["left".into()],
            },
        ],
    )
    .unwrap();
    let state = applied(&mut h, dir.path(), seq);
    assert_eq!(h.state().hidden, BTreeSet::from(["right".to_string()]));
    assert_eq!(state.hidden, vec!["right".to_string()]);
    assert_eq!(state.selected, vec!["left".to_string()]);
    assert_eq!(
        state.notices,
        vec!["Part missing was not found.".to_string()]
    );

    let seq = viewer_link::send(dir.path(), vec![ViewerAction::ShowAll]).unwrap();
    let state = applied(&mut h, dir.path(), seq);
    assert!(state.hidden.is_empty() && h.state().hidden.is_empty());
    assert!(state.notices.is_empty());
}

#[test]
fn link_isolates_and_frames_a_part() {
    let (dir, mut h) = super::controls_fixture::two_blocks();
    let seq = viewer_link::send(
        dir.path(),
        vec![
            ViewerAction::Isolate {
                parts: vec!["left".into()],
            },
            ViewerAction::Camera {
                view: Some(ViewDirection::Front),
                parts: vec!["left".into()],
            },
        ],
    )
    .unwrap();
    applied(&mut h, dir.path(), seq);
    assert_eq!(h.state().hidden, BTreeSet::from(["right".to_string()]));
    let target = h.state().camera.target;
    // The left block spans x -30 to -15.
    assert!((target.x + 22.5).abs() < 0.01, "{target}");
}

#[test]
fn a_command_from_before_the_viewer_opened_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Old".into())).unwrap();
    let part: faber_model::Part = serde_json::from_value(serde_json::json!({
        "name": "a", "colour":[100,130,160],
        "placement":{"translation":[0,0,0],"rotation":[0,0,0]},
        "geometry":{"op":"history","output":"b","features":[{"id":"b","geometry":{"op":"box","size":[5,5,5]}}]}
    }))
    .unwrap();
    project.put_part("a".into(), part).unwrap();
    let (data, bodies) = (project.data.clone(), project.bodies.clone());
    drop(project);
    let old = viewer_link::send(
        dir.path(),
        vec![ViewerAction::Hide {
            parts: vec!["a".into()],
        }],
    )
    .unwrap();
    let root = dir.path().to_owned();
    let mut h = egui_kittest::Harness::builder()
        .with_step_dt(1. / 60.)
        .with_size(egui::vec2(1200., 800.))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    let state = applied(&mut h, dir.path(), old);
    assert!(h.state().hidden.is_empty() && state.hidden.is_empty());
}
