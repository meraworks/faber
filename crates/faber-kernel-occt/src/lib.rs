//! Exact solid adapter.
mod emboss;
mod projection;

use faber_model::{Geometry, Placement};
pub use faber_ports::SOLID_TRIANGLE_LIMIT;
use faber_ports::{Combine, Kernel, Mesh, PathSample, PathSamples, Properties, Result, Shape};
use glam::{DMat3, DVec3};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    ptr::NonNull,
    sync::{Mutex, MutexGuard, Once, OnceLock},
};

static GATE: Mutex<()> = Mutex::new(());
/// Largest thread count of the OCCT pool. It keeps the machine responsive during booleans.
const THREAD_LIMIT: i32 = 6;

/// Locks the kernel. The first lock caps the OCCT thread pool.
fn gate() -> MutexGuard<'static, ()> {
    let guard = GATE.lock().unwrap_or_else(|e| e.into_inner());
    static LIMIT: Once = Once::new();
    LIMIT.call_once(|| {
        let mut buffer = [0; 1024];
        // A failed cap leaves the OCCT default in place. The kernel still works.
        let _ = unsafe { faber_limit_threads(THREAD_LIMIT, buffer.as_mut_ptr(), buffer.len()) };
    });
    guard
}

/// Threads of the OCCT default pool, after the cap. Used by the tests.
pub fn thread_count() -> usize {
    let _guard = gate();
    usize::try_from(unsafe { faber_thread_count() }).unwrap_or(0)
}

/// OCCT version of the linked library, for example "7.9.3".
fn occt_version() -> &'static str {
    unsafe { CStr::from_ptr(faber_occt_version()) }
        .to_str()
        .unwrap_or("unknown")
}

#[repr(C)]
#[derive(Clone, Default)]
struct RawFaceSource {
    face: u32,
    input: u32,
    source_face: u32,
}

#[repr(C)]
#[derive(Clone, Default)]
struct RawEdgeFace {
    edge: u32,
    face: u32,
}

#[repr(C)]
#[derive(Default)]
struct RawDistance {
    distance: f64,
    point_a: [f64; 3],
    point_b: [f64; 3],
}

#[repr(C)]
struct RawCurveSegment {
    kind: i32,
    data: [f64; 9],
    points: *const f64,
    count: usize,
}

#[repr(C)]
struct RawSegment {
    kind: i32,
    data: [f64; 6],
    points: *const f64,
    count: usize,
}

fn spline_points<const N: usize>(start: [f64; N], points: &[[f64; N]]) -> Result<()> {
    if !(2..=4096).contains(&points.len()) {
        return Err("Spline needs between 2 and 4096 points after its start".into());
    }
    let mut previous = start;
    for point in points {
        if point.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
            return Err("Spline coordinates are outside the supported range".into());
        }
        let distance: f64 = point
            .iter()
            .zip(previous)
            .map(|(a, b)| (a - b).powi(2))
            .sum();
        if distance < 1e-14 {
            return Err("Spline has consecutive equal points".into());
        }
        previous = *point;
    }
    Ok(())
}

fn spline_tangents<const N: usize>(tangents: &[[f64; N]; 2], data: &mut [f64]) -> Result<()> {
    for tangent in tangents {
        if tangent.iter().any(|v| !v.is_finite() || v.abs() > 1e6)
            || tangent.iter().map(|v| v * v).sum::<f64>() < 1e-14
        {
            return Err("Spline tangents must be finite, non-zero and within 1000000".into());
        }
    }
    for (target, value) in data.iter_mut().zip(tangents.iter().flatten()) {
        *target = *value;
    }
    Ok(())
}

fn periodic_spline<const N: usize>(
    start: [f64; N],
    points: &[[f64; N]],
    tangents: bool,
    segments: usize,
) -> Result<()> {
    if tangents || segments != 1 {
        return Err(
            "A periodic spline must be the only segment and cannot set end tangents".into(),
        );
    }
    if points.iter().any(|p| {
        p.iter()
            .zip(start)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            < 1e-14
    }) {
        return Err("Periodic spline points must not repeat the start".into());
    }
    Ok(())
}

#[repr(C)]
#[derive(Clone, Default)]
struct RawFace {
    index: u32,
    area: f64,
    centre: [f64; 3],
    normal: [f64; 3],
    start: [f64; 3],
    axis: [f64; 3],
    radius: f64,
    length: f64,
    internal: i32,
}

#[repr(C)]
#[derive(Clone, Default)]
struct RawEdge {
    index: u32,
    length: f64,
    centre: [f64; 3],
    direction: [f64; 3],
    radius: f64,
}

struct OwnedShape(NonNull<c_void>);

#[repr(C)]
#[derive(Default)]
struct RawEdgeLines {
    owner: *mut c_void,
    points: *const [f64; 3],
    point_count: usize,
    ends: *const u32,
    edge_count: usize,
}

impl Drop for RawEdgeLines {
    fn drop(&mut self) {
        unsafe { faber_free_edge_lines(self.owner) };
    }
}
// Every access and deletion holds GATE.
unsafe impl Send for OwnedShape {}
unsafe impl Sync for OwnedShape {}

