use faber_files::apng;
use std::io::Cursor;
#[test]
fn apng_has_the_frame_count_and_a_regular_first_frame() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("test.apng");
    let frames = dir.path().join("frames");
    apng::write(&out, Some(&frames), 2, 2, 12, 24, |i| {
        Ok([i as u8, 2, 3, 255].repeat(4))
    })
    .unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let mut r = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    assert_eq!(r.info().animation_control.unwrap().num_frames, 12);
    for i in 0..12 {
        let mut pixels = vec![0; r.output_buffer_size().unwrap()];
        r.next_frame(&mut pixels).unwrap();
        assert_eq!(&pixels[..4], &[i, 2, 3, 255]);
    }
    assert!(frames.join("0001.png").exists() && frames.join("0012.png").exists());
}
#[test]
fn failed_frame_preserves_existing_output_and_does_not_publish_frames() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("test.png");
    let frames = dir.path().join("frames");
    std::fs::write(&out, b"keep").unwrap();
    let e = apng::write(&out, Some(&frames), 2, 2, 3, 24, |i| {
        if i == 1 {
            Err("Interrupted".into())
        } else {
            Ok(vec![0; 16])
        }
    })
    .unwrap_err();
    assert!(e.contains("Interrupted"));
    assert_eq!(std::fs::read(&out).unwrap(), b"keep");
    assert!(!frames.exists());
}

#[test]
fn an_existing_frame_directory_and_an_unwritable_output_stay_intact() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("animation.png");
    let frames = dir.path().join("frames");
    std::fs::create_dir(&frames).unwrap();
    std::fs::write(frames.join("0001.png"), b"original").unwrap();
    assert!(apng::write(&out, Some(&frames), 2, 2, 2, 24, |_| Ok(vec![0; 16])).is_err());
    assert_eq!(std::fs::read(frames.join("0001.png")).unwrap(), b"original");
    std::fs::create_dir(&out).unwrap();
    let new_frames = dir.path().join("new_frames");
    assert!(apng::write(&out, Some(&new_frames), 2, 2, 2, 24, |_| Ok(vec![0; 16])).is_err());
    assert!(out.is_dir());
    assert!(!new_frames.exists());
}
