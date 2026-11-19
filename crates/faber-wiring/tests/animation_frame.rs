use faber_wiring::{
    animate::{Animation, AnimationOptions, animate},
    read_snapshot,
};
use serde_json::{Value, json};
use std::{fs, path::Path};
fn read_project(dir: &Path, parts: Value, motion: Value) -> faber_core::Project {
    fs::write(
        dir.join("faber.json"),
        json!({"schema":1,"revision":0,"name":"Frame test","parts":parts,"motion":motion})
            .to_string(),
    )
    .unwrap();
    read_snapshot(dir).unwrap()
}
fn options(root: &Path) -> AnimationOptions {
    AnimationOptions {
        frames: 3,
        width: 256,
        height: 256,
        frames_dir: Some(root.join("frames")),
        ..Default::default()
    }
}
fn pixels(path: &Path) -> [usize; 8] {
    let mut count = [0; 8];
    for p in image::open(path).unwrap().into_rgb8().pixels() {
        let hi = *p.0.iter().max().unwrap();
        let lo = *p.0.iter().min().unwrap();
        if hi - lo < 35 {
            continue;
        }
        let mid = (u16::from(hi) + u16::from(lo)) / 2;
        let mask =
            p.0.into_iter()
                .fold(0, |bits, v| (bits << 1) + usize::from(u16::from(v) > mid));
        count[mask] += 1;
    }
    count
}
#[test]
fn automatic_explode_camera_keeps_all_six_concentric_sleeves_in_frame() {
    let d = tempfile::tempdir().unwrap();
    let mut parts = serde_json::Map::new();
    let square = |r: f64| vec![[-r, -r], [r, -r], [r, r], [-r, r]];
    let colours = [
        [250, 20, 20],
        [20, 250, 20],
        [20, 20, 250],
        [250, 250, 20],
        [20, 250, 250],
        [250, 20, 250],
    ];
    for (i, c) in colours.iter().enumerate() {
        let r = 10. - i as f64 * 1.1;
        let mut inside = square(r - 0.35);
        inside.reverse();
        parts.insert(i.to_string(),json!({"name":format!("Sleeve {i}"),"colour":c,"geometry":{"op":"extrude","profile":{"outer":square(r),"holes":[inside]},"height":16}}));
    }
    let p = read_project(d.path(), parts.into(), json!({}));
    animate(
        &p,
        &Animation::Explode { from: 0., to: 1. },
        &options(d.path()),
        &d.path().join("sleeves.png"),
    )
    .unwrap();
    let samples = pixels(&d.path().join("frames/0003.png"));
    for &count in &samples[1..7] {
        assert!(count > 35, "A sleeve left the frame: {samples:?}");
    }
}
#[test]
fn automatic_motion_camera_keeps_the_moving_end_and_fixed_base_in_frame() {
    let d = tempfile::tempdir().unwrap();
    let part =
        |c: [u8; 3]| json!({"name":"Block","colour":c,"geometry":{"op":"box","size":[6,6,6]}});
    let p = read_project(
        d.path(),
        json!({"base":part([250,20,20]),"traveller":part([20,20,250])}),
        json!({"joints":{"slide":{"kind":"slider","parts":["traveller"],"origin":[0,0,0],"axis":[1,0,0],"limits":[0,120]}}}),
    );
    animate(
        &p,
        &Animation::Motion {
            joint: "slide".into(),
            from: 0.,
            to: 120.,
        },
        &options(d.path()),
        &d.path().join("travel.png"),
    )
    .unwrap();
    let samples = pixels(&d.path().join("frames/0003.png"));
    assert!(
        samples[4] > 10 && samples[1] > 10,
        "The fixed and moving blocks must both be visible: {samples:?}"
    );
}
