use faber_files::gcode::Writer;
use faber_ports::program::{Dialect, Move, Program, ProgramExport, Section};

fn section(laser: bool) -> Section {
    Section {
        tool_number: 1,
        speed: 6400.,
        laser,
        coolant: false,
        moves: vec![
            Move::Rapid { to: [3., 5., 8.] },
            Move::Feed {
                to: [3., 5., -1.25],
                feed: 125.,
            },
        ],
    }
}
fn written(mut section: Section, dialect: Dialect, extra: bool) -> String {
    if extra {
        section.moves.push(Move::Feed {
            to: [8., 5., -1.25],
            feed: 125.,
        });
    }
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("profile.nc");
    Writer
        .write(
            &Program {
                name: "Plate".into(),
                sections: vec![section],
                warnings: vec![],
            },
            dialect,
            &dest,
        )
        .unwrap();
    std::fs::read_to_string(dest).unwrap()
}
#[test]
fn grbl_program_text() {
    assert_eq!(
        written(section(false), Dialect::Grbl, false),
        "G21\nG90\nG17\nM3 S6400\nG0 Z8\nG0 X3 Y5\nG1 X3 Y5 Z-1.25 F125\nM5\nM2\n"
    );
}
#[test]
fn feed_word_is_written_once() {
    assert_eq!(
        written(section(false), Dialect::Generic, true)
            .matches("F125")
            .count(),
        1
    );
}
#[test]
fn laser_has_no_z() {
    let p = written(section(true), Dialect::GrblLaser, true);
    assert!(p.contains("M4 S6400"));
    assert!(!p.contains('Z'));
    assert!(!p.contains("M6"));
}
#[test]
fn a_second_tool_retracts_in_z_before_it_moves_in_xy() {
    let dir = tempfile::tempdir().unwrap();
    let mut other = section(false);
    other.tool_number = 2;
    other.moves[0] = Move::Rapid { to: [20., 40., 9.] };
    let program = Program {
        name: "Plate".into(),
        sections: vec![section(false), other],
        warnings: vec![],
    };
    for (dialect, tool_change, end) in [
        (Dialect::LinuxCnc, "T2 M6", "M30"),
        (Dialect::Grbl, "(Fit tool 2)\nM0", "M2"),
    ] {
        let file = dir.path().join("plate.nc");
        Writer.write(&program, dialect, &file).unwrap();
        let text = std::fs::read_to_string(file).unwrap();
        assert!(text.contains(&format!("M5\n{tool_change}\nM3 S6400\nG0 Z9\nG0 X20 Y40\n")));
        assert_eq!(text.lines().next_back(), Some(end));
    }
}
#[test]
fn a_bad_late_section_preserves_an_existing_program() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("profile.nc");
    std::fs::write(&dest, "previous").unwrap();
    let mut bad = section(false);
    bad.moves[1] = Move::Feed {
        to: [1., f64::NAN, 0.],
        feed: 30.,
    };
    let program = Program {
        name: "test".into(),
        sections: vec![section(false), bad],
        warnings: vec![],
    };
    assert!(Writer.write(&program, Dialect::Grbl, &dest).is_err());
    assert_eq!(std::fs::read_to_string(dest).unwrap(), "previous");
}
