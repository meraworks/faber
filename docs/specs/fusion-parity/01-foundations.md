# Foundations: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the later chunks four shared parts: a material library, a mesh
that knows the face of each triangle, fast ray queries, and vector documents.

**Architecture:** Materials are data in `faber-std` and a field of each part.
The kernel port gets one method that returns a mesh with a face index for each
triangle. Ray queries are pure Rust in `faber-geometry`. Vector documents are a
port in `faber-ports` with SVG and DXF writers in `faber-files`.

**Tech Stack:** Rust 2024, OCCT 7.9.3 through `faber-kernel-occt`, `rayon`,
`quick-xml` (tests only).

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` (shared rules, decisions D6
and D7).

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Read them first.
- Task 1 changes `faber-model`: the coordinator commits that change alone.
- Task 3 changes `bridge.cpp`: say so before the kernel build.
- No task changes the result of an existing tool. Only new fields and tools.

## Review Focus

- A project with no `material` field opens and saves with no new bytes
  (Task 1: `old_part_file_stays_byte_identical`).
- An unknown material id is refused with the nearest names (Task 1:
  `unknown_material_names_the_nearest`).
- A ray that hits an edge or a vertex of the mesh does not count twice
  (Task 2: `inside_is_stable_on_edges_and_vertices`).
- A mesh with zero triangles gives an error, not a panic (Task 2:
  `empty_mesh_is_refused`).
- The face index survives a placement change and the body cache (Task 3:
  `face_index_survives_a_move_and_a_reopen`).

---

### Task 1: Material library

**Files:**
- Create: `crates/faber-std/src/materials.rs`
- Modify: `crates/faber-model/src/lib.rs` (`Part.material`, `ProjectData.materials`, `Edit::SetPartInfo` carries `material`)
- Modify: `crates/faber-files/src/layout.rs`, `crates/faber-core/src/lib.rs`, `crates/faber-wiring/src/lib.rs` (`mass_properties`), `src/server.rs`, `crates/faber-viewer/src/app.rs` (part panel)
- Test: `crates/faber-std/src/materials.rs` (unit), `crates/faber-wiring/tests/materials.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
// faber-std/src/materials.rs
pub struct Material {
    pub id: &'static str, pub name: &'static str, pub class: Class, // Polymer | Metal
    pub density_g_cm3: f64, pub youngs_mpa: f64, pub poisson: f64,
    pub yield_mpa: f64, pub ultimate_mpa: f64,
    pub conductivity_w_mk: f64, pub expansion_um_mk: f64, pub specific_heat_j_kgk: f64,
    pub layer_strength_factor: f64, // 1.0 for a metal; D7 for a printed polymer
    pub colour: [u8; 3], pub roughness: f64, pub metallic: f64,
    pub source: &'static str,       // public data sheet or standard
}
pub fn material(id: &str) -> Option<&'static Material>;
pub fn materials() -> &'static [Material];
```

```rust
// faber-model: owned form for project materials, same fields, `id` is the map key
pub struct MaterialData { /* name, class, the numbers, colour, roughness, metallic, source */ }
// Part gains:        #[serde(default, skip_serializing_if = "Option::is_none")] pub material: Option<String>
// ProjectData gains: #[serde(default, skip_serializing_if = "BTreeMap::is_empty")] pub materials: BTreeMap<String, MaterialData>
```

- Lookup order for a part: project `materials`, then the library.
- Library ids: `pla`, `petg`, `abs`, `asa`, `pa12`, `pc`, `tpu-95a`,
  `aluminium-6061-t6`, `steel-s235`, `stainless-304`, `brass-cw614n`.
- Start values (D6: confirm each row against a public source and record it;
  correct a value that differs):

| id | density | E (MPa) | Poisson | yield (MPa) | layer factor |
| --- | --- | --- | --- | --- | --- |
| pla | 1.24 | 3300 | 0.36 | 55 | 0.5 |
| petg | 1.27 | 2100 | 0.38 | 50 | 0.6 |
| abs | 1.04 | 2200 | 0.35 | 40 | 0.5 |
| pa12 | 1.01 | 1700 | 0.39 | 45 | 0.7 |
| aluminium-6061-t6 | 2.70 | 68900 | 0.33 | 276 | 1.0 |
| steel-s235 | 7.85 | 210000 | 0.30 | 235 | 1.0 |
| stainless-304 | 8.00 | 193000 | 0.29 | 215 | 1.0 |

- MCP: new read tool `materials {query?}` returns `[{id, name, class, density_g_cm3, youngs_mpa, yield_mpa}]`; `{id}` returns the full row. `set_part_info` accepts `material`. New tool `set_material {id, material}` puts a project material; `remove_material {id}` refuses a material in use and names the parts.
- `mass_properties` uses the density of the part material when the call gives no density. The reply names the material.

- [ ] **Step 1: Write failing tests.**
  - `materials.rs` unit `every_row_is_plausible`: for each row `0 < poisson < 0.5`, `youngs_mpa > 0`, `yield_mpa <= ultimate_mpa`, `0 < layer_strength_factor <= 1`, `source` is not empty.
  - `tests/materials.rs::old_part_file_stays_byte_identical`: open and save `examples/trash-bin`; the fingerprint and every file byte stay equal.
  - `tests/materials.rs::mass_uses_the_part_material`: a 10 x 10 x 10 mm box with `material: "steel-s235"` has mass 7.85 g within 1e-9.
  - `tests/materials.rs::project_material_wins`: a project material `pla` with density 2.0 gives 2.0 g for the same box.
  - `tests/mcp.rs::unknown_material_names_the_nearest`: `set_part_info` with `material: "pett"` returns `Unknown material pett. Nearest: petg.`
  - `tests/mcp.rs::material_in_use_is_not_removed`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-std -- --test-threads=1` and `cargo test --offline -j 4 -p faber-wiring --test materials -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** The viewer part panel shows a material selector and the mass.
- [ ] **Step 4: Run** the tests of Step 2 and `cargo test --offline -j 4 --test mcp --test context_size -- --test-threads=1`. Expected: pass.

### Task 2: Ray queries

**Files:**
- Create: `crates/faber-geometry/src/bvh.rs`
- Modify: `crates/faber-geometry/src/lib.rs` (`pub mod bvh;`), `crates/faber-geometry/Cargo.toml` (`rayon`)
- Test: unit tests in `bvh.rs`

**Interfaces:**

```rust
pub struct Bvh { /* flat node array, triangle order */ }
pub struct Hit { pub t: f64, pub triangle: u32, pub point: [f64; 3] }
impl Bvh {
    pub fn new(mesh: &faber_ports::Mesh) -> Result<Bvh>;                 // refuses zero triangles
    pub fn first_hit(&self, origin: [f64; 3], direction: [f64; 3]) -> Option<Hit>;
    pub fn hits(&self, origin: [f64; 3], direction: [f64; 3]) -> Vec<Hit>; // sorted by t
    pub fn inside(&self, point: [f64; 3]) -> bool;                       // closed mesh only
    pub fn closest(&self, point: [f64; 3]) -> Hit;                       // t = distance
}
```

- `inside` casts three rays in fixed irrational directions and takes the
  majority of the three parity results.
- Build: median split on the longest axis, at most 4 triangles in a leaf.

- [ ] **Step 1: Write failing tests** (a unit cube mesh from 0 to 10, 12 triangles):
  - `first_hit_gives_the_distance`: origin (5, 5, -5), direction +Z gives `t == 5.0` within 1e-12.
  - `hits_are_sorted`: the same ray gives two hits at 5 and 15.
  - `inside_is_stable_on_edges_and_vertices`: 1 000 points on a seeded grid that includes points in line with edges and corners; each answer equals the box test `0 < x, y, z < 10`.
  - `closest_gives_the_face_point`: point (5, 5, 14) gives point (5, 5, 10), `t == 4.0`.
  - `empty_mesh_is_refused`: `Bvh::new` returns `The mesh has no triangle.`
  - `matches_brute_force`: a seeded sphere mesh with 5 000 triangles; 500 seeded rays give the same first hit as a loop over all triangles.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-geometry bvh -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** Moller-Trumbore intersection with a fixed tie rule so that a ray through a shared edge counts once.
- [ ] **Step 4: Run** the tests. Expected: pass. Record the time of 100 000 rays on 100 000 triangles in the report (release build).

### Task 3: Face meshes

**Files:**
- Modify: `crates/faber-ports/src/lib.rs` (`FaceMesh`, `Kernel::tessellate_faces`)
- Modify: `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`
- Modify: `crates/faber-core/src/body_cache.rs` (`LocalBody.face_of_triangle`), `crates/faber-core/src/lib.rs` (`Body.face_of_triangle`)
- Test: `crates/faber-wiring/tests/face_mesh.rs`

**Interfaces:**

```rust
pub struct FaceMesh { pub mesh: Mesh, pub face_of_triangle: Vec<u32> } // face index as in Kernel::faces
fn tessellate_faces(&self, _shape: &Shape, _tolerance: f64) -> Result<FaceMesh>; // default: not supported
```

- `Body` and `LocalBody` gain `face_of_triangle: Vec<u32>` (empty for a
  mesh-only part). The body cache key and the persistent mesh cache store it.
- A helper in `faber-core`: `pub fn face_triangles(body: &Body, face: u32) -> Vec<[[f64; 3]; 3]>` (world coordinates).

- [ ] **Step 1: Write failing tests.**
  - `box_has_two_triangles_for_each_face`: 12 triangles; each of the 6 face indices occurs 2 times.
  - `cylinder_side_is_one_face`: all side triangles have the index of the face with `cylinder` data in `Kernel::faces`.
  - `face_index_survives_a_move_and_a_reopen`: move the part, open the project again; `face_triangles` of the top face has all Z equal to the moved top.
  - `mesh_only_part_has_no_face_index`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test face_mesh -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** In the bridge, walk the faces in the `TopExp` order that `faces` uses and append each face triangulation. Replace the `tessellate` call of the body cache with `tessellate_faces`.
- [ ] **Step 4: Run** the new tests and `cargo test --offline -j 4 -p faber-wiring -- --test-threads=1`. Expected: pass.

### Task 4: Vector documents

**Files:**
- Create: `crates/faber-ports/src/vector.rs`, `crates/faber-files/src/vector.rs`
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-files/src/lib.rs`, `crates/faber-wiring/src/lib.rs`
- Test: `crates/faber-files/tests/vector.rs`

