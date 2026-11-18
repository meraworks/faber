# Surface analysis: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show the quality and the manufacturability of the surfaces of a
part: curvature, smallest radius, draft against a pull direction, access from
a tool direction, and zebra stripes for surface continuity.

**Architecture:** One new kernel call gives the principal curvatures at the
vertices of the face mesh of chunk 01. `faber-core` makes the reports. Access
uses ray queries. The viewer gets colour overlays through the vertex colour
path of the stress study and one new shader mode for zebra stripes.

**Tech Stack:** Rust 2024, OCCT 7.9.3 (`BRepLProp_SLProps`), wgpu shader.

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` and the Inspect table of
`docs/specs/2026-11-18-fusion-feature-gap.md`.

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Needs chunk 01 (face meshes, ray
  queries).
- Task 1 changes `bridge.cpp`.
- Curvature unit is 1/mm. Sign: positive for a convex surface seen from
  outside the solid, negative for a concave one.
- Each report is a read. It never changes the project. It works on a purchased
  part.

## Review Focus

- A flat face has curvature exactly 0 and no radius (Task 1:
  `plane_has_zero_curvature`).
- A mesh-only part gets a curvature estimate with a limit sentence, not an
  error (Task 2: `mesh_part_gets_an_estimate`).
- A face that is exactly vertical is "vertical", not "undercut", with a
  tolerance of 0.01 degree (Task 3: `vertical_face_is_not_an_undercut`).
- A pull direction of zero length is refused (Task 3:
  `zero_direction_is_refused`).
- A point hidden by another part of the assembly is hidden when `scene` is set
  (Task 4: `other_parts_block_access`).

---

### Task 1: Curvature at mesh vertices

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`
- Test: `crates/faber-kernel-occt/tests/curvature.rs`

**Interfaces:**

```rust
pub struct CurvedMesh { pub faces: FaceMesh, pub curvature: Vec<[f64; 2]> } // per vertex: [min, max] principal curvature
fn tessellate_curvature(&self, _shape: &Shape, _tolerance: f64) -> Result<CurvedMesh>;
```

- [ ] **Step 1: Write failing tests.**
  - `plane_has_zero_curvature`: each vertex of a box has `[0.0, 0.0]` exactly.
  - `sphere_is_uniform`: radius 10 gives `[0.1, 0.1]` within 1e-9.
  - `cylinder_has_one_direction`: radius 5 gives `[0.0, 0.2]`.
  - `hole_is_concave`: the face of a hole of radius 2 gives `[-0.5, 0.0]`.
  - `fillet_radius_is_found`: an edge fillet of radius 2 gives max curvature 0.5 on its face.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-kernel-occt --test curvature -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** Use the UV nodes of the face triangulation. Flip the sign for a reversed face.
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Curvature report

**Files:**
- Create: `crates/faber-core/src/surface.rs`
- Modify: `crates/faber-wiring/src/lib.rs`, `src/server.rs`
- Test: `crates/faber-wiring/tests/surface.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
pub struct FaceCurvature { pub face: u32, pub kind: FaceKind /* Plane Cylinder Sphere Cone Torus Freeform */,
                           pub min_convex_radius: Option<f64>, pub min_concave_radius: Option<f64>, pub area: f64 }
pub struct CurvatureReport { pub min_convex_radius_mm: Option<f64>, pub min_concave_radius_mm: Option<f64>,
    pub smallest_concave_faces: Vec<u32>, pub faces: Vec<FaceCurvature>, pub limits: Vec<String> }
pub fn curvature_report(mesh: &CurvedMesh, faces: &[FaceInfo]) -> CurvatureReport;
pub fn curvature_colours(mesh: &CurvedMesh, mode: CurvatureMode /* Mean | Gauss | MinRadius */, range: Option<[f64; 2]>) -> Vec<[u8; 3]>;
pub fn estimate_curvature(mesh: &Mesh) -> Vec<[f64; 2]>; // for a mesh-only part
```

- MCP: `curvature {part, tool_radius_mm?, render?: {path, mode?}}`. With `tool_radius_mm` the reply lists the faces that a tool of that radius cannot reach (concave radius below the tool radius), with their area.

