mod input;
mod scan;
mod schema_tool;
mod server;

use rmcp::ServiceExt;
use std::path::Path;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("scan") => { scan::run(); Ok(()) }
        Some("mcp") => match server::Faber::new().serve(rmcp::transport::stdio()).await {
            Ok(service) => service.waiting().await.map(|_| ()).map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        },
        Some("guide") => { println!("{}",server::GUIDE); Ok(()) }
        Some("schema") if args.len() == 1 => serde_json::to_string_pretty(&schemars::schema_for!(faber_model::ProjectData))
            .map(|schema| println!("{schema}")).map_err(|error| error.to_string()),
        Some("drawing") if args.len() == 4 => drawing(Path::new(&args[1]), &args[2], Path::new(&args[3])),
        Some("gcode") if args.len() >= 4 => gcode(&args),
        Some("flat") if args.len() == 4 => faber_wiring::read_snapshot(Path::new(&args[1]))
            .and_then(|project| faber_wiring::sheet::report(&project,&args[2],Some(Path::new(&args[3]))))
            .map(|report| println!("{report}")),
        Some("render") if args.len() == 3 => faber_wiring::read(Path::new(&args[1])).and_then(|(_,bodies)|faber_wiring::render(&bodies,Path::new(&args[2]),None,1200,1000)),
        Some("status") if args.len()==2 => {
            let root=Path::new(&args[1]);
            faber_wiring::read_snapshot(root).and_then(|p|server::project_status(root,&p)).map(|v|println!("{v}"))
        },
        Some("import") if args.len() == 3 => faber_wiring::archive::import(Path::new(&args[1]), Path::new(&args[2])),
        Some("migrate") if args.len() == 2 => migrate(Path::new(&args[1])),
        Some("build") if args.len() == 2 => build(Path::new(&args[1])),
        Some("bom") if args.len() >= 2 => parts_list(&args),
        Some("export") if args.len() >= 3 => export(&args),
        _ => Err("Usage: faber mcp | import ARCHIVE.f3d NEW_DIRECTORY | migrate PROJECT | build PROJECT | status PROJECT | bom PROJECT [--format json|csv|md] | export PROJECT DIRECTORY [--configuration NAME|--all-configurations] [--format stl|step|3mf] [--tolerance MM] [--force] | render PROJECT IMAGE | drawing PROJECT ID FILE | flat PROJECT PART FILE | gcode PROJECT SETUP FILE [--dialect NAME] [--force] | schema | guide | faber scan PACKAGE".into()),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}

fn drawing(root: &Path, id: &str, path: &Path) -> Result<(), String> {
    let project = faber_wiring::drawing::open_snapshot(root)?;
    let rendered = faber_wiring::drawing::export(&project, id, path, None)?;
    println!(
        "{}",
        serde_json::json!({"path":path,"revision":project.data.revision,
        "problems":rendered.problems,"limits":rendered.limits})
    );
    Ok(())
}

fn program_dialect(value: Option<&str>) -> Result<faber_ports::program::Dialect, String> {
    use faber_ports::program::Dialect;
    match value.unwrap_or("generic") {
        "generic" => Ok(Dialect::Generic),
        "grbl" => Ok(Dialect::Grbl),
        "linux_cnc" | "linuxcnc" => Ok(Dialect::LinuxCnc),
        "grbl_laser" => Ok(Dialect::GrblLaser),
        _ => Err("Choose generic, grbl, linux_cnc or grbl_laser.".into()),
    }
}

fn gcode(args: &[String]) -> Result<(), String> {
    let mut options = args[4..].iter();
    let mut dialect = None;
    let mut force = false;
    while let Some(flag) = options.next() {
        match flag.as_str() {
            "--dialect" => dialect = Some(options.next().ok_or("Give the dialect name.")?.as_str()),
            "--force" => force = true,
            _ => return Err(format!("Unknown program option: {flag}.")),
        }
    }
    let root = Path::new(&args[1]);
    let dialect = program_dialect(dialect)?;
    let data = faber_files::read_committed(root)?;
    let run = faber_wiring::machining::calculate(root, &data, &args[2], None)?;
    faber_wiring::machining::check_program_source(root, &data)?;
    faber_wiring::machining::write_program(&run, &data, Path::new(&args[3]), dialect, force)?;
    println!("{}", run.summary());
    Ok(())
}

/// Writes a project in the split layout and prints each file that changed.
fn migrate(root: &Path) -> Result<(), String> {
    let _lock = faber_files::FileStore::open(root)?;
    let data = faber_files::read(root)?;
    for path in faber_files::layout::write(root, &data)? {
        println!("{}", path.display());
    }
    Ok(())
}

/// Builds the design source of a project folder and prints the report.
fn build(root: &Path) -> Result<(), String> {
    let report = faber_wiring::design_source().build(root)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn parts_list(args: &[String]) -> Result<(), String> {
    let format = match &args[2..] {
        [] => "json",
        [flag, format] if flag == "--format" => format.as_str(),
        _ => return Err("Use --format json, csv or md.".into()),
    };
    let project = faber_wiring::read_snapshot(Path::new(&args[1]))?;
    let rows = faber_wiring::bom::report(&project, None, true)?;
    match format {
        "json" => println!(
            "{}",
            serde_json::to_string(&rows).map_err(|e| e.to_string())?
        ),
        "csv" => print!("{}", faber_wiring::bom::csv(&rows)),
        "md" => print!("{}", faber_wiring::bom::markdown(&rows)),
        _ => return Err("Choose json, csv or md.".into()),
    }
    Ok(())
}

fn export(args: &[String]) -> Result<(), String> {
    let mut format = "stl";
    let mut force = false;
    let mut configuration = None;
    let mut all = false;
    let mut tolerance = faber_wiring::EXPORT_TOLERANCE_MM;
    let mut options = args[3..].iter();
    while let Some(option) = options.next() {
        match option.as_str() {
            "--force" => force = true,
            "--configuration" => {
                configuration = Some(
                    options
                        .next()
                        .ok_or("--configuration requires a name.")?
                        .as_str(),
                )
            }
            "--all-configurations" => all = true,
            "--tolerance" => {
                let value = options.next().ok_or("--tolerance requires a value in mm")?;
                tolerance = value
                    .parse::<f64>()
                    .map_err(|_| "--tolerance requires a number in mm")?;
            }
            "--format" => {
                format = options.next().ok_or("--format requires stl, step or 3mf")?;
                if !matches!(format, "stl" | "step" | "3mf") {
                    return Err("Export format must be stl, step or 3mf".into());
                }
            }
            _ => return Err(format!("Unknown export option: {option}")),
        }
    }
    let root = Path::new(&args[1]);
    let dest = Path::new(&args[2]);
    let files = if configuration.is_some() || all {
        let data = faber_files::read_committed(root)?;
        faber_wiring::configurations::export(
            root,
            &data,
            dest,
            configuration,
            all,
            &faber_wiring::configurations::ExportOptions {
                format,
                force,
                tolerance,
            },
        )?
    } else {
        let (data, bodies) = faber_wiring::read(root)?;
        faber_wiring::export_project_with(&data, &bodies, dest, format, force, tolerance)?
    };
    let mut reply =
        serde_json::json!({"files":files.iter().map(|file| file.to_json()).collect::<Vec<_>>()});
    if matches!(format, "stl" | "3mf") {
        reply["tolerance_mm"] = serde_json::json!(tolerance);
    }
    println!("{reply}");
    Ok(())
}