impl Drop for OwnedShape {
    fn drop(&mut self) {
        let _guard = gate();
        unsafe {
            faber_free_shape(self.0.as_ptr());
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct RawProperties {
    volume: f64,
    centre: [f64; 3],
    bounds: [f64; 6],
    faces: u32,
    edges: u32,
    solids: u32,
    valid: u32,
}

#[repr(C)]
#[derive(Default)]
struct RawMassProperties {
    volume: f64,
    area: f64,
    centre: [f64; 3],
    inertia: [f64; 9],
}

unsafe extern "C" {
    fn faber_occt_version() -> *const c_char;
    fn faber_limit_threads(count: i32, error: *mut c_char, capacity: usize) -> i32;
    fn faber_thread_count() -> i32;
    fn faber_linear_form(
        body: *const c_void,
        profile: *const c_void,
        direction: *const f64,
        thickness: f64,
        offset: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_inset_face(
        face: *const c_void,
        index: u32,
        distance: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_offset_sketch_face(
        face: *const c_void,
        distance: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_face_shape(
        input: *const c_void,
        index: u32,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_thicken(
        input: *const c_void,
        thickness: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_remove_faces(
        shape: *const c_void,
        faces: *const u32,
        count: usize,
        out: *mut *mut c_void,
        error: *mut c_char,
        len: usize,
    ) -> i32;
    fn faber_move_faces(
        shape: *const c_void,
        faces: *const u32,
        count: usize,
        distance: f64,
        out: *mut *mut c_void,
        error: *mut c_char,
        len: usize,
    ) -> i32;
    fn faber_split_face(
        shape: *const c_void,
        face: u32,
        tool: *const c_void,
        out: *mut *mut c_void,
        error: *mut c_char,
        len: usize,
    ) -> i32;
    fn faber_split(
        input: *const c_void,
        origin: *const f64,
        normal: *const f64,
        keep: i32,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_draft(
        input: *const c_void,
        faces: *const u32,
        count: usize,
        direction: *const f64,
        origin: *const f64,
        normal: *const f64,
        angle: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_offset(
        input: *const c_void,
        distance: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_distance(
        a: *const c_void,
        b: *const c_void,
        result: *mut RawDistance,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_helix(
        radius: f64,
        pitch: f64,
        turns: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_curve(
        start: *const f64,
        segments: *const RawCurveSegment,
        count: usize,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_sweep_shape(
        section: *const c_void,
        path: *const c_void,
        frenet: i32,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_revolve_shape(
        input: *const c_void,
        origin: *const f64,
        axis: *const f64,
        angle: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_loft_shapes(
        inputs: *const *const c_void,
        count: usize,
        ruled: i32,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_sketch_path(
        start: *const f64,
        segments: *const RawSegment,
        count: usize,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_sketch_loop(
        kind: i32,
        data: *const f64,
        count: usize,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_sketch_face(
        wires: *const *const c_void,
        count: usize,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_extrude_shape(
        input: *const c_void,
        vector: *const f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_edge_faces(
        input: *const c_void,
        result: *mut RawEdgeFace,
        capacity_links: usize,
        count: *mut usize,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_face_sources(
        input: *const c_void,
        sources: *const *const c_void,
        source_count: usize,
        result: *mut RawFaceSource,
        capacity_links: usize,
        count: *mut usize,
        known: *mut u32,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_faces(
        input: *const c_void,
        result: *mut RawFace,
        capacity_faces: usize,
        count: *mut usize,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_shell(
        input: *const c_void,
        indices: *const u32,
        count: usize,
        thickness: f64,
        offset_faces: *const u32,
        offset_values: *const f64,
        offset_count: usize,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_edges(
        input: *const c_void,
        result: *mut RawEdge,
        capacity_edges: usize,
        count: *mut usize,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_dress(
        input: *const c_void,
        indices: *const u32,
        count: usize,
        size: f64,
        chamfer: i32,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_chamfer_two(
        input: *const c_void,
        indices: *const u32,
        count: usize,
        first: f64,
        second: f64,
        face: u32,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_chamfer_angle(
        input: *const c_void,
        indices: *const u32,
        count: usize,
        distance: f64,
        angle: f64,
        face: u32,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_fillet_variable(
        input: *const c_void,
        indices: *const u32,
        count: usize,
        start: f64,
        finish: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_sweep(
        profile: *const f64,
        profile_count: usize,
        path: *const f64,
        path_count: usize,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_loft(
        points: *const f64,
        counts: *const usize,
        sections: usize,
        ruled: i32,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_curved(
        kind: i32,
        dimensions: *const f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_revolve(
        points: *const f64,
        counts: *const usize,
        rings: usize,
        origin: *const f64,
        direction: *const f64,
        angle: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_profile(
        points: *const f64,
        counts: *const usize,
        rings: usize,
        height: f64,
        floor: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_solid_from_mesh(
        vertices: *const f64,
        vertex_count: usize,
        triangles: *const u32,
        triangle_count: usize,
        merge_planar: i32,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_box(
        size: *const f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_cylinder(
        radius: f64,
        height: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_transform(
        input: *const c_void,
        matrix: *const f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_scale(
        input: *const c_void,
        centre: *const f64,
        factor: *const f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_path_samples(
        input: *const c_void,
        distances: *const f64,
        count: usize,
        result: *mut f64,
        length: *mut f64,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_combine(
        left: *const c_void,
        right: *const c_void,
        operation: i32,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_properties(
        input: *const c_void,
        result: *mut RawProperties,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_summary_properties(
        input: *const c_void,
        result: *mut RawProperties,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_mass_properties(
        input: *const c_void,
        result: *mut RawMassProperties,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_mesh(
        input: *const c_void,
        tolerance: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_curved_mesh(
        input: *const c_void,
        tolerance: f64,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_mesh_curvatures(
        input: *const c_void,
        curvature: *mut *const [f64; 2],
        nodes: *mut usize,
        kinds: *mut *const u8,
        faces: *mut usize,
    );
    fn faber_mesh_data(
        input: *const c_void,
        vertices: *mut *const f64,
        vertex_count: *mut usize,
        triangles: *mut *const u32,
        triangle_count: *mut usize,
        normals: *mut *const f64,
    );
    fn faber_mesh_face_indices(input: *const c_void) -> *const u32;
    fn faber_edge_lines(
        shape: *const c_void,
        tolerance: f64,
        result: *mut RawEdgeLines,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_free_edge_lines(lines: *mut c_void);
    fn faber_read(
        path: *const c_char,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_write(
        input: *const c_void,
        path: *const c_char,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_free_shape(shape: *mut c_void);
    fn faber_free_mesh(mesh: *mut c_void);
    fn faber_face_count(
        input: *const c_void,
        count: *mut usize,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_encode(
        input: *const c_void,
        data: *mut *mut c_void,
        length: *mut usize,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
    fn faber_free_bytes(data: *mut c_void);
    fn faber_decode(
        data: *const c_void,
        length: usize,
        result: *mut *mut c_void,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
}

fn error(status: i32, buffer: &[c_char; 1024]) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned())
    }
}

fn shape_call(action: impl FnOnce(*mut *mut c_void, *mut c_char, usize) -> i32) -> Result<Shape> {
    let _guard = gate();
    let mut buffer = [0; 1024];
    let mut output = std::ptr::null_mut();
    error(
        action(&mut output, buffer.as_mut_ptr(), buffer.len()),
        &buffer,
    )?;
    let pointer = NonNull::new(output).ok_or("Geometry operation returned no shape")?;
    Ok(Shape::new(OwnedShape(pointer)))
}

fn pointer(shape: &Shape) -> Result<*const c_void> {
    sketch_text::unmarked(shape)
        .get::<OwnedShape>()
        .map(|s| s.0.as_ptr().cast_const())
        .ok_or_else(|| "Shape belongs to another adapter".into())
}

/// Writes a shape as OCCT binary bytes, without a tag.
fn encode_plain(shape: &Shape) -> Result<Vec<u8>> {
    let shape = pointer(shape)?;
    let _guard = gate();
    let mut buffer = [0; 1024];
    let mut data = std::ptr::null_mut();
    let mut length = 0;
    error(
        unsafe {
            faber_encode(
                shape,
                &mut data,
                &mut length,
                buffer.as_mut_ptr(),
                buffer.len(),
            )
        },
        &buffer,
    )?;
    let data = NonNull::new(data).ok_or("Shape encoding returned no data")?;
    let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), length) }.to_vec();
    unsafe { faber_free_bytes(data.as_ptr()) };
    Ok(bytes)
}

/// Reads OCCT binary bytes that `encode_plain` wrote.
fn decode_plain(bytes: &[u8]) -> Result<Shape> {
    shape_call(|out, error, len| unsafe {
        faber_decode(bytes.as_ptr().cast(), bytes.len(), out, error, len)
    })
}

fn check_edges(edges: &[u32]) -> Result<()> {
    if edges.is_empty() {
        return Err("Select at least one edge".into());
    }
    let unique: std::collections::BTreeSet<_> = edges.iter().collect();
    if unique.len() != edges.len() {
        return Err("Edge selection contains duplicates".into());
    }
    Ok(())
}

fn positive(value: f64) -> Result<()> {
    if value.is_finite() && (1e-6..=1e6).contains(&value) {
        Ok(())
    } else {
        Err("Dimension must be between 0.000001 and 1000000 mm".into())
    }
}

fn path_text(path: &Path) -> Result<CString> {
    CString::new(path.to_str().ok_or("Path must be valid text")?)
        .map_err(|_| "Path contains a zero byte".into())
}

use faber_geometry::sketch_loops::rounded;
use faber_geometry::sketch_loops::sketch_arrays;
mod sketch_text;
use faber_geometry::sketch_loops::slot;

pub struct OcctKernel;

impl OcctKernel {
    fn mesh_with_curvature(
        shape: &Shape,
        tolerance: f64,
        curved: bool,
    ) -> Result<faber_ports::CurvedMesh> {
        positive(tolerance)?;
        let shape = pointer(shape)?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        let mut raw = std::ptr::null_mut();
        let make = if curved {
            faber_curved_mesh
        } else {
            faber_mesh
        };
        error(
            unsafe {
                make(
                    shape,
                    tolerance,
                    &mut raw,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            },
            &buffer,
        )?;
        let raw = NonNull::new(raw).ok_or("Geometry operation returned no mesh")?;
        struct Guard(NonNull<c_void>);
        impl Drop for Guard {
            fn drop(&mut self) {
                unsafe {
                    faber_free_mesh(self.0.as_ptr());
                }
            }
        }
        let raw = Guard(raw);
        let (mut vertices, mut triangles) = (std::ptr::null(), std::ptr::null());
        let mut normals = std::ptr::null();
        let (mut vertex_count, mut triangle_count) = (0, 0);
        unsafe {
            faber_mesh_data(
                raw.0.as_ptr(),
                &mut vertices,
                &mut vertex_count,
                &mut triangles,
                &mut triangle_count,
                &mut normals,
            );
        }
        if vertex_count == 0 || triangle_count == 0 {
            return Err("Shape has no mesh faces".into());
        }
        let points = unsafe { std::slice::from_raw_parts(vertices, vertex_count * 3) };
        let indices = unsafe { std::slice::from_raw_parts(triangles, triangle_count * 3) };
        let faces = unsafe { faber_mesh_face_indices(raw.0.as_ptr()) };
        let mesh = Mesh {
            normals: unsafe { std::slice::from_raw_parts(normals, vertex_count * 3) }
                .as_chunks::<3>()
                .0
                .to_vec(),
            vertices: points.as_chunks::<3>().0.to_vec(),
            triangles: indices.as_chunks::<3>().0.to_vec(),
        };
        let faces = faber_ports::FaceMesh {
            mesh,
            face_of_triangle: unsafe { std::slice::from_raw_parts(faces, triangle_count) }.to_vec(),
        };
        let mut result = faber_ports::CurvedMesh {
            faces,
            curvature: Vec::new(),
            kinds: Default::default(),
        };
        if curved {
            let (mut curvature, mut kinds) = (std::ptr::null(), std::ptr::null());
            let (mut nodes, mut count) = (0, 0);
            unsafe {
                faber_mesh_curvatures(
                    raw.0.as_ptr(),
                    &mut curvature,
                    &mut nodes,
                    &mut kinds,
                    &mut count,
                );
            }
            if nodes != vertex_count || count == 0 || count > u32::MAX as usize {
                return Err("Surface samples do not match the face mesh.".into());
            }
            result.curvature = unsafe { std::slice::from_raw_parts(curvature, nodes) }.to_vec();
            for (i, &kind) in unsafe { std::slice::from_raw_parts(kinds, count) }
                .iter()
                .enumerate()
            {
                use faber_ports::SurfaceKind;
                let kind = match kind {
                    0 => SurfaceKind::Plane,
                    1 => SurfaceKind::Cylinder,
                    2 => SurfaceKind::Sphere,
                    3 => SurfaceKind::Cone,
                    4 => SurfaceKind::Torus,
                    5 => SurfaceKind::Freeform,
                    _ => return Err("The face has an unknown surface type.".into()),
                };
                result.kinds.insert(i as u32 + 1, kind);
            }
        }
        Ok(result)
    }

    fn dress(&self, shape: &Shape, edges: &[u32], size: f64, chamfer: bool) -> Result<Shape> {
        positive(size)?;
        check_edges(edges)?;
        let shape = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_dress(
                shape,
                edges.as_ptr(),
                edges.len(),
                size,
                i32::from(chamfer),
                out,
                error,
                len,
            )
        })
    }
}

impl Kernel for OcctKernel {
    fn project_edges(
        &self,
        shapes: &[Shape],
        frame: &faber_ports::drawing::ViewFrame,
        hidden: bool,
    ) -> Result<faber_ports::drawing::Projection2> {
        projection::project(shapes, frame, hidden)
    }
    fn section_face(&self, shape: &Shape, origin: [f64; 3], normal: [f64; 3]) -> Result<Shape> {
        projection::section(shape, origin, normal)
    }
    fn solid_from_mesh(&self, mesh: &Mesh, merge_planar: bool) -> Result<Shape> {
        let count = mesh.triangles.len();
        if count > SOLID_TRIANGLE_LIMIT {
            return Err(format!(
                "The mesh has {count} triangles. The limit for a solid is {SOLID_TRIANGLE_LIMIT}. Reduce the mesh first."
            ));
        }
        if count == 0 {
            return Err("A solid needs a closed mesh with faces.".into());
        }
        let (points, map) = mesh.welded_vertices(1e-6)?;
        let mut edges = std::collections::HashMap::new();
        let mut triangles = Vec::with_capacity(count);
        for face in &mesh.triangles {
            let triangle = face.map(|i| map[i as usize]);
            let [a, b, c] = triangle.map(|i| DVec3::from_array(points[i as usize]));
            let area = (b - a).cross(c - a).length_squared();
            if !area.is_finite() || area <= 1e-28 {
                return Err("The mesh contains a degenerate face. Repair the mesh first.".into());
            }
            for (i, j) in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                let (n, direction) = edges.entry((i.min(j), i.max(j))).or_insert((0_u32, 0_i32));
                *n += 1;
                *direction += if i < j { 1 } else { -1 };
            }
            triangles.push(triangle);
        }
        if edges.values().any(|&(n, d)| n != 2 || d != 0) {
            return Err("A solid needs a closed manifold mesh with consistent winding. Repair the mesh first.".into());
        }
        shape_call(|out, error, len| unsafe {
            faber_solid_from_mesh(
                points.as_ptr().cast(),
                points.len(),
                triangles.as_ptr().cast(),
                count,
                i32::from(merge_planar),
                out,
                error,
                len,
            )
        })
    }
    fn face_sources(
        &self,
        shape: &Shape,
        inputs: &[Shape],
    ) -> Result<Option<Vec<faber_ports::FaceSource>>> {
        let output = pointer(shape)?;
        let sources = inputs.iter().map(pointer).collect::<Result<Vec<_>>>()?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        let mut count = 0;
        let mut known = 0;
        error(
            unsafe {
                faber_face_sources(
                    output,
                    sources.as_ptr(),
                    sources.len(),
                    std::ptr::null_mut(),
                    0,
                    &mut count,
                    &mut known,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            },
            &buffer,
        )?;
        if known == 0 {
            return Ok(None);
        }
        let mut links = vec![RawFaceSource::default(); count];
        error(
            unsafe {
                faber_face_sources(
                    output,
                    sources.as_ptr(),
                    sources.len(),
                    links.as_mut_ptr(),
                    links.len(),
                    &mut count,
                    &mut known,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            },
            &buffer,
        )?;
        links.truncate(count);
        Ok(Some(
            links
                .into_iter()
                .map(|link| faber_ports::FaceSource {
                    face: link.face,
                    input: link.input as usize,
                    source_face: link.source_face,
                })
                .collect(),
        ))
    }
    fn edge_faces(&self, shape: &Shape) -> Result<Vec<faber_ports::EdgeFace>> {
        let input = pointer(shape)?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        let mut count = 0;
        error(
            unsafe {
                faber_edge_faces(
                    input,
                    std::ptr::null_mut(),
                    0,
                    &mut count,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            },
            &buffer,
        )?;
        let mut links = vec![RawEdgeFace::default(); count];
        error(
            unsafe {
                faber_edge_faces(
                    input,
                    links.as_mut_ptr(),
                    links.len(),
                    &mut count,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            },
            &buffer,
        )?;
        links.truncate(count);
        Ok(links
            .into_iter()
            .map(|link| faber_ports::EdgeFace {
                edge: link.edge,
                face: link.face,
            })
            .collect())
    }
    fn split(
        &self,
        shape: &Shape,
        origin: [f64; 3],
        normal: [f64; 3],
        keep: faber_model::SplitKeep,
    ) -> Result<Shape> {
        if origin
            .iter()
            .chain(&normal)
            .any(|v| !v.is_finite() || v.abs() > 1e6)
            || DVec3::from(normal).length() < 1e-10
        {
            return Err("Split needs a finite origin and a non-zero plane normal".into());
        }
        let info = self.properties(shape)?;
        if !info.valid || info.solids == 0 || info.volume <= 0. {
            return Err("Split needs a valid solid".into());
        }
        let input = pointer(shape)?;
        let keep = match keep {
            faber_model::SplitKeep::Both => 0,
            faber_model::SplitKeep::Positive => 1,
            faber_model::SplitKeep::Negative => -1,
        };
        shape_call(|out, error, len| unsafe {
            faber_split(
                input,
                origin.as_ptr(),
                normal.as_ptr(),
                keep,
                out,
                error,
                len,
            )
        })
    }
    fn remove_faces(&self, shape: &Shape, faces: &[u32]) -> Result<Shape> {
        let input = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_remove_faces(input, faces.as_ptr(), faces.len(), out, error, len)
        })
    }
    fn move_faces(&self, shape: &Shape, faces: &[u32], distance: f64) -> Result<Shape> {
        positive(distance.abs())?;
        let input = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_move_faces(
                input,
                faces.as_ptr(),
                faces.len(),
                distance,
                out,
                error,
                len,
            )
        })
    }
    fn split_face(&self, shape: &Shape, face: u32, tool: &Shape) -> Result<Shape> {
        let input = pointer(shape)?;
        let cutter = pointer(tool)?;
        shape_call(|out, error, len| unsafe {
            faber_split_face(input, face, cutter, out, error, len)
        })
    }
    fn linear_form(
        &self,
        body: &Shape,
        profile: &Shape,
        direction: [f64; 3],
        thickness: f64,
        offset: f64,
    ) -> Result<Shape> {
        positive(thickness)?;
        if !(-1.0..=1.0).contains(&offset) || DVec3::from(direction).try_normalize().is_none() {
            return Err("Invalid wall direction or side offset".into());
        }
        let (body, profile) = (pointer(body)?, pointer(profile)?);
        shape_call(|out, error, len| unsafe {
            faber_linear_form(
                body,
                profile,
                direction.as_ptr(),
                thickness,
                offset,
                out,
                error,
                len,
            )
        })
    }
    fn inset_face(&self, face: &Shape, index: u32, distance: f64) -> Result<Shape> {
        if !distance.is_finite() || distance.abs() > 1e6 {
            return Err("The loop offset exceeds the supported range".into());
        }
        let face = pointer(face)?;
        shape_call(|out, error, len| unsafe {
            faber_inset_face(face, index, distance, out, error, len)
        })
    }
    fn offset_sketch_face(&self, face: &Shape, distance: f64) -> Result<Shape> {
        if !distance.is_finite() || distance.abs() > 1e6 {
            return Err("The loop offset exceeds the supported range".into());
        }
        let face = pointer(face)?;
        shape_call(|out, error, len| unsafe {
            faber_offset_sketch_face(face, distance, out, error, len)
        })
    }
    fn face_shape(&self, shape: &Shape, index: u32) -> Result<Shape> {
        let input = pointer(shape)?;
        shape_call(|out, error, len| unsafe { faber_face_shape(input, index, out, error, len) })
    }
    fn wrap_on_cylinder(
        &self,
        flat: &Shape,
        cylinder: &faber_ports::CylinderFace,
        at: [f64; 2],
    ) -> Result<Shape> {
        emboss::wrap(flat, cylinder, at)
    }
    fn thicken(&self, shape: &Shape, thickness: f64) -> Result<Shape> {
        positive(thickness.abs())?;
        let info = self.properties(shape)?;
        if !info.valid || info.solids != 0 || info.faces == 0 {
            return Err("Thicken needs a face or open surface".into());
        }
        let input = pointer(shape)?;
        let result = shape_call(|out, error, len| unsafe {
            faber_thicken(input, thickness, out, error, len)
        })?;
        let info = self.properties(&result)?;
        if !info.valid || info.solids == 0 || info.volume <= 0. {
            return Err("Thicken did not produce a valid solid".into());
        }
        Ok(result)
    }
    fn draft(
        &self,
        shape: &Shape,
        faces: &[u32],
        direction: [f64; 3],
        origin: [f64; 3],
        normal: [f64; 3],
        angle: f64,
    ) -> Result<Shape> {
        if !angle.is_finite() || angle == 0. || angle.abs() >= 90. {
            return Err("Draft angle must be non-zero and between -90 and 90 degrees".into());
        }
        if direction
            .iter()
            .chain(&origin)
            .chain(&normal)
            .any(|v| !v.is_finite() || v.abs() > 1e6)
            || DVec3::from(direction).length() < 1e-10
            || DVec3::from(normal).length() < 1e-10
        {
            return Err(
                "Draft needs a finite origin and non-zero direction and plane normal".into(),
            );
        }
        if faces.is_empty()
            || faces
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != faces.len()
        {
            return Err("Draft needs a non-empty set of unique faces".into());
        }
        let info = self.properties(shape)?;
        if !info.valid || info.solids == 0 || info.volume <= 0. {
            return Err("Draft needs a valid solid".into());
        }
        let input = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_draft(
                input,
                faces.as_ptr(),
                faces.len(),
                direction.as_ptr(),
                origin.as_ptr(),
                normal.as_ptr(),
                angle.to_radians(),
                out,
                error,
                len,
            )
        })
    }
    fn offset(&self, shape: &Shape, distance: f64) -> Result<Shape> {
        positive(distance.abs())?;
        let info = self.properties(shape)?;
        if !info.valid || info.solids == 0 || info.volume <= 0. {
            return Err("Offset needs a valid solid".into());
        }
        let input = pointer(shape)?;
        let result = shape_call(|out, error, len| unsafe {
            faber_offset(input, distance, out, error, len)
        })?;
        let output = self.properties(&result)?;
        if !output.valid || output.solids == 0 || output.volume <= 0. {
            return Err("Offset did not produce a valid solid".into());
        }
        Ok(result)
    }
    fn distance(&self, a: &Shape, b: &Shape) -> Result<faber_ports::Distance> {
        let a = pointer(a)?;
        let b = pointer(b)?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        let mut data = RawDistance::default();
        error(
            unsafe { faber_distance(a, b, &mut data, buffer.as_mut_ptr(), buffer.len()) },
            &buffer,
        )?;
        if !data.distance.is_finite()
            || data.distance < 0.
            || data
                .point_a
                .iter()
                .chain(&data.point_b)
                .any(|v| !v.is_finite())
        {
            return Err("Distance measurement returned invalid values".into());
        }
        Ok(faber_ports::Distance {
            distance_mm: data.distance,
            point_a: data.point_a,
            point_b: data.point_b,
        })
    }
    fn helix(&self, radius: f64, pitch: f64, turns: f64) -> Result<Shape> {
        positive(radius)?;
        positive(pitch.abs())?;
        if !turns.is_finite() || turns <= 0. || turns > 100. || (pitch * turns).abs() > 1e6 {
            return Err(
                "Helix needs 0 to 100 turns, excluding zero, and height at most 1000000 mm".into(),
            );
        }
        shape_call(|out, error, len| unsafe { faber_helix(radius, pitch, turns, out, error, len) })
    }
    fn curve(&self, path: &faber_model::CurvePath) -> Result<Shape> {
        use faber_model::CurveSegment;
        if !(1..=4096).contains(&path.segments.len()) {
            return Err("Curve needs between 1 and 4096 segments".into());
        }
        let mut raw = Vec::new();
        let mut previous = path.start;
        for segment in &path.segments {
            let mut data = [0.; 9];
            let mut points = std::ptr::null();
            let mut count = 0;
            let (kind, end) = match segment {
                CurveSegment::Spline {
                    points: values,
                    tangents,
                    periodic,
                } => {
                    spline_points(previous, values)?;
                    points = values.as_ptr().cast();
                    count = values.len();
                    if let Some(tangents) = tangents {
                        spline_tangents(tangents, &mut data)?;
                    }
                    if *periodic {
                        periodic_spline(previous, values, tangents.is_some(), path.segments.len())?;
                        (5, &path.start)
                    } else {
                        (if tangents.is_some() { 4 } else { 3 }, &values[count - 1])
                    }
                }
                CurveSegment::Line { end } => {
                    data[..3].copy_from_slice(end);
                    (0, end)
                }
                CurveSegment::Arc { via, end } => {
                    data[..3].copy_from_slice(via);
                    data[3..6].copy_from_slice(end);
                    (1, end)
                }
                CurveSegment::Bezier {
                    control1,
                    control2,
                    end,
                } => {
                    data[..3].copy_from_slice(control1);
                    data[3..6].copy_from_slice(control2);
                    data[6..].copy_from_slice(end);
                    (2, end)
                }
            };
            if kind != 5 && glam::DVec3::from(previous).distance(glam::DVec3::from(*end)) < 1e-7 {
                return Err("Curve segment has equal endpoints".into());
            }
            raw.push(RawCurveSegment {
                kind,
                data,
                points,
                count,
            });
            previous = *end;
        }
        if path
            .start
            .iter()
            .chain(raw.iter().flat_map(|s| &s.data))
            .any(|v| !v.is_finite() || v.abs() > 1e6)
        {
            return Err("Curve coordinates are outside the supported range".into());
        }
        shape_call(|out, error, len| unsafe {
            faber_curve(
                path.start.as_ptr(),
                raw.as_ptr(),
                raw.len(),
                out,
                error,
                len,
            )
        })
    }
    fn sweep_shape(&self, section: &Shape, path: &Shape, frenet: bool) -> Result<Shape> {
        let section = pointer(section)?;
        let path = pointer(path)?;
        shape_call(|out, error, len| unsafe {
            faber_sweep_shape(section, path, i32::from(frenet), out, error, len)
        })
    }
    fn path_samples(&self, path: &Shape, distances: &[f64]) -> Result<PathSamples> {
        if distances
            .iter()
            .any(|d| !d.is_finite() || *d < 0. || *d > 1e6)
        {
            return Err("Path distance is outside the supported range".into());
        }
        let path = pointer(path)?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        let mut values = vec![0.; distances.len() * 6];
        let mut length = 0.;
        error(
            unsafe {
                faber_path_samples(
                    path,
                    distances.as_ptr(),
                    distances.len(),
                    values.as_mut_ptr(),
                    &mut length,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            },
            &buffer,
        )?;
        let samples = values
            .as_chunks::<6>()
            .0
            .iter()
            .map(|v| PathSample {
                point: [v[0], v[1], v[2]],
                tangent: [v[3], v[4], v[5]],
            })
            .collect();
        Ok(PathSamples { length, samples })
    }
    fn scale_shape(&self, shape: &Shape, about: [f64; 3], factor: [f64; 3]) -> Result<Shape> {
        if about.iter().any(|v| !v.is_finite() || v.abs() > 1e6)
            || factor.iter().any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err("Scale centre or factor is outside the supported range".into());
        }
        let shape = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_scale(shape, about.as_ptr(), factor.as_ptr(), out, error, len)
        })
    }
    fn revolve_shape(
        &self,
        shape: &Shape,
        origin: [f64; 3],
        axis: [f64; 3],
        angle: f64,
    ) -> Result<Shape> {
        if !angle.is_finite() || angle <= 0. || angle > 360. {
            return Err("Revolve angle must be greater than zero and at most 360 degrees".into());
        }
        if origin
            .iter()
            .chain(&axis)
            .any(|v| !v.is_finite() || v.abs() > 1e6)
            || glam::DVec3::from(axis).length() < 1e-10
        {
            return Err("Revolve needs a finite origin and a non-zero axis".into());
        }
        let shape = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_revolve_shape(
                shape,
                origin.as_ptr(),
                axis.as_ptr(),
                angle.to_radians(),
                out,
                error,
                len,
            )
        })
    }
    fn loft_shapes(&self, sections: &[Shape], ruled: bool) -> Result<Shape> {
        if !(2..=256).contains(&sections.len()) {
            return Err("Loft needs between 2 and 256 sketches".into());
        }
        let pointers = sections.iter().map(pointer).collect::<Result<Vec<_>>>()?;
        shape_call(|out, error, len| unsafe {
            faber_loft_shapes(
                pointers.as_ptr(),
                pointers.len(),
                i32::from(ruled),
                out,
                error,
                len,
            )
        })
    }
    fn sketch(&self, profile: &faber_model::SketchProfile, placement: &Placement) -> Result<Shape> {
        use faber_model::SketchLoop;
        let profile = sketch_arrays::expand(profile)?;
        if matches!(profile.outer, SketchLoop::Text { .. }) {
            return sketch_text::sketch(self, &profile, placement);
        }
        if profile.holes.len() > 256 {
            return Err("Sketch has more than 256 holes".into());
        }
        let mut wires = Vec::new();
        for boundary in std::iter::once(&profile.outer).chain(&profile.holes) {
            let rounded = rounded::lower(boundary)?;
            let boundary = rounded.as_ref().unwrap_or(boundary);
            let slot;
            let boundary = if let SketchLoop::Slot {
                centre,
                length,
                width,
                rotation,
            } = boundary
            {
                slot = slot::boundary(*centre, *length, *width, *rotation)?;
                &slot
            } else {
                boundary
            };
            if let SketchLoop::Path { start, segments } = boundary {
                use faber_model::SketchSegment;
                if !(1..=4096).contains(&segments.len()) {
                    return Err("Sketch path needs between 1 and 4096 segments".into());
                }
                let mut raw = Vec::new();
                let mut last = *start;
                for segment in segments {
                    let mut points = std::ptr::null();
                    let mut count = 0;
                    let (kind, data, end) = match segment {
                        SketchSegment::Spline {
                            points: values,
                            tangents,
                            periodic,
                        } => {
                            spline_points(last, values)?;
                            points = values.as_ptr().cast();
                            count = values.len();
                            let mut data = [0.; 6];
                            if let Some(tangents) = tangents {
                                spline_tangents(tangents, &mut data)?;
                            }
                            if *periodic {
                                periodic_spline(last, values, tangents.is_some(), segments.len())?;
                                (5, data, *start)
                            } else {
                                (
                                    if tangents.is_some() { 4 } else { 3 },
                                    data,
                                    values[count - 1],
                                )
                            }
                        }
                        SketchSegment::Line { end } => (0, [end[0], end[1], 0., 0., 0., 0.], *end),
                        SketchSegment::Arc { via, end } => {
                            (1, [via[0], via[1], end[0], end[1], 0., 0.], *end)
                        }
                        SketchSegment::Bezier {
                            control1: a,
                            control2: b,
                            end,
                        } => (2, [a[0], a[1], b[0], b[1], end[0], end[1]], *end),
                    };
                    if kind != 5 && glam::DVec2::from(last).distance(glam::DVec2::from(end)) < 1e-7
                    {
                        return Err("Sketch segment has equal endpoints".into());
                    }
                    raw.push(RawSegment {
                        kind,
                        data,
                        points,
                        count,
                    });
                    last = end;
                }
                if start
                    .iter()
                    .chain(raw.iter().flat_map(|s| &s.data))
                    .any(|v| !v.is_finite() || v.abs() > 1e6)
                {
                    return Err("Sketch coordinates are outside the supported range".into());
                }
                if glam::DVec2::from(last).distance(glam::DVec2::from(*start)) > 1e-7 {
                    return Err("Sketch path must end at its start point".into());
                }
                wires.push(shape_call(|out, error, len| unsafe {
                    faber_sketch_path(start.as_ptr(), raw.as_ptr(), raw.len(), out, error, len)
                })?);
                continue;
            }
            let (kind, data, count) = match boundary {
                SketchLoop::Ellipse {
                    centre,
                    major_radius,
                    minor_radius,
                    rotation,
                } => {
                    positive(*major_radius)?;
                    positive(*minor_radius)?;
                    if minor_radius > major_radius {
                        return Err("Ellipse major radius must be at least its minor radius".into());
                    }
                    (
                        2,
                        vec![
                            centre[0],
                            centre[1],
                            *major_radius,
                            *minor_radius,
                            rotation.to_radians(),
                        ],
                        0,
                    )
                }
                SketchLoop::Circle { centre, radius } => {
                    positive(*radius)?;
                    (0, vec![centre[0], centre[1], *radius], 0)
                }
                other => {
                    let points = match other {
                        SketchLoop::RegularPolygon {
                            centre,
                            radius,
                            sides,
                            rotation,
                            ..
                        } => rounded::regular_points(*centre, *radius, *sides, *rotation)?,
                        SketchLoop::Rectangle {
                            origin: [x, y],
                            size: [w, h],
                        } => {
                            positive(*w)?;
                            positive(*h)?;
                            vec![[*x, *y], [x + w, *y], [x + w, y + h], [*x, y + h]]
                        }
                        SketchLoop::Polygon { points, .. } => points.clone(),
                        _ => unreachable!(),
                    };
                    faber_geometry::NativeKernel.build(&Geometry::Extrude {
                        profile: faber_model::Profile {
                            outer: points.clone(),
                            holes: vec![],
                        },
                        height: 1.,
                    })?;
                    let count = points.len();
                    (1, points.into_iter().flatten().collect(), count)
                }
            };
            if data.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
                return Err("Sketch coordinates are outside the supported range".into());
            }
            wires.push(shape_call(|out, error, len| unsafe {
                faber_sketch_loop(kind, data.as_ptr(), count, out, error, len)
            })?);
        }
        let pointers = wires.iter().map(pointer).collect::<Result<Vec<_>>>()?;
        let face = shape_call(|out, error, len| unsafe {
            faber_sketch_face(pointers.as_ptr(), pointers.len(), out, error, len)
        })?;
        self.transform_shape(&face, placement)
    }
    fn extrude_shape(&self, shape: &Shape, vector: [f64; 3]) -> Result<Shape> {
        if vector.iter().any(|v| !v.is_finite() || v.abs() > 1e6)
            || glam::DVec3::from(vector).length() < 1e-6
        {
            return Err("Extrusion vector must be finite and non-zero".into());
        }
        if let Some(solids) = sketch_text::extrude_regions(self, shape, vector)? {
            return Ok(solids);
        }
        let shape = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_extrude_shape(shape, vector.as_ptr(), out, error, len)
        })
    }
    fn faces(&self, shape: &Shape) -> Result<Vec<faber_ports::FaceInfo>> {
        let origins = shape.face_origins();
        let mut faces = vec![RawFace::default(); self.face_count(shape)? as usize];
        let shape = pointer(shape)?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        let mut count = 0;
        error(
            unsafe {
                faber_faces(
                    shape,
                    faces.as_mut_ptr(),
                    faces.len(),
                    &mut count,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            },
            &buffer,
        )?;
        faces.truncate(count);
        Ok(faces
            .into_iter()
            .map(|f| faber_ports::FaceInfo {
                cylinder: (f.radius > 0.).then_some(faber_ports::CylinderFace {
                    start: f.start,
                    axis: f.axis,
                    radius: f.radius,
                    length: f.length,
                    internal: f.internal != 0,
                }),
                index: f.index,
                created_by: origins.map(|origins| {
                    origins
                        .get(&f.index)
                        .map(|names| names.iter().cloned().collect())
                        .unwrap_or_default()
                }),
                area: f.area,
                centre: f.centre,
                normal: (f.normal != [0.; 3]).then_some(f.normal),
            })
            .collect())
    }
    fn shell(
        &self,
        shape: &Shape,
        faces: &[u32],
        thickness: f64,
        offsets: &[(u32, f64)],
    ) -> Result<Shape> {
        positive(thickness)?;
        for (_, value) in offsets {
            positive(*value)?;
        }
        if faces.is_empty() {
            return Err("Select at least one face to remove".into());
        }
        if faces
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != faces.len()
        {
            return Err("Face selection contains duplicates".into());
        }
        let offset_faces: Vec<u32> = offsets.iter().map(|(face, _)| *face).collect();
        let offset_values: Vec<f64> = offsets.iter().map(|(_, value)| *value).collect();
        let shape = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_shell(
                shape,
                faces.as_ptr(),
                faces.len(),
                thickness,
                offset_faces.as_ptr(),
                offset_values.as_ptr(),
                offsets.len(),
                out,
                error,
                len,
            )
        })
    }
    fn edges(&self, shape: &Shape) -> Result<Vec<faber_ports::EdgeInfo>> {
        let mut edges = vec![RawEdge::default(); self.summary_properties(shape)?.edges as usize];
        let shape = pointer(shape)?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        let mut count = 0;
        error(
            unsafe {
                faber_edges(
                    shape,
                    edges.as_mut_ptr(),
                    edges.len(),
                    &mut count,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            },
            &buffer,
        )?;
        edges.truncate(count);
        Ok(edges
            .into_iter()
            .map(|e| faber_ports::EdgeInfo {
                index: e.index,
                length: e.length,
                centre: e.centre,
                direction: (e.direction != [0.; 3]).then_some(e.direction),
                radius: (e.radius >= 0.).then_some(e.radius),
            })
            .collect())
    }
    fn fillet(&self, shape: &Shape, edges: &[u32], radius: f64) -> Result<Shape> {
        self.dress(shape, edges, radius, false)
    }
    fn chamfer(&self, shape: &Shape, edges: &[u32], distance: f64) -> Result<Shape> {
        self.dress(shape, edges, distance, true)
    }
    fn fillet_variable(&self, shape: &Shape, edges: &[u32], start: f64, end: f64) -> Result<Shape> {
        positive(start)?;
        positive(end)?;
        check_edges(edges)?;
        let shape = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_fillet_variable(
                shape,
                edges.as_ptr(),
                edges.len(),
                start,
                end,
                out,
                error,
                len,
            )
        })
    }
    fn chamfer_two(
        &self,
        shape: &Shape,
        edges: &[u32],
        first: f64,
        second: f64,
        face: Option<u32>,
    ) -> Result<Shape> {
        positive(first)?;
        positive(second)?;
        check_edges(edges)?;
        let shape = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_chamfer_two(
                shape,
                edges.as_ptr(),
                edges.len(),
                first,
                second,
                face.unwrap_or(0),
                out,
                error,
                len,
            )
        })
    }
    fn chamfer_angle(
        &self,
        shape: &Shape,
        edges: &[u32],
        distance: f64,
        angle_deg: f64,
        face: Option<u32>,
    ) -> Result<Shape> {
        positive(distance)?;
        if !(angle_deg > 0. && angle_deg < 90.) {
            return Err("Chamfer angle must be between 0 and 90 degrees".into());
        }
        check_edges(edges)?;
        let shape = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_chamfer_angle(
                shape,
                edges.as_ptr(),
                edges.len(),
                distance,
                angle_deg.to_radians(),
                face.unwrap_or(0),
                out,
                error,
                len,
            )
        })
    }
    fn sweep(
        &self,
        profile: &faber_model::Profile,
        placement: &Placement,
        path: &[[f64; 3]],
    ) -> Result<Shape> {
        if !(2..=4096).contains(&path.len()) {
            return Err("Sweep path needs between 2 and 4096 points".into());
        }
        if path
            .iter()
            .flatten()
            .chain(&placement.translation)
            .chain(&placement.rotation)
            .any(|v| !v.is_finite() || v.abs() > 1e6)
        {
            return Err("Sweep coordinates are outside the supported range".into());
        }
        for segment in path.windows(2) {
            if glam::DVec3::from(segment[0]).distance(glam::DVec3::from(segment[1])) < 1e-6 {
                return Err("Sweep path has a zero-length segment".into());
            }
        }
        let [x, y, z] = placement.rotation.map(f64::to_radians);
        let rotation =
            DMat3::from_rotation_z(z) * DMat3::from_rotation_y(y) * DMat3::from_rotation_x(x);
        let translation = glam::DVec3::from(placement.translation);
        let tangent = (glam::DVec3::from(path[1]) - glam::DVec3::from(path[0])).normalize();
        if translation.distance(glam::DVec3::from(path[0])) > 1e-6
            || (rotation * glam::DVec3::Z).dot(tangent).abs() < 1. - 1e-8
        {
            return Err(
                "Sweep profile must start at the path origin and be normal to its first segment"
                    .into(),
            );
        }
        faber_geometry::NativeKernel.build(&Geometry::Extrude {
            profile: profile.clone(),
            height: 1.,
        })?;
        let path_points: Vec<f64> = path.iter().flatten().copied().collect();
        let make = |ring: &[[f64; 2]]| -> Result<Shape> {
            let area: f64 = ring
                .iter()
                .zip(ring.iter().cycle().skip(1))
                .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
                .sum();
            let mut points = Vec::with_capacity(ring.len() * 3);
            for i in 0..ring.len() {
                let point = ring[if area > 0. { i } else { ring.len() - 1 - i }];
                points.extend(
                    (rotation * glam::DVec3::new(point[0], point[1], 0.) + translation).to_array(),
                );
            }
            shape_call(|out, error, len| unsafe {
                faber_sweep(
                    points.as_ptr(),
                    ring.len(),
                    path_points.as_ptr(),
                    path.len(),
                    out,
                    error,
                    len,
                )
            })
        };
        let mut shape = make(&profile.outer)?;
        for hole in &profile.holes {
            shape = self.combine(&shape, &make(hole)?, Combine::Difference)?;
        }
        Ok(shape)
    }
    fn loft(&self, sections: &[(faber_model::Profile, Placement)], ruled: bool) -> Result<Shape> {
        if !(2..=256).contains(&sections.len()) {
            return Err("Loft needs between 2 and 256 sections".into());
        }
        let holes = sections[0].0.holes.len();
        for (profile, placement) in sections {
            if profile.holes.len() != holes {
                return Err("Loft sections must have the same hole count".into());
            }
            if placement
                .translation
                .iter()
                .chain(&placement.rotation)
                .any(|v| !v.is_finite() || v.abs() > 1e6)
            {
                return Err("Loft placement is outside the supported range".into());
            }
            faber_geometry::NativeKernel.build(&Geometry::Extrude {
                profile: profile.clone(),
                height: 1.,
            })?;
        }
        let make = |ring_index: usize| -> Result<Shape> {
            let mut counts = Vec::new();
            let mut points = Vec::new();
            for (profile, placement) in sections {
                let ring = if ring_index == 0 {
                    &profile.outer
                } else {
                    &profile.holes[ring_index - 1]
                };
                let area: f64 = ring
                    .iter()
                    .zip(ring.iter().cycle().skip(1))
                    .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
                    .sum();
                let [x, y, z] = placement.rotation.map(f64::to_radians);
                let rotation = DMat3::from_rotation_z(z)
                    * DMat3::from_rotation_y(y)
                    * DMat3::from_rotation_x(x);
                let translation = glam::DVec3::from(placement.translation);
                counts.push(ring.len());
                for i in 0..ring.len() {
                    let point = ring[if area > 0. { i } else { ring.len() - 1 - i }];
                    points.extend(
                        (rotation * glam::DVec3::new(point[0], point[1], 0.) + translation)
                            .to_array(),
                    );
                }
            }
            shape_call(|out, error, len| unsafe {
                faber_loft(
                    points.as_ptr(),
                    counts.as_ptr(),
                    counts.len(),
                    i32::from(ruled),
                    out,
                    error,
                    len,
                )
            })
        };
        let mut shape = make(0)?;
        for hole in 1..=holes {
            shape = self.combine(&shape, &make(hole)?, Combine::Difference)?;
        }
        Ok(shape)
    }
    fn sphere_shape(&self, radius: f64) -> Result<Shape> {
        positive(radius)?;
        shape_call(|out, error, len| unsafe {
            faber_curved(0, [radius, 0., 0.].as_ptr(), out, error, len)
        })
    }
    fn cone_shape(&self, base_radius: f64, top_radius: f64, height: f64) -> Result<Shape> {
        positive(height)?;
        for radius in [base_radius, top_radius] {
            if radius != 0. {
                positive(radius)?;
            }
        }
        if base_radius == top_radius {
            if base_radius == 0. {
                return Err("Cone needs a positive radius".into());
            }
            return self.cylinder_shape(base_radius, height);
        }
        shape_call(|out, error, len| unsafe {
            faber_curved(
                1,
                [base_radius, top_radius, height].as_ptr(),
                out,
                error,
                len,
            )
        })
    }
    fn torus_shape(&self, major_radius: f64, minor_radius: f64) -> Result<Shape> {
        positive(major_radius)?;
        positive(minor_radius)?;
        if major_radius <= minor_radius {
            return Err("Major radius must exceed minor radius".into());
        }
        shape_call(|out, error, len| unsafe {
            faber_curved(
                2,
                [major_radius, minor_radius, 0.].as_ptr(),
                out,
                error,
                len,
            )
        })
    }
    fn revolve(
        &self,
        profile: &faber_model::Profile,
        origin: [f64; 3],
        axis: [f64; 3],
        angle: f64,
    ) -> Result<Shape> {
        if !angle.is_finite() || angle <= 0. || angle > 360. {
            return Err("Revolve angle must be greater than zero and at most 360 degrees".into());
        }
        if origin
            .iter()
            .chain(&axis)
            .any(|v| !v.is_finite() || v.abs() > 1e6)
            || glam::DVec3::from(axis).length() < 1e-10
        {
            return Err("Revolve needs a finite origin and a non-zero axis".into());
        }
        faber_geometry::NativeKernel.build(&Geometry::Extrude {
            profile: profile.clone(),
            height: 1.,
        })?;
        let rings: Vec<_> = std::iter::once(&profile.outer)
            .chain(&profile.holes)
            .collect();
        let counts: Vec<_> = rings.iter().map(|r| r.len()).collect();
        let points: Vec<f64> = rings
            .iter()
            .flat_map(|r| r.iter().flatten().copied())
            .collect();
        shape_call(|out, error, len| unsafe {
            faber_revolve(
                points.as_ptr(),
                counts.as_ptr(),
                counts.len(),
                origin.as_ptr(),
                axis.as_ptr(),
                angle.to_radians(),
                out,
                error,
                len,
            )
        })
    }
    fn build(&self, geometry: &Geometry) -> Result<Mesh> {
        let shape = self.exact_shape(geometry)?.ok_or("No exact shape")?;
        self.tessellate(&shape, 0.03)
    }

    fn exact_shape(&self, geometry: &Geometry) -> Result<Option<Shape>> {
        match geometry {
            Geometry::Sweep {
                profile,
                placement,
                path,
            } => return self.sweep(profile, placement, path).map(Some),
            Geometry::Loft { sections, ruled } => {
                return self
                    .loft(
                        &sections
                            .iter()
                            .map(|s| (s.profile.clone(), s.placement.clone()))
                            .collect::<Vec<_>>(),
                        *ruled,
                    )
                    .map(Some);
            }
            Geometry::Sphere { radius } => return self.sphere_shape(*radius).map(Some),
            Geometry::Cone {
                base_radius,
                top_radius,
                height,
            } => {
                return self
                    .cone_shape(*base_radius, *top_radius, *height)
                    .map(Some);
            }
            Geometry::Torus {
                major_radius,
                minor_radius,
            } => return self.torus_shape(*major_radius, *minor_radius).map(Some),
            Geometry::Revolve {
                profile,
                origin,
                axis,
                angle,
            } => return self.revolve(profile, *origin, *axis, *angle).map(Some),
            Geometry::Box { size } => return self.box_shape(*size).map(Some),
            Geometry::Cylinder { radius, height } => {
                return self.cylinder_shape(*radius, *height).map(Some);
            }
            _ => {}
        }
        faber_geometry::NativeKernel.build(geometry)?;
        let (profile, height, floor) = match geometry {
            Geometry::Extrude { profile, height } => (profile, *height, 0.),
            Geometry::Pocket {
                profile,
                height,
                floor,
            } => (profile, *height, *floor),
            _ => return Err("Operation needs a feature context".into()),
        };
        let rings: Vec<_> = std::iter::once(&profile.outer)
            .chain(&profile.holes)
            .collect();
        let counts: Vec<_> = rings.iter().map(|r| r.len()).collect();
        let points: Vec<f64> = rings
            .iter()
            .flat_map(|r| r.iter().flatten().copied())
            .collect();
        shape_call(|out, error, len| unsafe {
            faber_profile(
                points.as_ptr(),
                counts.as_ptr(),
                rings.len(),
                height,
                floor,
                out,
                error,
                len,
            )
        })
        .map(Some)
    }

    fn box_shape(&self, size: [f64; 3]) -> Result<Shape> {
        for value in size {
            positive(value)?;
        }
        shape_call(|out, error, len| unsafe { faber_box(size.as_ptr(), out, error, len) })
    }
    fn cylinder_shape(&self, radius: f64, height: f64) -> Result<Shape> {
        positive(radius)?;
        positive(height)?;
        shape_call(|out, error, len| unsafe { faber_cylinder(radius, height, out, error, len) })
    }
    fn mirror_shape(&self, shape: &Shape, origin: [f64; 3], normal: [f64; 3]) -> Result<Shape> {
        if origin
            .iter()
            .chain(&normal)
            .any(|value| !value.is_finite() || value.abs() > 1e6)
        {
            return Err("Mirror plane is outside the supported range".into());
        }
        let normal = DVec3::from_array(normal)
            .try_normalize()
            .ok_or("Mirror normal must be non-zero")?;
        let translation = 2. * normal.dot(DVec3::from_array(origin)) * normal;
        let matrix: [f64; 12] = std::array::from_fn(|i| {
            let row = i / 4;
            let column = i % 4;
            if column == 3 {
                translation[row]
            } else {
                (if row == column { 1. } else { 0. }) - 2. * normal[row] * normal[column]
            }
        });
        let shape = pointer(shape)?;
        shape_call(|out, error, len| unsafe {
            faber_transform(shape, matrix.as_ptr(), out, error, len)
        })
    }

    fn transform_shape(&self, source: &Shape, placement: &Placement) -> Result<Shape> {
        if placement
            .rotation
            .iter()
            .chain(&placement.translation)
            .any(|v| !v.is_finite() || v.abs() > 1e6)
        {
            return Err("Placement is outside the supported range".into());
        }
        let shape = pointer(source)?;
        let [x, y, z] = placement.rotation.map(f64::to_radians);
        let rotation =
            DMat3::from_rotation_z(z) * DMat3::from_rotation_y(y) * DMat3::from_rotation_x(x);
        let matrix: [f64; 12] = std::array::from_fn(|i| {
            if i % 4 == 3 {
                placement.translation[i / 4]
            } else {
                rotation.col(i % 4)[i / 4]
            }
        });
        let copy = shape_call(|out, error, len| unsafe {
            faber_transform(shape, matrix.as_ptr(), out, error, len)
        })?;
        Ok(sketch_text::carried(source, copy))
    }
    fn combine(&self, left: &Shape, right: &Shape, operation: Combine) -> Result<Shape> {
        let (left, right) = (pointer(left)?, pointer(right)?);
        let op = match operation {
            Combine::Union => 0,
            Combine::Difference => 1,
            Combine::Intersection => 2,
        };
        shape_call(|out, error, len| unsafe { faber_combine(left, right, op, out, error, len) })
    }
    fn intersect_solids(&self, left: &Shape, right: &Shape) -> Result<Option<Shape>> {
        let common = self.combine(left, right, Combine::Intersection)?;
        // The summary has no bounds. An empty common part has no solid.
        let solids = self.summary_properties(&common)?.solids;
        Ok((solids > 0).then_some(common))
    }
    fn properties(&self, shape: &Shape) -> Result<Properties> {
        let shape = pointer(shape)?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        let mut data = RawProperties::default();
        error(
            unsafe { faber_properties(shape, &mut data, buffer.as_mut_ptr(), buffer.len()) },
            &buffer,
        )?;
        Ok(Properties {
            volume: data.volume,
            centre: data.centre,
            bounds: [
                [data.bounds[0], data.bounds[1], data.bounds[2]],
                [data.bounds[3], data.bounds[4], data.bounds[5]],
            ],
            faces: data.faces,
            edges: data.edges,
            solids: data.solids,
            valid: data.valid != 0,
        })
    }
    fn summary_properties(&self, shape: &Shape) -> Result<Properties> {
        let shape = pointer(shape)?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        let mut data = RawProperties::default();
        error(
            unsafe {
                faber_summary_properties(shape, &mut data, buffer.as_mut_ptr(), buffer.len())
            },
            &buffer,
        )?;
        Ok(Properties {
            volume: data.volume,
            centre: data.centre,
            bounds: [[0.; 3]; 2],
            faces: data.faces,
            edges: data.edges,
            solids: data.solids,
            valid: data.valid != 0,
        })
    }
    fn mass_properties(&self, shape: &Shape) -> Result<faber_ports::MassProperties> {
        let shape = pointer(shape)?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        let mut data = RawMassProperties::default();
        error(
            unsafe { faber_mass_properties(shape, &mut data, buffer.as_mut_ptr(), buffer.len()) },
            &buffer,
        )?;
        if !data.volume.is_finite()
            || data.volume <= 0.
            || !data.area.is_finite()
            || data.area <= 0.
            || data
                .centre
                .iter()
                .chain(&data.inertia)
                .any(|v| !v.is_finite())
        {
            return Err("Mass properties returned invalid values".into());
        }
        let i = data.inertia;
        Ok(faber_ports::MassProperties {
            volume_mm3: data.volume,
            area_mm2: data.area,
            centre_of_mass_mm: data.centre,
            inertia_unit_mm5: [[i[0], i[1], i[2]], [i[3], i[4], i[5]], [i[6], i[7], i[8]]],
        })
    }
    fn tessellate(&self, shape: &Shape, tolerance: f64) -> Result<Mesh> {
        Ok(self.tessellate_faces(shape, tolerance)?.mesh)
    }
    fn tessellate_edges(&self, shape: &Shape, tolerance: f64) -> Result<faber_ports::EdgeLines> {
        positive(tolerance)?;
        let shape = pointer(shape)?;
        let _guard = gate();
        let mut raw = RawEdgeLines::default();
        let mut buffer = [0; 1024];
        error(
            unsafe {
                faber_edge_lines(
                    shape,
                    tolerance,
                    &mut raw,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            },
            &buffer,
        )?;
        let mut lines = faber_ports::EdgeLines::default();
        if raw.edge_count > 0 {
            let ends = unsafe { std::slice::from_raw_parts(raw.ends, raw.edge_count) };
            let mut start = 0;
            lines.ranges = ends
                .iter()
                .map(|&end| {
                    let range = start..end;
                    start = end;
                    range
                })
                .collect();
        }
        if raw.point_count > 0 {
            lines.points =
                unsafe { std::slice::from_raw_parts(raw.points, raw.point_count) }.to_vec();
        }
        Ok(lines)
    }
    fn tessellate_faces(&self, shape: &Shape, tolerance: f64) -> Result<faber_ports::FaceMesh> {
        Ok(Self::mesh_with_curvature(shape, tolerance, false)?.faces)
    }
    fn tessellate_curvature(
        &self,
        shape: &Shape,
        tolerance: f64,
    ) -> Result<faber_ports::CurvedMesh> {
        Self::mesh_with_curvature(shape, tolerance, true)
    }
    fn write_shape(&self, shape: &Shape, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let temporary = tempfile::Builder::new()
            .suffix(".step")
            .tempfile_in(parent)
            .map_err(|e| e.to_string())?
            .into_temp_path();
        let shape = pointer(shape)?;
        let temporary_text = path_text(&temporary)?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        error(
            unsafe {
                faber_write(
                    shape,
                    temporary_text.as_ptr(),
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            },
            &buffer,
        )?;
        temporary.persist(path).map_err(|e| e.to_string())
    }
    fn read_shape(&self, path: &Path) -> Result<Shape> {
        let path = path_text(path)?;
        shape_call(|out, error, len| unsafe { faber_read(path.as_ptr(), out, error, len) })
    }
    fn cache_version(&self) -> Option<&'static str> {
        // The version names the OCCT library, the Faber crate and the adapter source.
        static VERSION: OnceLock<String> = OnceLock::new();
        Some(
            VERSION
                .get_or_init(|| {
                    format!(
                        "occt-{}-bintools-1 faber-{}-adapter-{}",
                        occt_version(),
                        env!("CARGO_PKG_VERSION"),
                        env!("FABER_ADAPTER_HASH")
                    )
                })
                .as_str(),
        )
    }
    fn encode_shape(&self, shape: &Shape) -> Result<Vec<u8>> {
        // The first byte tells whether the shape carries the text marker.
        let (tag, plain) = match shape.get::<sketch_text::TextRegions>() {
            Some(marked) => (sketch_text::TEXT_TAG, &marked.0),
            None => (sketch_text::PLAIN_TAG, shape),
        };
        let mut bytes = vec![tag];
        bytes.extend(encode_plain(plain)?);
        Ok(bytes)
    }
    fn decode_shape(&self, bytes: &[u8]) -> Result<Shape> {
        let (tag, body) = bytes.split_first().ok_or("Cannot decode shape: no data")?;
        if *tag != sketch_text::PLAIN_TAG && *tag != sketch_text::TEXT_TAG {
            return Err("Cannot decode shape: unknown format".into());
        }
        let shape = decode_plain(body)?;
        Ok(if *tag == sketch_text::TEXT_TAG {
            sketch_text::marked(shape)
        } else {
            shape
        })
    }
    fn face_count(&self, shape: &Shape) -> Result<u32> {
        let shape = pointer(shape)?;
        let _guard = gate();
        let mut buffer = [0; 1024];
        let mut count = 0;
        error(
            unsafe { faber_face_count(shape, &mut count, buffer.as_mut_ptr(), buffer.len()) },
            &buffer,
        )?;
        u32::try_from(count).map_err(|_| "Too many faces".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use faber_model::{SketchLoop, SketchProfile, TextAlign};

    #[test]
    fn cache_version_names_occt_the_crate_and_the_adapter_source() {
        let version = OcctKernel.cache_version().unwrap();
        assert_eq!(occt_version(), "7.9.3");
        assert!(version.starts_with("occt-7.9.3-"), "{version}");
        assert!(
            version.contains(&format!("faber-{}-adapter-", env!("CARGO_PKG_VERSION"))),
            "{version}"
        );
        assert_eq!(env!("FABER_ADAPTER_HASH").len(), 64);
        assert!(version.ends_with(env!("FABER_ADAPTER_HASH")), "{version}");
    }

    fn square(x: f64) -> SketchProfile {
        SketchProfile {
            outer: SketchLoop::Polygon {
                points: vec![[x, 0.], [x + 10., 0.], [x + 10., 10.], [x, 10.]],
                corner_radius: None,
            },
            holes: Vec::new(),
        }
    }

    #[test]
    fn encoded_text_keeps_its_regions_and_a_plain_shape_stays_plain() {
        let kernel = OcctKernel;
        let placement = Placement::default();
        let text = SketchProfile {
            outer: SketchLoop::Text {
                text: "Hi".into(),
                height: 10.,
                at: [0., 0.],
                align: TextAlign::Left,
                angle_deg: None,
                font: None,
            },
            holes: Vec::new(),
        };
        let shape = sketch_text::sketch(&kernel, &text, &placement).unwrap();
        let bytes = kernel.encode_shape(&shape).unwrap();
        assert_eq!(bytes[0], sketch_text::TEXT_TAG);
        let decoded = kernel.decode_shape(&bytes).unwrap();
        let solids = sketch_text::extrude_regions(&kernel, &decoded, [0., 0., 1.]).unwrap();
        assert!(solids.is_some(), "the decoded text lost its regions");

        let plain = kernel.sketch(&square(0.), &placement).unwrap();
        let bytes = kernel.encode_shape(&plain).unwrap();
        assert_eq!(bytes[0], sketch_text::PLAIN_TAG);
        let decoded = kernel.decode_shape(&bytes).unwrap();
        let solids = sketch_text::extrude_regions(&kernel, &decoded, [0., 0., 1.]).unwrap();
        assert!(solids.is_none());
    }

    #[test]
    fn entries_without_a_tag_and_unknown_tags_are_refused() {
        let kernel = OcctKernel;
        let plain = kernel.sketch(&square(0.), &Placement::default()).unwrap();
        // An old entry is the bare OCCT stream. Its first byte is not a tag.
        let old = encode_plain(&plain).unwrap();
        assert!(
            old[0] > sketch_text::TEXT_TAG,
            "old entry starts with {}",
            old[0]
        );
        assert!(kernel.decode_shape(&old).is_err());
        assert!(kernel.decode_shape(&[]).is_err());
        let mut unknown = kernel.encode_shape(&plain).unwrap();
        unknown[0] = 9;
        let error = kernel.decode_shape(&unknown).err().unwrap();
        assert!(error.to_string().contains("unknown format"), "{error}");
    }
}
