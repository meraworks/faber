//! Shared GPU renderer for the viewer and image files.

use bytemuck::{Pod, Zeroable};
use faber_ports::{Body, Mesh, Result};
use faber_scene::{Camera, Section, section};
use glam::Vec3;
use std::{collections::BTreeSet, path::Path};
use wgpu::util::DeviceExt;

static HEADLESS_GPU: std::sync::Mutex<Option<Gpu>> = std::sync::Mutex::new(None);

pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    colour: [f32; 3],
    kind: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniform {
    matrix: [[f32; 4]; 4],
    eye: [f32; 4],
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

pub struct Target {
    pub width: u32,
    pub height: u32,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub display_view: wgpu::TextureView,
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
}

pub struct DrawList {
    buffer: wgpu::Buffer,
    count: u32,
}

impl Gpu {
    pub fn headless() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .or_else(|_| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                force_fallback_adapter: true,
                ..Default::default()
            }))
        })
        .map_err(|e| e.to_string())?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Faber"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| e.to_string())?;
        Ok(Self::new(device, queue))
    }

    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Faber surfaces"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes =
            wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Faber solid"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 4,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        Self {
            device,
            queue,
            uniform,
            bind,
            pipeline,
        }
    }

    pub fn target(&self, width: u32, height: u32) -> Target {
        let width = width.clamp(1, 4096);
        let height = height.clamp(1, 4096);
        let make = |format, sample_count, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: if format == FORMAT {
                    &[wgpu::TextureFormat::Rgba8Unorm]
                } else {
                    &[]
                },
            })
        };
        let texture = make(
            FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let view = texture.create_view(&Default::default());
        let display_view = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            ..Default::default()
        });
        let msaa = make(FORMAT, 4, wgpu::TextureUsages::RENDER_ATTACHMENT)
            .create_view(&Default::default());
        let depth = make(
            wgpu::TextureFormat::Depth32Float,
            4,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
        .create_view(&Default::default());
        Target {
            width,
            height,
            texture,
            view,
            display_view,
            msaa,
            depth,
        }
    }

    pub fn upload(
        &self,
        bodies: &[Body],
        hidden: &BTreeSet<String>,
        plane: Option<Section>,
        selected: Option<&str>,
    ) -> Result<DrawList> {
        let mut vertices = Vec::new();
        let mut floor = 0f32;
        let mut extent = 300f32;
        for body in bodies {
            floor = floor.min(body.bounds[0][2] as f32);
            extent = extent.max(
                body.bounds
                    .iter()
                    .flatten()
                    .map(|v| v.abs() as f32)
                    .fold(0., f32::max)
                    * 2.5,
            );
            if hidden.contains(&body.id) {
                continue;
            }
            let colour = if selected == Some(&body.id) {
                [126, 197, 197]
            } else {
                body.colour
            };
            if let Some(plane) = plane {
                let cut = section(&body.mesh, plane)?;
                append(&mut vertices, &cut.surface, colour, 0.);
                append(&mut vertices, &cut.cap, colour, 2.);
            } else {
                append(&mut vertices, &body.mesh, colour, 0.);
            }
        }
        let floor_mesh = Mesh {
            vertices: vec![
                [-extent as f64, -extent as f64, floor as f64 - 0.5],
                [extent as f64, -extent as f64, floor as f64 - 0.5],
                [extent as f64, extent as f64, floor as f64 - 0.5],
                [-extent as f64, extent as f64, floor as f64 - 0.5],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        };
        append(&mut vertices, &floor_mesh, [34, 41, 52], 1.);
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Solid vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        Ok(DrawList {
            buffer,
            count: vertices.len() as u32,
        })
    }

    pub fn draw(&self, target: &Target, list: &DrawList, camera: Camera) {
        let u = Uniform {
            matrix: camera
                .matrix(target.width as f32 / target.height as f32)
                .to_cols_array_2d(),
            eye: camera.eye().extend(1.).to_array(),
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Faber view"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.msaa,
                    depth_slice: None,
                    resolve_target: Some(&target.view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.014,
                            g: 0.020,
                            b: 0.030,
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_vertex_buffer(0, list.buffer.slice(..));
            pass.draw(0..list.count, 0..1);
        }
        self.queue.submit([encoder.finish()]);
    }

    pub fn read(&self, target: &Target) -> Result<Vec<u8>> {
        let row = target.width * 4;
        let padded = row.div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Image readback"),
            size: (padded * target.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(target.height),
                },
            },
            wgpu::Extent3d {
                width: target.width,
                height: target.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let data = slice.get_mapped_range().map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for y in 0..target.height {
            out.extend_from_slice(&data[(y * padded) as usize..(y * padded + row) as usize]);
        }
        drop(data);
        buffer.unmap();
        Ok(out)
    }
}

fn append(vertices: &mut Vec<Vertex>, mesh: &Mesh, colour: [u8; 3], kind: f32) {
    let colour = colour.map(|c| (c as f32 / 255.).powf(2.2));
    for t in &mesh.triangles {
        let points = t.map(|i| Vec3::from(mesh.vertices[i as usize].map(|v| v as f32)));
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or_zero()
            .to_array();
        vertices.extend(points.map(|p| Vertex {
            position: p.to_array(),
            normal,
            colour,
            kind,
        }));
    }
}

pub fn save(
    bodies: &[Body],
    path: &Path,
    plane: Option<Section>,
    width: u32,
    height: u32,
) -> Result<()> {
    save_view(bodies, path, plane, width, height, faber_scene::View::Iso)
}

pub fn save_view(
    bodies: &[Body],
    path: &Path,
    plane: Option<Section>,
    width: u32,
    height: u32,
    view: faber_scene::View,
) -> Result<()> {
    let mut cached = HEADLESS_GPU
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if cached.is_none() {
        *cached = Some(Gpu::headless()?);
    }
    let gpu = cached.as_ref().ok_or("Renderer is not available")?;
    let target = gpu.target(width, height);
    let list = gpu.upload(bodies, &BTreeSet::new(), plane, None)?;
    let mut camera = Camera::fit(bodies);
    camera.set_view(view);
    gpu.draw(&target, &list, camera);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    image::save_buffer(
        path,
        &gpu.read(&target)?,
        target.width,
        target.height,
        image::ColorType::Rgba8,
    )
    .map_err(|e| e.to_string())
}
