# Print preparation: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tell the user before the print: does the part fit the printer, which
side goes down, how much support it needs, where a wall is too thin, and do
the mating parts have the correct clearance. Export 3MF for a slicer.

**Architecture:** A new pure Rust crate `faber-print` works on meshes with
`faber-geometry::bvh` and the existing overhang analysis of `faber-core`. The
printer profile is project data in `manufacture.json`. 3MF is a mesh exchange
format in `faber-files`.

**Tech Stack:** Rust 2024, `rayon`, `zip`, `quick-xml`.

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` (decision D2; shared rules).

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Needs chunk 01.
- Faber does not slice and does not make support geometry. A support volume is
  an estimate. Each reply says so in `limits`.
- Task 1 changes `faber-model`: the coordinator commits that change alone.
- Printer and clearance numbers are practice values. Each table has a comment
  that says so and names its source.

## Review Focus

- A part larger than the bed in each orientation gets a clear answer, not an
  empty ranking (Task 3: `too_large_part_is_reported`).
- A mesh that is not closed is refused for the wall and support checks with
  the count of open edges (Task 2: `open_mesh_is_refused`).
- An assembly check uses the placed parts, not the unplaced ones (Task 2:
  `check_uses_the_placement`).
- `orient_for_print` with `apply: true` on a purchased part moves it and does
  not change its geometry (Task 3: `orientation_changes_only_the_placement`).
- Two coaxial cylinders with equal radius give clearance 0, not a division by
  zero (Task 4: `equal_radius_is_a_zero_clearance_fit`).

---

### Task 1: Printer profile

**Files:**
- Modify: `crates/faber-model/src/lib.rs` (`Printer`, `ProjectData.printer`, `Edit::SetPrinter`)
- Modify: `crates/faber-files/src/layout.rs` (new file `manufacture.json`: `{schema: 1, printer}`)
- Create: `crates/faber-std/src/printers.rs`
- Modify: `src/server.rs`
- Test: `crates/faber-files/tests/layout.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
pub struct Printer { pub name: String, pub bed_mm: [f64; 3], pub nozzle_mm: f64, pub layer_mm: f64,
    pub min_wall_mm: f64,        // default 2 x nozzle
    pub overhang_deg: f64,       // default 45, from the vertical
    pub bridge_mm: f64,          // default 10
    pub clearance_mm: Clearance } // press 0.0, snug 0.15, slide 0.25, loose 0.4 (diametral, practice values)
```

- Profiles in `faber-std`: `generic-220` (220 x 220 x 250), `bambu-a1` (256 x 256 x 256), `bambu-a1-mini` (180 x 180 x 180), `prusa-mk4` (250 x 210 x 220), `ender-3` (220 x 220 x 250). All with nozzle 0.4 and layer 0.2.
- MCP: `set_printer {profile? , printer?}` (a profile id or a full object), `printers` (list). A project with no printer uses `generic-220`.

- [ ] **Step 1: Write failing tests:** `manufacture_file_round_trips`, `project_without_manufacture_file_reads_the_default`, `mcp.rs::set_printer_by_profile`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-files --test layout -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** Add `manufacture.json` to the layout fingerprint.
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Print check

**Files:**
- Create: `crates/faber-print/Cargo.toml`, `src/lib.rs`, `src/wall.rs`, `src/support.rs`, `src/check.rs`
- Test: `crates/faber-print/tests/check.rs`

**Interfaces:**

```rust
pub struct Finding { pub kind: Kind, pub severity: Severity, pub message: String,
                     pub at: [f64; 3], pub value: f64, pub limit: f64, pub area_mm2: f64 }
pub enum Kind { TooLarge, ThinWall, Overhang, Island, SmallContact, LongBridge, TallAndNarrow }
pub struct PrintReport { pub fits: bool, pub size_mm: [f64; 3], pub height_mm: f64, pub layers: u32,
    pub bed_contact_mm2: f64, pub overhang_mm2: f64, pub support_mm3: f64, pub thin_wall_mm2: f64,
    pub min_wall_mm: f64, pub islands: u32, pub findings: Vec<Finding>, pub limits: Vec<String> }
pub fn check(meshes: &[&Mesh], printer: &Printer, up: [f64; 3]) -> Result<PrintReport>;
pub fn wall_thickness(mesh: &Mesh, bvh: &Bvh) -> Vec<f32>;  // per triangle; ray from the centroid along the inward normal
pub fn support_volume(mesh: &Mesh, bvh: &Bvh, up: [f64; 3], overhang_deg: f64) -> f64;
pub fn islands(mesh: &Mesh, up: [f64; 3], bed: f64) -> Vec<[f64; 3]>; // local low points that are not on the bed
```

- Support volume: for each overhang triangle, its projected area times the
  distance from its centroid down to the first hit (the part or the bed).
- `TallAndNarrow`: height divided by the square root of the bed contact area is above 8.
- `SmallContact`: bed contact below 25 mm2.
- A finding joins connected triangles into one region and reports its centre and area.

- [ ] **Step 1: Write failing tests.**
  - `shelled_box_wall_is_measured`: box 20 mm with a 1.2 mm shell: `min_wall_mm` is 1.2 within 0.05. With `min_wall_mm: 1.6` in the printer: `thin_wall_mm2 > 2000` and one `ThinWall` finding for each connected region.
  - `solid_box_has_no_thin_wall`.
  - `table_support_volume`: a plate 20 x 20 x 2 at Z 10 to 12 on a centre post 4 x 4 x 10: `support_mm3` within 5 % of 3840.
  - `l_bracket_flat_needs_no_support`.
  - `hanging_tooth_is_an_island`: a ceiling with a spike that points down gives `islands == 1`; an arch gives 0.
  - `too_large_part`: a 300 mm bar on `bambu-a1`: `fits == false` and the message `Size 300 mm along X is larger than the bed 256 mm.`
  - `open_mesh_is_refused`: `The mesh is open (<n> open edges). Repair it first.`
  - `check_uses_the_placement`: the same part turned 90 degrees gives a different `height_mm`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-print -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** Use `faber_core::overhang` for the overhang area.
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Orientation search

**Files:**
- Create: `crates/faber-print/src/orient.rs`
- Modify: `crates/faber-wiring/src/lib.rs` (`print_check`, `orient_for_print`), `src/server.rs`
- Test: `crates/faber-print/tests/orient.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
pub struct Candidate { pub rotation_deg: [f64; 3], pub down: [f64; 3], pub score: f64,
    pub support_mm3: f64, pub overhang_mm2: f64, pub height_mm: f64, pub bed_contact_mm2: f64, pub fits: bool }
