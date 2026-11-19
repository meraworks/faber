//! Studio materials and soft shadows in the same coordinates as the technical view.
mod occlusion;
mod pipeline;
use crate::{DrawList, Gpu, Target};
use bytemuck::{Pod, Zeroable};
use faber_ports::{Body, Result};
use faber_scene::{Section, Surface};
use glam::{Mat4, Vec3};
pub(crate) use pipeline::Pipelines;
use std::collections::{BTreeMap, BTreeSet};
use wgpu::util::DeviceExt;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Style {
    Technical,
    Studio,
}
#[derive(Clone, Copy, Debug)]
pub struct StudioOptions {
    pub ground: bool,
    pub background: [u8; 3],
    pub exposure: f32,
}
impl Default for StudioOptions {
    fn default() -> Self {
        Self {
            ground: true,
            background: [192, 201, 211],
            exposure: 1.,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    base: [f32; 3],
    surface: [f32; 3],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniform {
    vp: [[f32; 4]; 4],
    eye: [f32; 4],
    settings: [f32; 4],
    background: [f32; 4],
    shadow_frame: [f32; 4],
    section: [f32; 4],
    cut_style: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    flags: [f32; 4],
}
struct Piece {
    id: String,
    range: std::ops::Range<u32>,
    bounds: [[f64; 3]; 2],
}
pub struct StudioList {
    buffer: wgpu::Buffer,
    pieces: Vec<Piece>,
    instances: wgpu::Buffer,
}

/// Prepare the material pipelines and the body buffers in a worker.
pub fn uploader(
    gpu: &Gpu,
) -> impl FnOnce(&[Body], &BTreeMap<String, Surface>) -> Result<StudioList> + Send + 'static {
    let device = gpu.device.clone();
    let pipelines = gpu.studio.clone();
    move |bodies, surfaces| {
        pipelines.get_or_init(|| Pipelines::new(&device));
        upload(&device, bodies, surfaces)
    }
}

pub fn upload(
    device: &wgpu::Device,
    bodies: &[Body],
    surfaces: &BTreeMap<String, Surface>,
) -> Result<StudioList> {
    let count = bodies
        .iter()
        .map(|b| b.mesh.triangles.len() as u64 * 3)
        .sum::<u64>();
    if count * 48 > device.limits().max_buffer_size || count > u32::MAX as u64 {
        return Err("The studio mesh exceeds the vertex buffer limit.".into());
    }
    let mut vertices = Vec::with_capacity(count as usize);
    let mut pieces = vec![];
    for body in bodies {
        let m = &body.mesh;
        if m.triangles.is_empty() {
            continue;
        }
        if m.triangles
            .iter()
            .flatten()
            .any(|&i| i as usize >= m.vertices.len())
            || m.vertices
                .iter()
                .flatten()
                .any(|c| !(*c as f32).is_finite())
        {
            return Err("The studio mesh has an invalid vertex.".into());
        }
        let surface = surfaces
            .get(&body.id)
            .copied()
            .unwrap_or_else(|| Surface::from_colour(body.colour));
        let ns = faber_scene::corner_normals(m);
        let ao = occlusion::bake(m, &ns)?;
        let start = vertices.len() as u32;
        for (i, tri) in m.triangles.iter().enumerate() {
            for (k, &v) in tri.iter().enumerate() {
                vertices.push(Vertex {
                    position: m.vertices[v as usize].map(|c| c as f32),
                    normal: ns[i][k],
                    base: surface.base,
                    surface: [surface.roughness, surface.metallic, ao[i][k]],
                });
            }
        }
        pieces.push(Piece {
            id: body.id.clone(),
            range: start..vertices.len() as u32,
            bounds: body.bounds,
        });
    }
    if vertices.is_empty() {
        vertices.push(Vertex::zeroed());
    }
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Studio parts"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let instances = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Studio body placements"),
        size: (pieces.len().max(1) * size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    Ok(StudioList {
        buffer,
        pieces,
        instances,
    })
}
fn batches(
    pass: &mut wgpu::RenderPass<'_>,
    list: &StudioList,
    instances: &wgpu::Buffer,
    hidden: &BTreeSet<String>,
) {
    pass.set_vertex_buffer(0, list.buffer.slice(..));
    pass.set_vertex_buffer(1, instances.slice(..));
    for (i, p) in list.pieces.iter().enumerate() {
        if !hidden.contains(&p.id) {
            pass.draw(p.range.clone(), i as u32..i as u32 + 1);
        }
    }
}
#[allow(clippy::too_many_arguments)]
pub fn draw(
    gpu: &Gpu,
    target: &Target,
    list: &StudioList,
    matrix: Mat4,
    eye: Vec3,
    options: &StudioOptions,
    selected: Option<&str>,
    plane: Option<Section>,
    caps: Option<&DrawList>,
    poses: &BTreeMap<String, Mat4>,
    hidden: &BTreeSet<String>,
    scope: &BTreeSet<String>,
) -> Result<()> {
    if !options.exposure.is_finite() || options.exposure <= 0. {
        return Err("Exposure must be positive and finite.".into());
    }
    if !matrix.is_finite() || !eye.is_finite() {
        return Err("The studio camera is invalid.".into());
    }
    let pipe = gpu.studio.get_or_init(|| Pipelines::new(&gpu.device));
    let mut transforms = vec![];
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = -lo;
    for p in &list.pieces {
        let model = poses.get(&p.id).copied().unwrap_or(Mat4::IDENTITY);
        if !hidden.contains(&p.id) {
            for i in 0..8 {
                let local = std::array::from_fn(|j| p.bounds[(i >> j) & 1][j] as f32);
                let q = model.transform_point3(Vec3::from(local));
                lo = lo.min(q);
                hi = hi.max(q);
            }
        }
        transforms.push(Instance {
            model: model.to_cols_array_2d(),
            flags: [
                f32::from(plane.is_some() && (scope.is_empty() || scope.contains(&p.id))),
                f32::from(selected == Some(p.id.as_str())),
                0.,
                0.,
            ],
        });
    }
    if transforms.is_empty() {
        transforms.push(Instance::zeroed());
    }
    let any = lo.is_finite();
    if !any {
        lo = Vec3::ZERO;
        hi = Vec3::ONE;
    }
    let extent = (hi - lo).length().max(0.0001);
    let centre = (hi + lo) * 0.5;
    let bg = Surface::from_colour(options.background).base;
    let floor_z = lo.z - extent * 1e-5;
    // All sight lines of an orthographic camera have the same direction.
    let orthographic = matrix.row(3).truncate().length_squared() < 1e-16;
    let view = if orthographic {
        let inv = matrix.inverse();
        (inv.project_point3(Vec3::ZERO) - inv.project_point3(Vec3::Z))
            .try_normalize()
            .ok_or("Invalid orthographic camera.")?
            .extend(0.)
    } else {
        eye.extend(1.)
    };
    let ground_faces_camera = if orthographic {
        view.z > 0.
    } else {
        eye.z > floor_z
    };
    let show_ground = options.ground && any && ground_faces_camera;
    let cut = gpu
        .cut_colour
        .map_or([-1.; 3], |c| Surface::from_colour(c).base);
    let uniform = Uniform {
        vp: matrix.to_cols_array_2d(),
        eye: view.to_array(),
        settings: [floor_z, options.exposure, 0., 0.],
        background: [bg[0], bg[1], bg[2], 1.],
        shadow_frame: [
            centre.x + (hi.z - lo.z) * 0.33,
            centre.y + (hi.z - lo.z) * 0.40,
            extent * 2.2,
            extent * 150. + eye.distance(centre) * 4.,
        ],
        section: plane.map_or([0.; 4], |p| {
            let sign = if p.reverse { -1. } else { 1. };
            (glam::DVec3::from(p.normal).normalize() * sign)
                .extend(-p.offset * sign)
                .as_vec4()
                .to_array()
        }),
        cut_style: [cut[0], cut[1], cut[2], f32::from(gpu.hatch)],
    };
    gpu.queue
        .write_buffer(&pipe.uniform, 0, bytemuck::bytes_of(&uniform));
    gpu.queue
        .write_buffer(&list.instances, 0, bytemuck::cast_slice(&transforms));
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    if show_ground {
        let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Ground shadow silhouette"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &pipe.mask_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        p.set_pipeline(&pipe.shadow);
        p.set_bind_group(0, &pipe.scene, &[]);
        batches(&mut p, list, &list.instances, hidden);
    }
    {
        let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Studio"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.msaa,
                depth_slice: None,
                resolve_target: Some(&target.view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: bg[0] as f64,
                        g: bg[1] as f64,
                        b: bg[2] as f64,
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
            ..Default::default()
        });
        p.set_bind_group(0, &pipe.scene, &[]);
        if show_ground {
            p.set_pipeline(&pipe.floor);
            p.set_bind_group(1, &pipe.mask, &[]);
            p.draw(0..6, 0..1);
        }
        p.set_pipeline(&pipe.material);
        p.set_bind_group(0, &pipe.scene, &[]);
        batches(&mut p, list, &list.instances, hidden);
        if let Some(c) = caps {
            p.set_pipeline(&pipe.cap);
            p.set_bind_group(0, &pipe.scene, &[]);
            p.set_vertex_buffer(0, c.buffer.slice(..));
            for (id, &[first, end]) in &c.ranges {
                if !hidden.contains(id) && (scope.is_empty() || scope.contains(id)) {
                    p.draw(first..end, 0..1);
                }
            }
        }
    }
    gpu.queue.submit([enc.finish()]);
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub fn pixels(
    bodies: &[Body],
    surfaces: &BTreeMap<String, Surface>,
    matrix: Mat4,
    eye: Vec3,
    options: &StudioOptions,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    crate::with_headless(|gpu| {
        let target = gpu.target(width, height);
        let list = upload(&gpu.device, bodies, surfaces)?;
        draw(
            gpu,
            &target,
            &list,
            matrix,
            eye,
            options,
            None,
            None,
            None,
            &Default::default(),
            &Default::default(),
            &Default::default(),
        )?;
        gpu.read(&target)
    })
}
pub fn technical_pixels(
    bodies: &[Body],
    matrix: Mat4,
    eye: Vec3,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    crate::with_headless(|gpu| {
        let target = gpu.target(width, height);
        let list = gpu.upload(bodies, &Default::default(), None, None)?;
        gpu.draw_scoped_matrix(
            &target,
            &list,
            matrix,
            eye,
            None,
            None,
            None,
            &Default::default(),
            &Default::default(),
            &Default::default(),
        );
        gpu.read(&target)
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_studio_worker_prepares_all_pipelines_before_drawing() {
        let gpu = Gpu::headless().unwrap();
        let load = uploader(&gpu);
        let _list = std::thread::spawn(move || load(&[], &BTreeMap::new()))
            .join()
            .unwrap()
            .unwrap();
        let first = gpu
            .studio
            .get()
            .expect("The worker must prepare the first draw.");
        let load = uploader(&gpu);
        let _list = std::thread::spawn(move || load(&[], &BTreeMap::new()))
            .join()
            .unwrap()
            .unwrap();
        assert!(std::ptr::eq(first, gpu.studio.get().unwrap()));
    }
}
