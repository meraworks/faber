# Subdivision forms: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Model free shapes (handles, grips, housings) by the edit of a coarse
cage that a subdivision rule turns into a smooth closed shape, and use that
shape as a solid in a part history.

**Architecture:** A form is one recipe: a cage of vertices and faces with
crease values. A new pure Rust crate `faber-form` holds the cage operations
and the Catmull-Clark subdivision. The result is a closed mesh. With
`as_solid` the mesh becomes a faceted solid through `solid_from_mesh`
(chunk 05), so Boolean and feature tools work on it (decision D5).

**Tech Stack:** Rust 2024, `faber-meshtools` (mesh report), the kernel call
`solid_from_mesh`.

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` (decision D5). Fusion scope
covered: the Form workspace primitives and the edit operations below. Not
covered: T-spline surfaces, a smooth NURBS result, pull to a target body,
thicken of an open form.

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Needs chunk 05.
- Task 1 changes `faber-model`.
- A cage is a closed manifold: each edge has two faces. An operation that
  breaks this is refused. An open cage is not in this chunk.
- Cage faces are triangles or quadrilaterals. Vertex indexes are stable under
  an edit that adds geometry: new vertices go at the end.
- Subdivision levels are 0 to 4. The default is 3. The face count of the
  result is refused above 400 000.

## Review Focus

- A cage with a face that names a vertex twice, or a vertex that no face uses,
  is refused with the index (Task 1: `bad_cage_is_refused`).
- A move that makes the result intersect itself is reported by the mesh
  report, and `as_solid` refuses it (Task 5: `self_intersecting_form_is_not_a_solid`).
- An extrude of two faces that share an edge makes one joined extrusion, not
  two walls inside the shape (Task 3: `joined_faces_extrude_as_one`).
- A symmetry plane keeps the two halves equal after each edit (Task 3:
  `symmetry_mirrors_a_move`).
- Level 0 gives the cage itself (Task 2: `level_zero_is_the_cage`).

---

### Task 1: Form recipe and cage checks

**Files:**
- Modify: `crates/faber-model/src/lib.rs` (`Geometry::Form`), `crates/faber-geometry/src/lib.rs`
- Create: `crates/faber-form/Cargo.toml`, `src/lib.rs`, `src/cage.rs`
- Test: `crates/faber-form/tests/cage.rs`

**Interfaces:**

```rust
Geometry::Form { cage: CageData<N>, levels: Option<u32>, as_solid: bool, symmetry: Vec<SymmetryPlane> /* x | y | z through the part origin */ }
pub struct CageData<N> { pub vertices: Vec<[N; 3]>, pub faces: Vec<Vec<u32>>, pub creases: Vec<Crease> /* edge [a, b], sharpness 0..=10 */ }
// faber-form
pub struct Cage { /* half-edge structure */ }
impl Cage { pub fn from_data(data: &CageData<f64>) -> Result<Cage>; pub fn to_data(&self) -> CageData<f64>;
            pub fn euler(&self) -> i64; pub fn edges(&self) -> Vec<[u32; 2]>; }
pub fn form_box(size: [f64; 3], segments: [u32; 3]) -> CageData<f64>;
pub fn form_cylinder(radius: f64, height: f64, sides: u32, segments: u32) -> CageData<f64>;
pub fn form_sphere(radius: f64, segments: u32) -> CageData<f64>;   // a subdivided cube pushed to the sphere
pub fn form_torus(major: f64, minor: f64, sides: u32, rings: u32) -> CageData<f64>;
```

- [ ] **Step 1: Write failing tests.**
  - `box_cage_counts`: `form_box` with 1 segment has 8 vertices, 6 faces, 12 edges, Euler number 2.
  - `torus_euler_is_zero`.
  - `round_trip_keeps_the_order`.
  - `bad_cage_is_refused`: `Face 3 names vertex 5 two times.`, `Vertex 9 is not used.`, `Edge 2-6 has 3 faces.`, `Edge 0-1 has 1 face. The cage must be closed.`
  - `crease_on_a_missing_edge_is_refused`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-form --test cage -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Subdivision

**Files:**
- Create: `crates/faber-form/src/subdivide.rs`
- Test: `crates/faber-form/tests/subdivide.rs`

**Interfaces:**

```rust
pub fn subdivide(cage: &Cage, levels: u32) -> Result<Mesh>;      // quads split into two triangles; smooth vertex normals
pub fn subdivide_once(cage: &Cage) -> Cage;                      // Catmull-Clark with crease rules
```

- Crease rule: an edge with sharpness `s >= 1` uses the sharp edge rule and passes `s - 1` to its two children. A vertex with two sharp edges uses the crease vertex rule. A vertex with three or more sharp edges does not move.

- [ ] **Step 1: Write failing tests** (cube cage of side 20):
  - `level_zero_is_the_cage`: 12 triangles, volume 8000.
  - `face_count_grows_by_four`: level 3 has `6 * 4^3` quads.
  - `result_is_closed`: no open edge, Euler number 2, at each level 1 to 4.
  - `result_stays_inside_the_cage`: each vertex is inside the cube within 1e-9.
  - `result_is_symmetric`: the vertex set is equal under the three mirror planes within 1e-9.
  - `fully_creased_cube_stays_a_cube`: sharpness 10 on all 12 edges, level 3: volume 8000 within 1e-6.
  - `one_creased_loop_keeps_a_flat_end`: the four top edges creased: the top is a flat face at Z 20 within 1e-9; the bottom is round.
  - `volume_falls_with_each_level_and_converges`: the change from level 3 to level 4 is below 1 %.
  - `torus_keeps_euler_zero`.
  - `too_many_faces_are_refused`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-form --test subdivide -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Cage operations

**Files:**
- Create: `crates/faber-form/src/edit.rs`
- Test: `crates/faber-form/tests/edit.rs`

**Interfaces:**

```rust
pub enum Select { Vertices(Vec<u32>), Edges(Vec<[u32; 2]>), Faces(Vec<u32>) }
pub enum FormOp { Move { select: Select, by: [f64; 3] }, Scale { select: Select, factor: [f64; 3], about: Option<[f64; 3]> },
    Rotate { select: Select, axis: [f64; 3], degrees: f64, about: Option<[f64; 3]> },
    Extrude { faces: Vec<u32>, distance: f64 },             // along the mean normal; joined faces move as one
    Inset { faces: Vec<u32>, distance: f64 },
    InsertLoop { edge: [u32; 2], at: f64 /* 0..1 */ },       // an edge loop across the ring of quads
    Crease { edges: Vec<[u32; 2]>, sharpness: f64 }, Bridge { a: u32, b: u32 } /* joins two faces with a tube; both are removed */,
    Weld { a: u32, b: u32 }, Delete { faces: Vec<u32> } /* refused if the cage opens */, Flatten { select: Select, plane: [f64; 4] } }
