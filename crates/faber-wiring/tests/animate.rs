use faber_wiring::{
    animate::{Animation, AnimationOptions, animate},
    read_snapshot,
};
use std::path::Path;
fn project() -> faber_core::Project {
    read_snapshot(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assembly-motion"))
        .unwrap()
}

#[test]
fn the_example_linkage_centres_meet_the_holes_in_both_parts() {
    let p = project();
    let link = &p.data.motion.linkages["connecting_rod"];
    for (joint, pin) in [
        (&link.driver, link.driver_pin),
        (&link.follower, link.follower_pin),
    ] {
        let arm = &p.data.motion.joints[joint].parts[0];
        for id in [arm, &link.rod_parts[0]] {
            let body = p.bodies.iter().find(|b| &b.id == id).unwrap();
            let ray = glam::DVec3::new(pin[0], pin[1], 30.);
            assert!(faber_scene::pick(&body.mesh, ray, -glam::DVec3::Z).is_none());
            for x in [-1.7, 1.7] {
                assert!(
                    faber_scene::pick(&body.mesh, ray + glam::DVec3::X * x, -glam::DVec3::Z)
                        .is_some(),
                    "{joint} pin misses {id}"
                );
            }
        }
    }
}
#[test]
fn turntable_middle_frame_differs_and_last_frame_meets_first() {
    let dir = tempfile::tempdir().unwrap();
    let frames = dir.path().join("frames");
    let opts = AnimationOptions {
        frames: 5,
        width: 160,
        height: 120,
        frames_dir: Some(frames.clone()),
        ..Default::default()
    };
    let out = dir.path().join("spin.apng");
    let report = animate(&project(), &Animation::Turntable { turns: 1. }, &opts, &out).unwrap();
    assert_eq!(report.frames, 5);
    let read = |index| {
        image::open(frames.join(format!("{index:04}.png")))
            .unwrap()
            .to_rgba8()
    };
    assert_ne!(read(1), read(3));
    assert_eq!(read(1), read(5));
}
#[test]
fn explode_animation_starts_assembled() {
    let dir = tempfile::tempdir().unwrap();
    let p = project();
    let frames = dir.path().join("steps");
    let opts = AnimationOptions {
        frames: 3,
        width: 128,
        height: 128,
        frames_dir: Some(frames.clone()),
        ..Default::default()
    };
    animate(
        &p,
        &Animation::Explode { from: 0., to: 1. },
        &opts,
        &dir.path().join("e.png"),
    )
    .unwrap();
    let first = image::open(frames.join("0001.png")).unwrap().to_rgba8();
    assert_ne!(
        first,
        image::open(frames.join("0003.png")).unwrap().to_rgba8()
    );
    let second = dir.path().join("still");
    animate(
        &p,
        &Animation::Explode { from: 0., to: 0. },
        &AnimationOptions {
            frames_dir: Some(second.clone()),
            ..opts
        },
        &dir.path().join("still.png"),
    )
    .unwrap();
    assert_eq!(
        first,
        image::open(second.join("0002.png")).unwrap().to_rgba8()
    );
}
#[test]
fn motion_animation_respects_limits_and_one_frame_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let p = project();
    let out = dir.path().join("fail.png");
    let mut opts = AnimationOptions {
        width: 64,
        height: 64,
        frames: 1,
        ..Default::default()
    };
    let animation = Animation::Motion {
        joint: "carriage".into(),
        from: 0.,
        to: 11.,
    };
    assert_eq!(
        animate(&p, &animation, &opts, &out).unwrap_err(),
        "An animation needs 2 frames or more."
    );
    opts.frames = 3;
    let err = animate(&p, &animation, &opts, &out).unwrap_err();
    assert!(err.contains("10") && err.contains("carriage"), "{err}");
    assert!(!out.exists());
}

#[test]
fn a_camera_transition_keeps_both_saved_endpoints() {
    let dir = tempfile::tempdir().unwrap();
    let p = project();
    let frames = dir.path().join("roll");
    let opts = AnimationOptions {
        frames: 5,
        width: 144,
        height: 112,
        frames_dir: Some(frames.clone()),
        ..Default::default()
    };
    let names = vec!["front_detail".to_owned(), "side_detail".to_owned()];
    animate(
        &p,
        &Animation::Views {
            names: names.clone(),
        },
        &opts,
        &dir.path().join("roll.png"),
    )
    .unwrap();
    for (i, id) in [(1, &names[0]), (5, &names[1])] {
        let view = faber_wiring::views::prepare(
            &p,
            &faber_wiring::views::Picture {
                view: Some(id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let expected = faber_wiring::studio::draw(
            &p.data,
            &view.bodies,
            &view.camera,
            &Default::default(),
            144,
            112,
        )
        .unwrap();
        let read = image::open(frames.join(format!("{i:04}.png")))
            .unwrap()
            .to_rgba8();
        assert_eq!(read.as_raw(), &expected.rgba);
    }
}

#[test]
fn a_view_transition_moves_the_joint_before_the_last_frame() {
    let mut p = project();
    let mut second = p.data.views["front_detail"].clone();
    second
        .joints
        .insert("input".into(), faber_model::motion::PoseValue::Number(30.));
    p.data.views.insert("second".into(), second);
    let dir = tempfile::tempdir().unwrap();
    let frames = dir.path().join("transition");
    let opts = AnimationOptions {
        width: 160,
        height: 120,
        frames: 3,
        frames_dir: Some(frames.clone()),
        ..Default::default()
    };
    animate(
        &p,
        &Animation::Views {
            names: vec!["front_detail".into(), "second".into()],
        },
        &opts,
        &dir.path().join("movement.png"),
    )
    .unwrap();
    let pic = |i| {
        image::open(frames.join(format!("{i:04}.png")))
            .unwrap()
            .to_rgb8()
    };
    assert_ne!(pic(1), pic(2));
    assert_ne!(pic(2), pic(3));
}