pub fn rank(mesh: &Mesh, printer: &Printer, weights: Weights) -> Result<Vec<Candidate>>; // best first, 5 at most
```

- Candidates: the normal of each planar region with an area above 2 % of the
  surface (the region goes on the bed), then the six axis directions. At most
  40 candidates; equal directions within 1 degree count once.
- Score (lower is better): `support_mm3 / volume + 0.5 * overhang_mm2 / area + 0.2 * height / longest size - 0.3 * bed_contact / area`. A candidate that does not fit is last.
- MCP: `print_check {parts?, up?}` returns the report of Task 2. `orient_for_print {part, apply?, strength_direction?}` returns the ranking. With `apply: true` it turns the part about its bounds centre, puts its lowest point on Z 0 and saves one revision. `strength_direction` adds a penalty when the layer plane is normal to that direction.

- [ ] **Step 1: Write failing tests.**
  - `l_bracket_best_side_has_no_support`: `rank(...)[0].support_mm3 == 0`.
  - `t_part_does_not_stand_on_its_stem`.
  - `cylinder_stands_on_its_end`: for a cylinder of radius 10 and height 5 the best `down` is along the axis.
  - `too_large_part_is_reported`: every candidate has `fits == false` and the reply has `fits_any: false` with the sizes.
  - `mcp.rs::orientation_changes_only_the_placement`: the part file is byte-equal; `placement.json` changed; the lowest Z of the bounds is 0 within 1e-6.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Fit and tolerance

**Files:**
- Create: `crates/faber-print/src/fit.rs`
- Modify: `crates/faber-wiring/src/lib.rs`, `src/server.rs`
- Test: `crates/faber-print/tests/fit.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
pub struct CylinderFit { pub a_face: u32, pub b_face: u32, pub axis_point: [f64; 3], pub hole_d: f64, pub shaft_d: f64,
                         pub clearance_mm: f64, pub class: FitClass } // Interference | Press | Snug | Slide | Loose
pub fn cylinder_fits(a: &[FaceInfo], b: &[FaceInfo], clearance: &Clearance) -> Vec<CylinderFit>; // coaxial within 0.01 mm and 0.1 degree
pub struct Stack { pub nominal: f64, pub worst: [f64; 2], pub rss: [f64; 2] }
pub fn stack(items: &[StackItem]) -> Stack; // StackItem { nominal, plus, minus, sign }
```

- MCP: `fit_check {a, b}` and `tolerance_stack {items}`. `fit_check` uses the placed face data of both parts.

- [ ] **Step 1: Write failing tests.**
  - `pin_in_hole_is_a_slide_fit`: hole 5.25, shaft 5.00 gives clearance 0.25 and `Slide`.
  - `equal_radius_is_a_zero_clearance_fit`: `Press`, clearance 0.
  - `larger_shaft_is_interference`.
  - `offset_axes_are_not_a_fit`: axes 0.5 mm apart give no entry.
  - `stack_of_three`: nominals 10, 20, -5 with +-0.1 each: nominal 25, worst +-0.3, RSS +-0.1732 within 1e-4.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-print --test fit -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: 3MF export, viewer panel, guide

**Files:**
- Create: `crates/faber-files/src/threemf.rs`, `crates/faber-viewer/src/app/print_check.rs`
- Modify: `crates/faber-wiring/src/lib.rs` (export format `3mf`), `src/main.rs`, `src/server.rs`, `crates/faber-viewer/src/app.rs`, `docs/modelling-guide.md` (section `## Print check`)
- Test: `crates/faber-files/tests/threemf.rs`, viewer test in `print_check.rs`

- 3MF: one `object` for each part with its colour as a base material, one build `item` for each part with the placement matrix, unit `millimeter` (the 3MF word). All parts go in one file.
- Viewer panel "Print check": printer selector, the numbers of the report, a list of findings (a click moves the camera to the finding), a "Best orientation" list with "Apply", and a thin-wall colour overlay.

- [ ] **Step 1: Write failing tests.**
  - `threemf_has_one_object_for_each_part`: unzip, parse `3D/3dmodel.model`: 2 objects, the vertex and triangle counts equal the meshes, 2 build items with 12-number transforms.
  - `threemf_has_the_content_types_file`.
  - Viewer: `print_panel_lists_the_findings`, `apply_turns_the_part`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.** Add `examples/print-check/` with one part that has a thin wall and an island.
- [ ] **Step 4: Run** the tests and `cargo test --offline -j 4 --test mcp --test context_size -- --test-threads=1`. Expected: pass.
