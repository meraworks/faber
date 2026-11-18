use faber_ports::EdgeLines;
use faber_scene::{Section, pick_edge, pick_edge_hit};
use glam::{DMat4, Mat4};

fn lines() -> EdgeLines {
    EdgeLines {
        points: vec![
            [-0.8, 0.0, 0.75],
            [0.8, 0.0, 0.75],
            [-0.8, 0.0, 0.25],
            [0.8, 0.0, 0.25],
        ],
        ranges: vec![0..2, 2..4],
    }
}
const RECT: [[f32; 2]; 2] = [[10., 20.], [210., 120.]];

#[test]
fn nearest_edge_within_eight_pixels_wins_and_depth_breaks_a_tie() {
    let lines = lines();
    assert_eq!(
        pick_edge(
            &lines,
            DMat4::IDENTITY,
            Mat4::IDENTITY,
            RECT,
            [110., 78.],
            8.
        ),
        Some(2)
    );
    assert_eq!(
        pick_edge(
            &lines,
            DMat4::IDENTITY,
            Mat4::IDENTITY,
            RECT,
            [110., 78.01],
            8.
        ),
        None
    );
}
#[test]
fn edge_pick_clips_crossing_segments_and_reports_world_point() {
    let lines = lines();
    let cut = Some(Section {
        normal: [1., 0., 0.],
        offset: 0.,
        reverse: false,
    });
    let hit = pick_edge_hit(
        &lines,
        DMat4::IDENTITY,
        Mat4::IDENTITY,
        RECT,
        [113., 70.],
        8.,
        cut,
    )
    .unwrap();
    assert_eq!(hit.edge, 2);
    assert!((hit.distance_px - 3.).abs() < 1e-4);
    assert!(glam::DVec3::from(hit.point).distance(glam::DVec3::new(0., 0., 0.25)) < 1e-8);
    assert!(
        pick_edge_hit(
            &lines,
            DMat4::IDENTITY,
            Mat4::IDENTITY,
            RECT,
            [122., 70.],
            8.,
            cut
        )
        .is_none()
    );
}
