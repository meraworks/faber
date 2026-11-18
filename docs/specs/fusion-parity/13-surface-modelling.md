# Surface modelling: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build shapes from surfaces: make a surface by extrude, revolve, loft,
sweep or patch; trim and extend it; stitch surfaces into a solid; and cut or
replace a face of a solid with a surface.

**Architecture:** A surface is a feature inside a part history, as sketches,
curves and planes are now. It is not a solid and it cannot be the output of a
part. Stitch and thicken turn surfaces into a solid. Each operation is a new
variant of `faber_model::Geometry` with one kernel call.

**Tech Stack:** Rust 2024, OCCT 7.9.3 (`BRepFill_Filling`,
`BRepBuilderAPI_Sewing`, `BRepAlgoAPI_Splitter`, `GeomLib::ExtendSurfByLength`).

**Spec:** `docs/specs/fusion-parity/00-roadmap.md`. Fusion scope covered:
Patch, surface Extrude, Revolve, Loft, Sweep, Trim, Extend, Stitch, Thicken
(present), Split Body with a surface, Replace Face. Not covered: Untrim,
Boundary Fill, Ruled along a normal, surface Offset (the `offset` feature
works on a solid).

## Global Constraints

- All shared rules of `00-roadmap.md` apply.
- Task 1 changes `faber-model` (all variants in one commit). Tasks 2 to 5
  change `bridge.cpp`.
- The output feature of a part must be a solid. A history whose output is a
  surface is refused: `The output of part <id> is a surface. Stitch or thicken
  it to make a solid.`
- `feature_info` of a surface feature gives `kind: "surface"`, its area, its
  bounds and the count of open boundary edges. It gives no volume.
- Stitch tolerance default is 0.001 mm. The reply gives the largest gap that
  was closed.

## Review Focus

- A patch over a boundary that is not closed is refused with the gap size and
  position (Task 3: `open_boundary_is_refused`).
- A stitch that leaves open edges gives a surface, and `as_solid: true`
  refuses it with the count of open edges (Task 4:
  `open_shell_is_not_a_solid`).
- A stitched solid with inward normals is turned so its volume is positive
  (Task 4: `inside_out_shell_is_turned`).
- A trim tool that does not cross the surface is refused (Task 5:
  `tool_that_misses_is_refused`).
- A replace of a face with a surface that does not cover the face outline is
  refused (Task 6: `surface_too_small_for_the_face_is_refused`).

---

### Task 1: Model variants

**Files:**
- Modify: `crates/faber-model/src/lib.rs`, `crates/faber-geometry/src/lib.rs`
- Test: unit tests in `crates/faber-model/src/lib.rs`

**Interfaces:**

```rust
SurfaceExtrude { profile: String /* sketch or curve feature */, distance: N, direction: Option<[N; 3]> }
SurfaceRevolve { profile: String, axis: String, angle_deg: N }
SurfaceLoft    { sections: Vec<String> /* curves or sketches */, ruled: bool }
SurfaceSweep   { profile: String, path: PathInput<N> }
Patch          { boundary: Vec<EdgeRef<N>>, tangent_to: Vec<String> /* features whose faces the patch meets smoothly */ }
Trim           { surface: String, tool: String /* surface, plane or sketch feature */, keep: TrimKeep<N> /* point near the kept side */ }
Extend         { surface: String, edge: EdgeSelector<N>, distance: N }
Stitch         { surfaces: Vec<String>, tolerance: Option<N>, as_solid: bool }
SplitBody      { body: String, tool: String /* surface feature */, keep: SplitKeep }
ReplaceFace    { body: String, face: FaceSelector<N>, surface: String }
pub enum EdgeRef<N> { Edge { feature: String, edge: EdgeSelector<N> }, Curve { feature: String } }
```

- [ ] **Step 1: Write failing tests:** JSON round trip with the `op` names `surface_extrude`, `surface_revolve`, `surface_loft`, `surface_sweep`, `patch`, `trim`, `extend`, `stitch`, `split_body`, `replace_face`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-model -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Surfaces from profiles

**Files:**
- Modify: `crates/faber-ports/src/lib.rs` (`Kernel::surface_area`, `Kernel::open_edges`, `Kernel::shape_kind`), `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`, `crates/faber-core/src/history.rs`, `crates/faber-core/src/lib.rs` (output rule), `src/server.rs` (`feature_info`)
- Test: `crates/faber-wiring/tests/surfaces.rs`

**Interfaces:**

```rust
pub enum ShapeKind { Solid, Surface, Curve, Other }
fn shape_kind(&self, _shape: &Shape) -> Result<ShapeKind>;
fn surface_area(&self, _shape: &Shape) -> Result<f64>;
fn open_edges(&self, _shape: &Shape) -> Result<u32>;
```

- The four surface features use the present calls (`extrude_shape`, `revolve_shape`, `loft_shapes`, `sweep_shape`) with the outline wire of the profile, not its face.

