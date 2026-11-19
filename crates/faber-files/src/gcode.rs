//! Write a controller program in the work coordinates of one setup.
use faber_ports::{
    Result,
    program::{Dialect, Move, Program, ProgramExport},
};
use std::{fmt::Write, path::Path};

pub struct Writer;

fn number(value: f64) -> Result<String> {
    if !value.is_finite() {
        return Err("A program value must be finite.".into());
    }
    if value.abs() < 0.0005 {
        return Ok("0".into());
    }
    let text = format!("{value:.3}");
    Ok(text.trim_end_matches('0').trim_end_matches('.').to_owned())
}
fn axes(to: [f64; 3], laser: bool) -> Result<String> {
    let [x, y, z] = to;
    let xy = format!("X{} Y{}", number(x)?, number(y)?);
    let z = number(z)?;
    Ok(if laser { xy } else { format!("{xy} Z{z}") })
}
impl ProgramExport for Writer {
    fn write(&self, program: &Program, dialect: Dialect, path: &Path) -> Result<()> {
        if program.sections.is_empty() {
            return Err("The program needs an operation.".into());
        }
        let mut text = String::new();
        for warning in &program.warnings {
            let safe: String = warning
                .chars()
                .map(|c| {
                    if c.is_control() || matches!(c, '(' | ')' | '%') {
                        ' '
                    } else {
                        c
                    }
                })
                .collect();
            writeln!(text, "({safe})").unwrap();
        }
        text.push_str("G21\nG90\nG17\n");
        let laser = dialect == Dialect::GrblLaser;
        let mut previous_tool = None;
        for (index, section) in program.sections.iter().enumerate() {
            if section.laser != laser {
                return Err("Select a laser dialect for a laser cut and a milling dialect for a milling operation.".into());
            }
            if section.speed <= 0. || section.tool_number == 0 {
                return Err("Set a positive spindle speed or laser power and tool number.".into());
            }
            let Some((Move::Rapid { to: first }, rest)) = section.moves.split_first() else {
                return Err("Each operation must start with a rapid move to clearance.".into());
            };
            if index > 0 {
                text.push_str("M5\n")
            }
            if !matches!(dialect, Dialect::Grbl | Dialect::GrblLaser)
                && previous_tool != Some(section.tool_number)
            {
                writeln!(text, "T{} M6", section.tool_number).unwrap();
            } else if dialect == Dialect::Grbl
                && previous_tool.is_some_and(|tool| tool != section.tool_number)
            {
                writeln!(text, "(Fit tool {})\nM0", section.tool_number).unwrap();
            }
            previous_tool = Some(section.tool_number);
            writeln!(
                text,
                "{} S{}",
                if laser { "M4" } else { "M3" },
                number(section.speed)?
            )
            .unwrap();
            if section.coolant {
                text.push_str("M8\n")
            }
            if !laser {
                writeln!(text, "G0 Z{}", number(first[2])?).unwrap();
            }
            writeln!(text, "G0 {}", axes(*first, true)?).unwrap();
            let mut last_feed = None;
            for m in rest {
                match *m {
                    Move::Rapid { to } => writeln!(text, "G0 {}", axes(to, laser)?).unwrap(),
                    Move::Feed { to, feed } => {
                        if feed <= 0. {
                            return Err("The feed must be positive.".into());
                        }
                        let feed = number(feed)?;
                        if feed == "0" {
                            return Err("The feed is below the program precision.".into());
                        }
                        write!(text, "G1 {}", axes(to, laser)?).unwrap();
                        if last_feed.as_ref() != Some(&feed) {
                            write!(text, " F{feed}").unwrap();
                            last_feed = Some(feed);
                        }
                        text.push('\n');
                    }
                    Move::Dwell { seconds } => {
                        if seconds < 0. {
                            return Err("The dwell time must be non-negative.".into());
                        }
                        writeln!(text, "G4 P{}", number(seconds)?).unwrap();
                    }
                }
            }
            if section.coolant {
                text.push_str("M9\n")
            }
        }
        text.push_str(if matches!(dialect, Dialect::Grbl | Dialect::GrblLaser) {
            "M5\nM2\n"
        } else {
            "M5\nM30\n"
        });
        crate::atomic_write(path, text.as_bytes())
    }
}
