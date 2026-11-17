//! `faber scan <package-dir> [--static-only] [--threads N] [--out FILE]`
//!
//! Exit code: 0 pass, 1 fail, 2 usage or load error.

use faber_clash::{ScanOptions, Scene, Status, scan};
use std::path::PathBuf;

fn usage() -> ! {
    eprintln!("usage: faber scan <package-dir> [--static-only] [--threads N] [--out FILE]");
    std::process::exit(2)
}

pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 || args[0] != "scan" {
        usage();
    }
    let dir = PathBuf::from(&args[1]);
    let mut options = ScanOptions {
        threads: 6,
        ..ScanOptions::default()
    };
    let mut out = dir.join("report.json");
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--static-only" => options.static_only = true,
            "--threads" => {
                i += 1;
                options.threads = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage());
            }
            "--out" => {
                i += 1;
                out = PathBuf::from(args.get(i).unwrap_or_else(|| usage()));
            }
            _ => usage(),
        }
        i += 1;
    }
    let scene = match Scene::load(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let report = scan(&scene, &options).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(2);
    });
    if let Err(e) = report.write(&out) {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
    println!(
        "{} parts, {} poses, {} pair tests ({} rejected by AABB), {:.1} s -> {:?}: {} violations, {} allowed contacts, {} allowed interferences (max depth {:.3} mm)",
        report.part_count,
        report.pose_count,
        report.pair_tests,
        report.aabb_rejected,
        report.seconds,
        report.status,
        report.violations.len(),
        report.allowed_contacts.len(),
        report.allowed_interference.len(),
        report.max_allowed_interference_depth_mm
    );
    for v in report.violations.iter().take(20) {
        println!(
            "  {:?} {:?} {} x {} depth {:.3} gap {:.3}",
            v.pose, v.kind, v.a, v.b, v.depth_mm, v.gap_mm
        );
    }
    std::process::exit(if report.status == Status::Pass { 0 } else { 1 });
}
