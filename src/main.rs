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
        Some("render") if args.len() == 3 => faber_wiring::read(Path::new(&args[1])).and_then(|(_,bodies)|faber_wiring::render(&bodies,Path::new(&args[2]),None,1200,1000)),
        Some("status") if args.len() == 2 => faber_wiring::read(Path::new(&args[1])).map(|(data,bodies)| {
            println!("{}",server::report(&data,&bodies));
        }),
        Some("import") if args.len() == 3 => faber_wiring::archive::import(Path::new(&args[1]), Path::new(&args[2])),
        Some("migrate") if args.len() == 2 => migrate(Path::new(&args[1])),
        Some("export") if args.len() >= 3 => export(&args),
        _ => Err("Usage: faber mcp | import ARCHIVE.f3d NEW_DIRECTORY | migrate PROJECT | status PROJECT | export PROJECT DIRECTORY [--format stl|step] [--force] | render PROJECT IMAGE | schema | guide | faber scan PACKAGE".into()),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
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

fn export(args: &[String]) -> Result<(), String> {
    let mut format = "stl";
    let mut force = false;
    let mut options = args[3..].iter();
    while let Some(option) = options.next() {
        match option.as_str() {
            "--force" => force = true,
            "--format" => {
                format = options.next().ok_or("--format requires stl or step")?;
                if !matches!(format, "stl" | "step") {
                    return Err("Export format must be stl or step".into());
                }
            }
            _ => return Err(format!("Unknown export option: {option}")),
        }
    }
    let (data, bodies) = faber_wiring::read(Path::new(&args[1]))?;
    let files =
        faber_wiring::export_checked(&bodies, &data.contacts, Path::new(&args[2]), format, force)?;
    println!("{}", serde_json::json!({"files":files}));
    Ok(())
}