- [ ] **Step 1: Write failing tests.**
  - `pocket_corner_radius`: a pocket with corner fillets of 1.5: `min_concave_radius_mm == 1.5` within 1e-6 and `smallest_concave_faces` has the 4 fillet faces.
  - `outer_fillet_is_convex`: `min_convex_radius_mm == 2.0`.
  - `tool_three_does_not_fit_the_corner`: `tool_radius_mm: 3` lists the 4 faces.
  - `mesh_part_gets_an_estimate`: a sphere mesh of radius 10 gives a radius within 5 % of 10 and the limit `Curvature of a mesh part is an estimate.`
  - `mcp.rs::curvature_reply_is_small`: `faces` is left out unless `detail: true`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Draft report

**Files:**
- Modify: `crates/faber-core/src/surface.rs`, `crates/faber-wiring/src/lib.rs`, `src/server.rs`
- Test: `crates/faber-wiring/tests/surface.rs`

**Interfaces:**

```rust
pub enum DraftClass { Positive, Negative, Vertical, Undercut, BelowMinimum }
pub struct DraftFace { pub face: u32, pub class: DraftClass, pub min_deg: f64, pub max_deg: f64, pub area: f64 }
pub struct DraftReport { pub faces: Vec<DraftFace>, pub vertical_mm2: f64, pub below_minimum_mm2: f64, pub undercut_mm2: f64,
                         pub parting_z: Option<f64>, pub limits: Vec<String> }
pub fn draft_report(mesh: &FaceMesh, pull: [f64; 3], min_deg: f64) -> Result<DraftReport>;
```

- The draft angle of a triangle is 90 degrees minus the angle between its normal and the pull direction. Positive faces look along the pull; negative faces look against it.
- Undercut: a positive triangle that a ray along the pull direction hits again, or a negative triangle that a ray against it hits again.

- [ ] **Step 1: Write failing tests.**
  - `plain_box_has_four_vertical_faces`: `vertical_mm2` is the side area.
  - `drafted_box_passes`: 2 degrees of draft, `min_deg: 1`: no vertical face, no undercut.
  - `one_degree_is_below_a_two_degree_minimum`.
  - `vertical_face_is_not_an_undercut`.
  - `side_hole_is_an_undercut`: a hole across the pull direction gives `undercut_mm2 > 0`.
  - `zero_direction_is_refused`: `The pull direction has zero length.`
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test surface -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** MCP: `draft_analysis {part, pull, min_deg?, render?}`.
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Access report

**Files:**
- Modify: `crates/faber-core/src/surface.rs`, `crates/faber-wiring/src/lib.rs`, `src/server.rs`
- Test: `crates/faber-wiring/tests/surface.rs`

**Interfaces:**

```rust
pub struct AccessReport { pub reachable_mm2: f64, pub hidden_mm2: f64, pub hidden_faces: Vec<u32>, pub limits: Vec<String> }
pub fn access_report(mesh: &FaceMesh, bvh: &Bvh, directions: &[[f64; 3]], scene: Option<&Bvh>) -> AccessReport;
```

- A triangle is reachable when a ray from its centroid (moved 1e-6 along its normal) along one of the directions hits nothing.
- A triangle that looks away from a direction is not reachable from that direction.

- [ ] **Step 1: Write failing tests:** `box_from_above_reaches_the_top_only` (the top area, with the limit `Side faces count as hidden for a direction along them.`), `three_directions_reach_more`, `t_slot_has_a_hidden_area`, `other_parts_block_access`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.** MCP: `access_analysis {part, directions, scene?}`.
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Viewer overlays and zebra stripes

**Files:**
- Modify: `crates/faber-render/src/shader.wgsl`, `crates/faber-render/src/lib.rs`, `crates/faber-viewer/src/analysis.rs`, `crates/faber-viewer/src/app.rs`, `docs/modelling-guide.md` (section `## Surface analysis`)
- Test: `crates/faber-render` tests, viewer tests in `analysis.rs`

- Analysis menu: "Curvature" (mean, Gauss, smallest radius; a legend), "Draft" (green positive, blue negative, yellow below the minimum, red undercut; a pull direction selector), "Access", "Zebra".
- Zebra: the fragment colour is black or white from `fract(dot(reflect(view, normal), stripe_axis) * count)`; `count` is a setting (default 12).
- The viewer also shows the centre of mass as a marker when "Centre of mass" is on.

- [ ] **Step 1: Write failing tests.**
  - Render: `zebra_differs_from_plain_shading`; `zebra_on_a_flat_plate_has_straight_stripes` (each image row of the plate has the same stripe count).
  - Viewer: `curvature_overlay_shows_a_legend`, `draft_overlay_colours_a_vertical_face_yellow`, `overlay_clears_after_a_geometry_change`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-render -p faber-viewer -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.