- [ ] **Step 1: Write failing tests.**
  - `extruded_square_wire`: a 20 x 20 square extruded 10: `kind` surface, area 800 within 1e-6, 8 open edges.
  - `revolved_line_is_a_cone_surface`: a line from radius 10 to radius 5 over height 20, 360 degrees: area `pi * 15 * sqrt(425) = 971.48` within 0.1 %.
  - `ruled_loft_between_two_circles`: the same area.
  - `sweep_of_an_arc_along_a_line`.
  - `surface_output_is_refused`: the exact text of the constraint above.
  - `feature_info_of_a_surface_has_no_volume`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test surfaces -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Patch

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`, `crates/faber-core/src/history.rs`
- Test: `crates/faber-wiring/tests/surfaces.rs`

**Interfaces:**

```rust
/// A face that fills a closed boundary. `support` faces give tangent continuity on their edges.
fn patch(&self, _boundary: &[Shape], _support: &[Shape]) -> Result<Shape>;
```

- [ ] **Step 1: Write failing tests.**
  - `patch_of_a_planar_square`: area 400 within 1e-6.
  - `patch_of_a_circle_is_a_disc`: area `pi * 100` within 0.1 %.
  - `patch_of_a_bent_boundary`: a square loop with two opposite corners raised by 5: area above 400; the boundary of the patch is the loop within 0.001.
  - `tangent_patch_closes_a_tube_end`: a patch on the end circle of a cylinder surface with `tangent_to` is a dome, with area above the disc area.
  - `open_boundary_is_refused`: `The boundary has a gap of 2.5 mm at (10, 0, 0).`
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Stitch

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`, `crates/faber-core/src/history.rs`
- Test: `crates/faber-wiring/tests/surfaces.rs`

**Interfaces:**

```rust
pub struct Stitched { pub shape: Shape, pub open_edges: u32, pub largest_gap: f64, pub solid: bool }
fn stitch(&self, _surfaces: &[Shape], _tolerance: f64, _as_solid: bool) -> Result<Stitched>;
```

- [ ] **Step 1: Write failing tests.**
  - `box_from_six_faces`: the extruded square wire plus two patches, `as_solid: true`: volume 4000 within 1e-6; the part output is valid.
  - `stitched_solid_takes_a_fillet`: a fillet of radius 2 on one edge builds.
  - `open_shell_is_not_a_solid`: five faces with `as_solid: true`: `The stitched shell has 4 open edges. It is not a solid.`
  - `open_shell_is_a_surface_without_as_solid`: `open_edges == 4`.
  - `inside_out_shell_is_turned`: volume is positive.
  - `gap_above_the_tolerance_stays_open`: two faces 0.01 mm apart with tolerance 0.001 are not joined; with tolerance 0.02 they are, and `largest_gap` is 0.01 within 1e-6.
  - `thicken_of_a_patch_is_a_solid`: the bent patch of Task 3 with `thicken` 2: volume within 2 % of area times 2.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Trim and extend

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`, `crates/faber-core/src/history.rs`
- Test: `crates/faber-wiring/tests/surfaces.rs`

**Interfaces:**

```rust
/// Split a surface with a tool and keep the piece nearest to `keep_near`.
fn trim_surface(&self, _surface: &Shape, _tool: &Shape, _keep_near: [f64; 3]) -> Result<Shape>;
fn extend_surface(&self, _surface: &Shape, _edge: u32, _distance: f64) -> Result<Shape>;
```

- A sketch tool is the prism of the sketch along its plane normal, through the whole surface.

- [ ] **Step 1: Write failing tests.**
  - `square_trimmed_by_a_circle_keeps_the_outside`: 40 x 40 patch, circle of diameter 20: area `1600 - 314.16` within 0.1 %.
  - `keep_point_selects_the_inside`: area 314.16.
  - `trim_by_another_surface`: two crossing planes patches: the kept half has area 800.
  - `tool_that_misses_is_refused`.
  - `planar_extend`: the 40 x 40 patch extended by 5 at one edge: area 1800 within 0.1 %.
  - `cylinder_extend_keeps_the_radius`: a cylinder surface extended by 5 along its axis: area grows by `2 * pi * r * 5`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 6: Split body and replace face

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`, `crates/faber-core/src/history.rs`
- Test: `crates/faber-wiring/tests/surfaces.rs`

**Interfaces:**

```rust
/// Cut a solid with a surface that crosses it. Returns the pieces, each with a point inside.
fn split_by_surface(&self, _solid: &Shape, _tool: &Shape) -> Result<Vec<Shape>>;
```

- `ReplaceFace`: extend the neighbour faces of the selected face to the surface and trim them with it. Method: `remove_faces` is not used; cut the body with the surface (`split_by_surface`) when the surface is below the face, or add the prism between the face and the surface when it is above, then keep the piece on the body side.

- [ ] **Step 1: Write failing tests** (box 20 mm):
  - `split_by_a_tilted_plane_surface`: the two volumes sum to 8000 within 1e-6.
  - `split_by_a_curved_surface`: a lofted surface through the box: two pieces; the sum is 8000 within 0.01 %.
  - `replace_top_with_a_lower_curved_surface`: the volume is below 8000; the top face is not planar; the four side faces are still planar.
  - `replace_top_with_a_higher_surface`: the volume is above 8000.
  - `surface_too_small_for_the_face_is_refused`: `The surface does not cover face <n>. Extend the surface.`
  - `surface_that_misses_the_body_is_refused`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 7: Viewer, guide, example

**Files:**
- Modify: `crates/faber-viewer/src/app/feature_tools.rs`, `crates/faber-viewer/src/feature_form.rs`, `crates/faber-viewer/src/app.rs` (surfaces drawn two-sided, with open edges in orange), `crates/faber-build/src/faber.libsonnet`, `docs/modelling-guide.md` (section `## Surfaces`)
- Create: `examples/surfaced-handle/` (lofted surfaces, a patch, a stitch, then a shell)

- [ ] **Step 1: Write failing viewer tests:** `surface_features_are_in_the_feature_list`, `open_edges_are_highlighted`, `surface_feature_is_visible_when_selected`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.
