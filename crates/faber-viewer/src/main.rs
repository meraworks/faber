#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod analysis;
mod app;
mod edit;
mod feature_form;
mod mark;
mod overhang;
mod recent;
mod sketch;

use eframe::{egui, egui_wgpu};
use faber_scene::Section;
use std::path::PathBuf;

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let shot = args.first().is_some_and(|a| a == "shot");
    let root = args
        .get(usize::from(shot))
        .filter(|value| !value.starts_with("--"))
        .map(PathBuf::from);
    let mut out = PathBuf::from("shot.png");
    let mut plane = None;
    let mut reverse = false;
    let mut drives = std::collections::BTreeMap::new();
    let mut i = usize::from(shot) + usize::from(root.is_some());
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                out = PathBuf::from(args.get(i).ok_or("Missing output path")?);
            }
            "--section" => {
                i += 1;
                let normal = match args.get(i).map(String::as_str) {
                    Some("X") => [1., 0., 0.],
                    Some("Y") => [0., 1., 0.],
                    Some("Z") => [0., 0., 1.],
                    _ => return Err("Section axis must be X, Y or Z".into()),
                };
                i += 1;
                let offset = args
                    .get(i)
                    .ok_or("Missing section offset")?
                    .parse::<f64>()
                    .map_err(|e| e.to_string())?;
                if !offset.is_finite() {
                    return Err("Section offset must be finite".into());
                }
                plane = Some(Section {
                    normal,
                    offset,
                    reverse: false,
                });
            }
            "--joint" => {
                let id = args.get(i + 1).ok_or("Supply the joint id")?.clone();
                let raw = args.get(i + 2).ok_or("Supply the joint value")?;
                let value = if let Ok(number) = raw.parse::<f64>() {
                    faber_model::motion::PoseValue::Number(number)
                } else {
                    serde_json::from_str(raw)
                        .map_err(|_| "Supply a joint number or a list of numbers")?
                };
                if drives.insert(id, value).is_some() {
                    return Err("Supply each joint once".into());
                }
                i += 2;
            }
            "--reverse" => reverse = true,
            _ => return Err(format!("Unknown option: {}", args[i])),
        }
        i += 1;
    }
    if let Some(p) = &mut plane {
        p.reverse = reverse;
    }
    if plane.is_some() && root.is_none() {
        return Err("A section needs a project".into());
    }
    let initial = root
        .map(|root| faber_wiring::read(&root).map(|(data, bodies)| (root, data, bodies)))
        .transpose()?;
    if !drives.is_empty() {
        let (_, data, _) = initial.as_ref().ok_or("A pose needs a project")?;
        data.motion.solve_pose(&drives)?;
    }
    let make_app = move |cc: &eframe::CreationContext<'_>| {
        let mut shell = match initial {
            Some((root, data, bodies)) => {
                let mut viewer = app::Shell::new(cc, root, data, bodies, plane);
                viewer.initial_pose(drives);
                viewer
            }
            None => app::Shell::welcome(cc),
        };
        if !shot && let Some(file) = recent::location() {
            shell.load_recent(file);
        }
        shell
    };
    if shot {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 920.))
            .wgpu_setup(gpu_setup())
            .build_eframe(move |cc| make_app(cc));
        for _ in 0..5 {
            harness.step();
        }
        if let Some(error) = &harness.state().error {
            return Err(error.clone());
        }
        let image = harness.render()?;
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        image.save(&out).map_err(|e| e.to_string())?;
        println!("Saved {}", out.display());
        Ok(())
    } else {
        eframe::run_native(
            "Faber",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_title("Faber")
                    .with_icon(mark::icon(256))
                    .with_inner_size([1440., 920.])
                    .with_min_inner_size([1000., 650.]),
                renderer: eframe::Renderer::Wgpu,
                wgpu_options: egui_wgpu::WgpuConfiguration {
                    surface: egui_wgpu::SurfaceConfig::LOW_LATENCY,
                    wgpu_setup: gpu_setup(),
                    ..Default::default()
                },
                ..Default::default()
            },
            Box::new(move |cc| Ok(Box::new(make_app(cc)))),
        )
        .map_err(|e| e.to_string())
    }
}

fn gpu_setup() -> egui_wgpu::WgpuSetup {
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    let default_descriptor = setup.device_descriptor.clone();
    setup.device_descriptor = std::sync::Arc::new(move |adapter| {
        let mut descriptor = default_descriptor(adapter);
        descriptor.required_limits.max_buffer_size = adapter.limits().max_buffer_size;
        descriptor
    });
    setup
        .instance_descriptor
        .backends
        .remove(egui_wgpu::wgpu::Backends::BROWSER_WEBGPU);
    setup.native_adapter_selector = Some(std::sync::Arc::new(|adapters, _| {
        use egui_wgpu::wgpu::DeviceType;
        adapters
            .iter()
            .min_by_key(|a| match a.get_info().device_type {
                DeviceType::DiscreteGpu => 0,
                DeviceType::IntegratedGpu => 1,
                DeviceType::Other | DeviceType::VirtualGpu => 2,
                DeviceType::Cpu => 3,
            })
            .cloned()
            .ok_or_else(|| "No GPU adapter found".into())
    }));
    egui_wgpu::WgpuSetup::CreateNew(setup)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}
