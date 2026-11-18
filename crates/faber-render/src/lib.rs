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
    plane: [f32; 4],
    selected: [u32; 4],
    hovered: [u32; 4],
    section_style: [f32; 4],
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub hatch: bool,
    pub hovered: Option<String>,
    pub cut_colour: Option<[u8; 3]>,
    uniform: wgpu::Buffer,
    identity: wgpu::Buffer,
    bind: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    #[cfg(debug_assertions)]
    layout: wgpu::PipelineLayout,
    #[cfg(debug_assertions)]
    shader_source: String,
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
    cap_meshes: std::collections::BTreeMap<String, Mesh>,
    ranges: std::collections::BTreeMap<String, [u32; 2]>,
}

impl DrawList {
    fn draw_visible(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        hidden: &BTreeSet<String>,
        posed: bool,
    ) {
        pass.set_vertex_buffer(0, self.buffer.slice(..));
        if !posed && hidden.is_empty() {
            pass.draw(0..self.count, 0..1);
            return;
        }
        let mut end = 0;
        for (index, (id, &[start, stop])) in self.ranges.iter().enumerate() {
            end = end.max(stop);
            if !hidden.contains(id) && start < stop {
                let instance = if posed { index as u32 } else { 0 };
                pass.draw(start..stop, instance..instance + 1);
            }
        }
        if end < self.count {
            let instance = if posed { self.ranges.len() as u32 } else { 0 };
            pass.draw(end..self.count, instance..instance + 1);
        }
    }

