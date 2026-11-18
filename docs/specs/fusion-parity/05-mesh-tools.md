# Mesh tools: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a downloaded mesh usable: report its defects, repair it, reduce
it, cut it with a plane, and turn it into a solid that the feature tools can
change.

**Architecture:** A new pure Rust crate `faber-meshtools` holds the mesh
algorithms. Each tool makes a new asset in the asset store and points the part
to it, so a change is one revision and undo works. One new kernel call makes a
solid from a closed mesh.

**Tech Stack:** Rust 2024, `earcutr`, `rayon`, OCCT 7.9.3
(`BRepBuilderAPI_Sewing`, `ShapeUpgrade_UnifySameDomain`).

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` (shared rules 3, 8, 9).

## Global Constraints

- All shared rules of `00-roadmap.md` apply.
- A purchased part is never changed. Each tool of this chunk refuses it with
  `Part <id> is a purchased part. A change is not permitted.`
- A tool never changes an asset file. It writes a new asset.
- Task 5 changes `bridge.cpp`: say so before the kernel build.
- Read how `Geometry::Imported` and `AssetStore` store a mesh before Task 1.

## Review Focus

- A mesh with two shells that touch at one edge is reported as non-manifold,
  not repaired to a wrong shape (Task 1: `shared_edge_of_two_cubes`).
- A hole with a boundary that is not planar is filled with no flipped triangle
  (Task 2: `saddle_hole_is_filled_outward`).
- A reduction target above the present count changes nothing and says so
  (Task 3: `target_above_the_count_is_a_no_op`).
- A plane that does not cut the mesh returns the whole mesh or an error for
  the empty side (Task 4: `plane_outside_the_mesh`).
- A mesh above the triangle limit is refused for the solid conversion with the
  count and the limit (Task 5: `large_mesh_is_refused_for_a_solid`).

---

### Task 1: Mesh report

**Files:**
- Create: `crates/faber-meshtools/Cargo.toml`, `src/lib.rs`, `src/topology.rs`, `src/report.rs`
- Test: `crates/faber-meshtools/tests/report.rs`

**Interfaces:**

```rust
pub struct Weld { pub mesh: Mesh, pub map: Vec<u32> }
pub fn weld(mesh: &Mesh, tolerance_mm: f64) -> Weld;                 // default 1e-6
pub struct MeshReport { pub vertices: usize, pub triangles: usize, pub shells: usize, pub open_edges: usize,
    pub non_manifold_edges: usize, pub degenerate: usize, pub flipped: usize, pub duplicate: usize,
    pub watertight: bool, pub volume_mm3: Option<f64>, pub area_mm2: f64, pub bounds: [[f64; 3]; 2], pub problems: Vec<String> }
pub fn report(mesh: &Mesh) -> MeshReport;
```

- `flipped`: triangles whose winding differs from the majority of their shell after a flood fill across edges.
- `volume_mm3` is present only for a watertight mesh.

- [ ] **Step 1: Write failing tests** (a 10 mm cube mesh as base):
  - `clean_cube`: 12 triangles, 1 shell, `watertight`, volume 1000 within 1e-9, no problem.
  - `cube_with_a_missing_face`: `open_edges == 4`, not watertight, `problems` has `4 open edges`.
  - `cube_with_one_flipped_triangle`: `flipped == 1`.
  - `unwelded_cube_is_welded_first`: 36 separate vertices give the result of `clean_cube`.
  - `shared_edge_of_two_cubes`: `non_manifold_edges == 1`, `shells == 2`.
  - `degenerate_triangle_is_counted`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-meshtools --test report -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Repair

**Files:**
- Create: `crates/faber-meshtools/src/repair.rs`
- Test: `crates/faber-meshtools/tests/repair.rs`

**Interfaces:**

```rust
pub struct RepairOptions { pub weld_mm: f64, pub fill_holes: bool, pub max_hole_edges: usize /* 500 */,
                           pub fix_winding: bool, pub drop_shells_below_mm3: Option<f64> }
pub struct RepairLog { pub welded: usize, pub removed_degenerate: usize, pub removed_duplicate: usize,
                       pub flipped: usize, pub filled_holes: usize, pub removed_shells: usize, pub left: MeshReport }
