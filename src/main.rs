mod input;
mod scan;
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
        Some("export") if args.len() >= 3 => export(&args),
        _ => Err("Usage: faber mcp | status PROJECT | export PROJECT DIRECTORY [--format stl|step] [--force] | render PROJECT IMAGE | schema | guide | faber scan PACKAGE".into()),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
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