    pub fn cap_mesh(&self, id: &str) -> Option<&Mesh> {
        self.cap_meshes.get(id)
    }
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
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera"),
            size: std::mem::size_of::<Uniform>() as u64,
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
        let pipeline =
            Self::surface_pipeline(&device, &pipeline_layout, include_str!("shader.wgsl"));
        Self {
            hatch: true,
            hovered: None,
            cut_colour: None,
            device: device.clone(),
            queue,
            identity: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Identity transform"),
                contents: bytemuck::cast_slice(&glam::Mat4::IDENTITY.to_cols_array()),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            uniform,
            bind,
            pipeline,
            #[cfg(debug_assertions)]
            layout: pipeline_layout,
            #[cfg(debug_assertions)]
            shader_source: include_str!("shader.wgsl").into(),
        }
    }

    fn surface_pipeline(
        device: &wgpu::Device,
        pipeline_layout: &wgpu::PipelineLayout,
        source: &str,
    ) -> wgpu::RenderPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Faber surfaces"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let attributes =
            wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32];
        let transforms =
            wgpu::vertex_attr_array![4=>Float32x4,5=>Float32x4,6=>Float32x4,7=>Float32x4];
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Faber solid"),
            layout: Some(pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &transforms,
                    }),
                ],
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
        })
    }

    #[cfg(debug_assertions)]
    pub fn refresh_shader(&mut self) -> Result<bool> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shader.wgsl");
        let source = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.to_string()),
        };
        self.apply_shader(source)
    }

    #[cfg(debug_assertions)]
    fn apply_shader(&mut self, source: String) -> Result<bool> {
        if source == self.shader_source {
            return Ok(false);
        }
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pipeline = Self::surface_pipeline(&self.device, &self.layout, &source);
        self.shader_source = source;
        if let Some(error) = pollster::block_on(scope.pop()) {
            return Err(error.to_string());
        }
        self.pipeline = pipeline;
        Ok(true)
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
        self.upload_painted(bodies, hidden, plane, selected, &Default::default())
    }

    /// Uploads bodies. A paint entry gives one sRGB colour per triangle of that body.
    /// Paint does not apply to a section cut.
    pub fn upload_painted(
        &self,
        bodies: &[Body],
        hidden: &BTreeSet<String>,
        plane: Option<Section>,
        selected: Option<&str>,
        paint: &std::collections::BTreeMap<String, Vec<[u8; 3]>>,
    ) -> Result<DrawList> {
        let mut vertices = Vec::new();
        let mut ranges = std::collections::BTreeMap::new();
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
            let start = vertices.len() as u32;
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
                let paint = paint.get(&body.id).map(Vec::as_slice);
                append_painted(&mut vertices, &body.mesh, colour, 0., paint);
            }
            ranges.insert(body.id.clone(), [start, vertices.len() as u32]);
        }
        let floor_mesh = Mesh {
            normals: Vec::new(),
            vertices: vec![
                [-extent as f64, -extent as f64, floor as f64 - 0.5],
                [extent as f64, -extent as f64, floor as f64 - 0.5],
                [extent as f64, extent as f64, floor as f64 - 0.5],
                [-extent as f64, extent as f64, floor as f64 - 0.5],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
        };
        append(&mut vertices, &floor_mesh, [226, 230, 234], 1.);
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Solid vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        Ok(DrawList {
            cap_meshes: Default::default(),
            buffer,
            ranges,
            count: vertices.len() as u32,
        })
    }

    pub fn draw(&self, target: &Target, list: &DrawList, camera: Camera) {
        self.draw_view(target, list, camera, None, None, None, &Default::default());
    }

    pub fn caps(
        device: &wgpu::Device,
        bodies: &[Body],
        indices: &[faber_scene::SectionIndex],
        hidden: &BTreeSet<String>,
        plane: Section,
        poses: &std::collections::BTreeMap<String, glam::Mat4>,
    ) -> Result<Option<DrawList>> {
        let mut vertices = Vec::new();
        let mut ranges = std::collections::BTreeMap::new();
        let mut cap_meshes = std::collections::BTreeMap::new();
        for (body, index) in bodies
            .iter()
            .zip(indices)
            .filter(|(body, _)| !hidden.contains(&body.id))
        {
            let pose = poses
                .get(&body.id)
                .copied()
                .unwrap_or(glam::Mat4::IDENTITY)
                .as_dmat4();
            let plane = plane.in_body(pose);
            let n = glam::DVec3::from(plane.normal);

            let [lo, hi] = body.bounds.map(glam::DVec3::from);
            let centre = n.dot((lo + hi) * 0.5) - plane.offset;
            if centre.abs() > n.abs().dot((hi - lo) * 0.5) + 1e-8 {
                continue;
            }
            let mut cap = index.cap(&body.mesh, plane)?;
            for v in &mut cap.vertices {
                *v = pose.transform_point3((*v).into()).into();
            }
            for v in &mut cap.normals {
                *v = pose.transform_vector3((*v).into()).into();
            }
            let start = vertices.len() as u32;
            append(&mut vertices, &cap, body.colour, 2.);
            cap_meshes.insert(body.id.clone(), cap);
            ranges.insert(body.id.clone(), [start, vertices.len() as u32]);
        }
        if vertices.is_empty() {
            return Ok(None);
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Section caps"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Ok(Some(DrawList {
            cap_meshes,
            buffer,
            count: vertices.len() as u32,
            ranges,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_view(
        &self,
        target: &Target,
        list: &DrawList,
        camera: Camera,
        selected: Option<&str>,
        plane: Option<Section>,
        caps: Option<&DrawList>,
        poses: &std::collections::BTreeMap<String, glam::Mat4>,
    ) {
        self.draw_visible_view(
            target,
            list,
            camera,
            selected,
            plane,
            caps,
            poses,
            &BTreeSet::new(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw_visible_view(
        &self,
        target: &Target,
        list: &DrawList,
        camera: Camera,
        selected: Option<&str>,
        plane: Option<Section>,
        caps: Option<&DrawList>,
        poses: &std::collections::BTreeMap<String, glam::Mat4>,
        hidden: &BTreeSet<String>,
    ) {
        let instances = (!poses.is_empty()).then(|| {
            let transforms: Vec<_> = list
                .ranges
                .keys()
                .map(|id| {
                    poses
                        .get(id)
                        .copied()
                        .unwrap_or(glam::Mat4::IDENTITY)
                        .to_cols_array()
                })
                .chain(std::iter::once(glam::Mat4::IDENTITY.to_cols_array()))
                .collect();
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Assembly pose"),
                    contents: bytemuck::cast_slice(&transforms),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let range = |list: &DrawList, id: Option<&str>| {
            id.and_then(|id| list.ranges.get(id).copied())
                .unwrap_or_default()
        };
        let ranges = |id: Option<&str>| {
            let [a, b] = range(list, id);
            let [c, d] = caps.map(|list| range(list, id)).unwrap_or_default();
            [a, b, c, d]
        };
        let tint = self
            .cut_colour
            .map_or([-1.; 3], |rgb| rgb.map(|c| (c as f32 / 255.).powf(2.2)));
        let u = Uniform {
            section_style: [tint[0], tint[1], tint[2], if self.hatch { 1. } else { 0. }],
            matrix: camera
                .matrix(target.width as f32 / target.height as f32)
                .to_cols_array_2d(),
            selected: ranges(selected),
            hovered: ranges(self.hovered.as_deref()),
            plane: plane.map_or([0.; 4], |p| {
                let sign = if p.reverse { -1. } else { 1. };
                (glam::DVec3::from(p.normal).normalize() * sign)
                    .extend(-p.offset * sign)
                    .as_vec4()
                    .to_array()
            }),
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
                            r: 0.72,
                            g: 0.75,
                            b: 0.79,
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
            pass.set_vertex_buffer(1, instances.as_ref().unwrap_or(&self.identity).slice(..));
            list.draw_visible(&mut pass, hidden, instances.is_some());
            if let Some(cap) = caps {
                pass.set_vertex_buffer(1, self.identity.slice(..));
                cap.draw_visible(&mut pass, hidden, false);
            }
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
    append_painted(vertices, mesh, colour, kind, None);
}

/// Adds a mesh. A paint list gives one colour per triangle in place of `colour`.
fn append_painted(
    vertices: &mut Vec<Vertex>,
    mesh: &Mesh,
    colour: [u8; 3],
    kind: f32,
    paint: Option<&[[u8; 3]]>,
) {
    let tint = |c: [u8; 3]| c.map(|c| (c as f32 / 255.).powf(2.2));
    let colour = tint(colour);
    if kind == 2. {
        // Encode distance to the outer and inner outlines in barycentric coordinates.
        let edge = |t: &[u32; 3], i: usize| {
            let a = t[(i + 1) % 3];
            let b = t[(i + 2) % 3];
            (a.min(b), a.max(b))
        };
        let mut counts = std::collections::HashMap::new();
        for t in &mesh.triangles {
            for i in 0..3 {
                *counts.entry(edge(t, i)).or_insert(0usize) += 1;
            }
        }
        for t in &mesh.triangles {
            for (corner, &index) in t.iter().enumerate() {
                let normal = std::array::from_fn(|i| {
                    if corner == i || counts[&edge(t, i)] != 1 {
                        1.
                    } else {
                        0.
                    }
                });
                vertices.push(Vertex {
                    position: mesh.vertices[index as usize].map(|x| x as f32),
                    normal,
                    colour,
                    kind,
                });
            }
        }
        return;
    }
    let normals = faber_scene::corner_normals(mesh);
    for (index, (t, normals)) in mesh.triangles.iter().zip(normals).enumerate() {
        let colour = paint
            .and_then(|paint| paint.get(index))
            .map_or(colour, |c| tint(*c));
        let points = t.map(|i| Vec3::from(mesh.vertices[i as usize].map(|v| v as f32)));
        vertices.extend(points.into_iter().zip(normals).map(|(p, normal)| Vertex {
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
    save_painted(
        bodies,
        path,
        plane,
        &Default::default(),
        width,
        height,
        view,
    )
}

/// Writes an image. A paint entry gives per-triangle sRGB colours for one body.
pub fn save_painted(
    bodies: &[Body],
    path: &Path,
    plane: Option<Section>,
    paint: &std::collections::BTreeMap<String, Vec<[u8; 3]>>,
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
    let list = gpu.upload_painted(bodies, &BTreeSet::new(), plane, None, paint)?;
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

#[cfg(all(test, debug_assertions))]
mod live_shader_tests {
    use super::*;
    #[test]
    fn changed_shader_reloads_and_recovers_from_invalid_source() {
        let mut gpu = Gpu::headless().unwrap();
        let source = include_str!("shader.wgsl");
        assert!(!gpu.apply_shader(source.into()).unwrap());
        assert!(gpu.apply_shader("invalid".into()).is_err());
        assert!(gpu.apply_shader(format!("{source}\n")).unwrap());
    }
}