**Interfaces:**

```rust
// faber-ports/src/vector.rs — millimetres, Y up, origin at the lower left
pub struct VectorDoc { pub size_mm: [f64; 2], pub layers: Vec<Layer>, pub texts: Vec<Text> }
pub struct Layer { pub name: String, pub colour: [u8; 3], pub width_mm: f64, pub dashed: bool, pub paths: Vec<Path2> }
pub enum Path2 { Polyline { points: Vec<[f64; 2]>, closed: bool }, Circle { centre: [f64; 2], radius: f64 },
                 Arc { centre: [f64; 2], radius: f64, start_deg: f64, end_deg: f64 } }
pub struct Text { pub at: [f64; 2], pub height_mm: f64, pub text: String, pub anchor: Anchor, pub layer: String }
pub trait VectorExport: Send + Sync { fn write(&self, doc: &VectorDoc, path: &Path) -> Result<()>; } // format from the extension
```

- Writers in this task: `.svg` and `.dxf` (R12 ASCII: `LINE`, `CIRCLE`, `ARC`, `TEXT`, `LAYER` table). An unknown extension is refused with the list of formats.

- [ ] **Step 1: Write failing tests.**
  - `svg_has_the_sheet_size`: a 297 x 210 document parses with `quick-xml`; the root has `width="297mm"`, `height="210mm"`, `viewBox="0 0 297 210"`.
  - `svg_flips_y`: a point at (10, 20) is written at (10, 190).
  - `dxf_circle_is_one_entity`: a circle of radius 5 gives one `CIRCLE` with group 40 equal to `5.0`, in the layer of its `Layer`.
  - `dxf_arc_is_counter_clockwise`: start 0, end 90 gives groups 50 and 51 equal to 0 and 90.
  - `dashed_layer_is_dashed`: SVG has `stroke-dasharray`; DXF layer has line type `DASHED`.
  - `text_is_escaped`: the text `a<b&c` is valid XML in SVG.
  - `unknown_format_is_refused`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-files --test vector -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests. Expected: pass.