pub fn apply(cage: &Cage, ops: &[FormOp], symmetry: &[SymmetryPlane]) -> Result<Cage>;
```

- With a symmetry plane, each operation is also applied to the mirror selection. A vertex on the plane stays on the plane.

- [ ] **Step 1: Write failing tests** (cube cage of side 20, level 0 for volumes):
  - `extrude_top_by_ten`: volume 12000; 4 new vertices at the end of the list; the old indexes are unchanged.
  - `joined_faces_extrude_as_one`: two top faces of a 2 x 1 x 1 cage: no inner wall; volume grows by area times distance.
  - `inset_then_extrude_makes_a_boss`.
  - `insert_loop_keeps_the_shape`: the level 0 volume is unchanged; the vertex count grows by 4.
  - `bridge_makes_a_hole`: bridge of two opposite faces of a ring-shaped cage changes the Euler number from 2 to 0.
  - `delete_is_refused`: `The cage is not closed after this operation.`
  - `symmetry_mirrors_a_move`: with plane X, a move of one vertex by (2, 1, 0) moves its mirror by (-2, 1, 0).
  - `move_keeps_plane_vertices_on_the_plane`.
  - `flatten_puts_vertices_on_the_plane`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-form --test edit -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Form in a part history and tools

**Files:**
- Create: `crates/faber-core/src/form.rs`
- Modify: `crates/faber-core/src/history.rs`, `crates/faber-core/src/lib.rs`, `crates/faber-wiring/src/lib.rs`, `src/server.rs`
- Test: `crates/faber-wiring/tests/form.rs`, `tests/mcp.rs`

- A part whose geometry is `Form` with `as_solid: false` is a mesh part. With `as_solid: true` it has an exact (faceted) shape and can be the first feature of a history or an input of `union`, `difference` and `intersection`.
- MCP: `new_form {id, primitive, ...}` (box, cylinder, sphere, torus), `edit_form {part, feature?, ops, expected_revision?}` (applies `FormOp` values, builds, saves one revision; the reply has the mesh report numbers and the changed part summary), `form_info {part}` (vertex, edge and face counts, the symmetry, the creases, and with `detail: true` the cage).

- [ ] **Step 1: Write failing tests.**
  - `form.rs::form_part_builds_and_has_a_volume`.
  - `form.rs::solid_form_takes_a_hole`: a `hole` feature of diameter 4 through a level 2 solid form builds; the volume falls.
  - `form.rs::form_minus_a_box`: `difference` with a box works and gives one solid.
  - `form.rs::edit_form_is_one_revision_and_undo_restores_the_cage`.
  - `form.rs::self_intersecting_form_is_not_a_solid`: a vertex pulled through the opposite side: the reply has the mesh problem and `as_solid` is refused with `The form intersects itself.`
  - `mcp.rs::form_info_is_small_without_detail`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.** The self-intersection test uses `Bvh` triangle pair tests on the level 2 mesh.
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Viewer form mode, guide, example

**Files:**
- Create: `crates/faber-viewer/src/app/form.rs`
- Modify: `crates/faber-viewer/src/app.rs`, `crates/faber-build/src/faber.libsonnet` (`form_box`, `form_cylinder`, `form_ops`), `docs/modelling-guide.md` (section `## Forms`)
- Create: `examples/form-handle/` (a tool handle: a form with one symmetry plane, joined to a modelled shaft socket)

- "Edit form" shows the cage over a level 2 preview. Selection modes: vertex, edge, face (keys 1, 2, 3). The Move/Copy triad moves, turns and scales the selection. Buttons: Extrude, Inset, Insert loop, Crease, Bridge, Flatten, Symmetry. Each action is one revision. "Finish" shows the final level.

- [ ] **Step 1: Write failing viewer tests:** `form_mode_shows_the_cage`, `face_drag_moves_four_vertices`, `extrude_button_adds_faces`, `symmetry_switch_mirrors_an_edit`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer form -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.