pub fn repair(mesh: &Mesh, options: &RepairOptions) -> (Mesh, RepairLog);
```

- Order: weld, drop degenerate and duplicate triangles, make the winding of each shell consistent, turn a shell with negative volume, fill holes, drop small shells.
- Hole fill: project the loop on its best plane and triangulate with `earcutr`; then turn the patch so its winding agrees with the neighbours.
- Repair never moves a vertex. A defect that it cannot repair stays in `left.problems`.

- [ ] **Step 1: Write failing tests.**
  - `missing_face_is_filled`: the cube with a missing face becomes watertight with volume 1000 within 1e-9.
  - `flipped_triangle_is_turned`.
  - `inside_out_cube_is_turned`: volume is +1000 after the repair.
  - `saddle_hole_is_filled_outward`: a sphere with a non-planar hole: watertight, `flipped == 0`, volume within 2 % of the sphere.
  - `dust_shell_is_removed`: a 0.1 mm cube next to the 10 mm cube, with `drop_shells_below_mm3: Some(1.0)`: 1 shell.
  - `non_manifold_edge_stays_reported`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-meshtools --test repair -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Reduce

**Files:**
- Create: `crates/faber-meshtools/src/reduce.rs`
- Test: `crates/faber-meshtools/tests/reduce.rs`

**Interfaces:**

```rust
pub enum Target { Triangles(usize), MaxError(f64) /* mm */, Ratio(f64) }
pub struct ReduceLog { pub before: usize, pub after: usize, pub max_error_mm: f64 }
pub fn reduce(mesh: &Mesh, target: Target) -> Result<(Mesh, ReduceLog)>;
```

- Quadric error edge collapse with a priority queue. A collapse that makes a non-manifold edge, a flipped triangle or an open edge is not made.
- `max_error_mm`: the largest distance from an original vertex to the result (with `Bvh::closest`).

- [ ] **Step 1: Write failing tests.**
  - `sphere_to_a_tenth`: a sphere of radius 10 with 20 000 triangles to 2 000: `after <= 2000`, watertight, volume within 2 %, `max_error_mm < 0.2`.
  - `flat_faces_collapse_fully`: a cube with 1 200 triangles and `MaxError(0.001)`: 12 triangles, volume 1000 within 1e-6.
  - `sharp_edges_stay`: the cube corners stay at their coordinates within 1e-9.
  - `target_above_the_count_is_a_no_op`: `after == before` and the log says so.
  - `open_mesh_is_refused`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-meshtools --test reduce -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Plane cut

**Files:**
- Create: `crates/faber-meshtools/src/cut.rs`
- Test: `crates/faber-meshtools/tests/cut.rs`

**Interfaces:**

```rust
pub enum Keep { Above, Below, Both }
pub fn cut(mesh: &Mesh, origin: [f64; 3], normal: [f64; 3], keep: Keep, cap: bool) -> Result<Vec<Mesh>>;
pub fn slice(mesh: &Mesh, origin: [f64; 3], normal: [f64; 3]) -> Vec<Vec<[f64; 3]>>; // closed loops in the plane
```

- [ ] **Step 1: Write failing tests.**
  - `cube_cut_at_four`: plane Z = 4, keep below, cap: volume 400 within 1e-9, watertight.
  - `both_sides_sum_to_the_whole`: volumes 400 and 600.
  - `torus_cut_gives_two_loops`: `slice` through the axis of a torus gives 2 loops; with the cap the half is watertight.
  - `plane_through_vertices`: the plane Z = 10 of a mesh with vertices at Z = 10 gives no degenerate triangle.
  - `plane_outside_the_mesh`: keep below returns the whole mesh; keep above returns `The plane does not cut the mesh. The selected side is empty.`
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-meshtools --test cut -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** `slice` is public: chunk 14 uses it.
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Mesh to solid

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`
- Test: `crates/faber-kernel-occt/tests/solid_from_mesh.rs`

**Interfaces:**

```rust
/// A solid from a closed mesh. `merge_planar` joins coplanar triangles into one face.
fn solid_from_mesh(&self, _mesh: &Mesh, _merge_planar: bool) -> Result<Shape>;
pub const SOLID_TRIANGLE_LIMIT: usize = 200_000;
```

- [ ] **Step 1: Write failing tests.**
  - `cube_mesh_becomes_six_faces`: with `merge_planar`: 6 faces, volume 1000 within 1e-6.
  - `sphere_mesh_keeps_its_triangles`: 2 000 faces, volume within 1e-6 of the mesh volume.
  - `open_mesh_is_refused`.
  - `large_mesh_is_refused_for_a_solid`: `The mesh has 250000 triangles. The limit for a solid is 200000. Reduce the mesh first.`
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-kernel-occt --test solid_from_mesh -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 6: Tools, viewer, guide

**Files:**
- Create: `crates/faber-wiring/src/mesh_edit.rs`
- Modify: `crates/faber-wiring/src/lib.rs`, `src/server.rs`, `crates/faber-viewer/src/app.rs` (part context menu), `docs/modelling-guide.md` (section `## Mesh parts`)
- Test: `crates/faber-wiring/tests/mesh_edit.rs`, `tests/mcp.rs`

- MCP tools: `mesh_report {part}` (read), `repair_mesh {part, options?}`, `reduce_mesh {part, triangles? | max_error_mm? | ratio?}`, `cut_mesh {part, plane, keep, cap?, copy_as?}`, `mesh_to_solid {part, merge_planar?, copy_as?}`. Each changing tool takes `expected_revision` and returns the log and the changed part summary.
- `mesh_to_solid` writes the solid as an exact asset and makes the part an imported exact part with no `purchased` record. Feature tools then accept it as the base of a history.

- [ ] **Step 1: Write failing tests.**
  - `mesh_edit.rs::repair_is_one_revision_and_undo_restores`.
  - `mesh_edit.rs::purchased_mesh_is_refused` for each of the four changing tools.
  - `mesh_edit.rs::solid_from_a_mesh_takes_a_hole`: convert a cube mesh, add a `hole` feature of diameter 4 through it: volume `1000 - pi * 4 * 10` within 0.1 %.
  - `mesh_edit.rs::old_asset_file_is_unchanged`.
  - `mcp.rs::mesh_report_is_small`: below 1 500 characters.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.** Viewer: the context menu of a mesh part shows "Mesh report", "Repair", "Reduce", "Convert to solid"; the report shows in a small docked panel. Add `examples/mesh-repair/` with one broken mesh (made by a script in the test, not downloaded).
- [ ] **Step 4: Run** the tests and `cargo test --offline -j 4 --test context_size -- --test-threads=1`. Expected: pass.
