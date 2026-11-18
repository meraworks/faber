# Static stress study: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Answer "does this part hold this load" with numbers and a colour map:
von Mises stress, displacement, safety factor and the most loaded points.

**Architecture:** A new pure Rust crate `faber-sim` puts the part in a regular
voxel grid. Each cell has a fill fraction (finite cell method) and one shared
hexahedral element matrix. A matrix-free conjugate gradient solver gives the
displacement. `faber-wiring` makes the grid from the part mesh and the face
triangles, and maps the result back to the mesh as colours.

**Tech Stack:** Rust 2024, `rayon`, `faber-geometry::bvh`, the material library.

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` (decisions D1, D7, D8; shared
rules 11, 14, 15).

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Needs chunk 01.
- Units inside `faber-sim`: mm, N, MPa. Force / area = MPa with no factor.
- Task 3 changes `faber-model`: the coordinator commits that change alone.
- No dependency for linear algebra. The solver is in the crate.
- Each reply carries these limits: `Linear elastic material. Small
  displacement.`, `One part. No contact.`, `Voxel grid of <h> mm: stress at a
  curved surface is approximate.`, and for a printed polymer `Layer direction
  strength is yield x <factor>; set the print direction to use it.`

## Review Focus

- A study with no fixture, or a fixture that leaves a free rigid motion, is
  refused before the solve (Task 2: `free_body_is_refused`).
- A load face and a fixture face that are the same face are refused (Task 3:
  `load_on_a_fixed_face_is_refused`).
- A part of two separate bodies, where one has no fixture, is refused with the
  count of loose cells (Task 2: `loose_island_is_refused`).
- A grid above the cell limit is refused with the count and the limit (Task 1:
  `too_fine_grid_is_refused`).
- A solve that does not converge returns an error, not a wrong result (Task 2:
  `no_convergence_is_an_error`).

---

### Task 1: Voxel grid

**Files:**
- Create: `crates/faber-sim/Cargo.toml`, `crates/faber-sim/src/lib.rs`, `crates/faber-sim/src/grid.rs`
- Test: unit tests in `grid.rs`

**Interfaces:**

```rust
pub struct Grid { pub origin: [f64; 3], pub h: f64, pub n: [usize; 3], pub fill: Vec<f32> } // fill 0..=1, index x + nx*(y + ny*z)
pub const CELL_LIMIT: usize = 4_000_000;
pub fn default_h(bounds: [[f64; 3]; 2]) -> f64;          // 150 000 cells in the bounds box, clamp 0.2..=10.0
pub fn voxelise(mesh: &Mesh, bvh: &Bvh, h: f64) -> Result<Grid>;
pub fn cells_near(grid: &Grid, triangles: &[[[f64; 3]; 3]]) -> Vec<usize>; // active cells that touch the triangles
impl Grid { pub fn volume(&self) -> f64; pub fn node(&self, i: usize, j: usize, k: usize) -> usize; }
```

- Fill: a cell fully inside is 1, fully outside is 0. A cell that the surface
  crosses gets the inside share of a 3 x 3 x 3 sample set. A cell with fill
  below 0.02 is void.
- The grid has one cell of margin on each side.

- [ ] **Step 1: Write failing tests.**
  - `box_volume_is_exact`: box 10 x 20 x 30 at `h = 1.0` aligned to the grid: `volume() == 6000.0` within 1e-6.
  - `sphere_volume_is_close`: sphere of radius 10, `h = 1.0`: volume within 1.5 % of 4188.79.
  - `rotated_box_volume_is_close`: the box turned 30 degrees about Z: within 1.5 % of 6000.
  - `default_h_gives_about_the_target`: bounds 100 x 50 x 20 gives between 100 000 and 200 000 cells.
  - `too_fine_grid_is_refused`: box 100 mm, `h = 0.2`: error `The grid has 125000000 cells. The limit is 4000000. Use a larger resolution.`
  - `cells_near_finds_the_top_layer`: the top face triangles of the box give exactly the 200 top cells.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-sim grid -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement** with `rayon` over Z slices.
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Elastic solver

**Files:**
- Create: `crates/faber-sim/src/element.rs`, `crates/faber-sim/src/solve.rs`, `crates/faber-sim/src/stress.rs`
- Test: `crates/faber-sim/tests/elastic.rs`

**Interfaces:**

```rust
pub struct Elastic { pub youngs_mpa: f64, pub poisson: f64 }
pub enum Support { Fixed { cells: Vec<usize> }, Slide { cells: Vec<usize>, normal: [f64; 3] } }
pub enum Load { Force { cells: Vec<usize>, newtons: [f64; 3] },        // spread on the outer nodes of the cells
                Pressure { triangles: Vec<[[f64; 3]; 3]>, mpa: f64 },  // along the inward normal
                Gravity { direction: [f64; 3], density_g_cm3: f64 } }
pub struct StaticCase { pub grid: Grid, pub material: Elastic, pub supports: Vec<Support>, pub loads: Vec<Load>,
                        pub tolerance: f64 /* 1e-6 */, pub max_iterations: usize /* 20 000 */ }
pub struct StaticResult { pub displacement: Vec<[f64; 3]>, pub von_mises: Vec<f32>, // node and cell values
                          pub iterations: usize, pub residual: f64, pub reaction: [f64; 3] }
pub fn solve_static(case: &StaticCase) -> Result<StaticResult>;
pub fn element_matrix(material: &Elastic, h: f64) -> [[f64; 24]; 24]; // 8-node hexahedron, 2 x 2 x 2 Gauss points
```

- Cell stiffness = `fill * element_matrix`. Void cells are not in the system.
  A node with no active cell is not a variable.
- Solver: conjugate gradient with a diagonal preconditioner, matrix-free. The
  product runs in parallel with an 8-colour split of the cells.
- Stress: at the cell centre from the node displacements; then the mean of the
  touching cells at each node; `von_mises` of a cell is the mean of its nodes.
  Cells with fill below 0.5 take the value of the nearest cell with fill 0.5
  or more.

- [ ] **Step 1: Write failing tests** (`E` in MPa, sizes in mm):
  - `axial_bar`: bar 10 x 10 x 100 along Z, `E = 1000`, `nu = 0`, `h = 2.5`. Fixed at Z = 0, force (0, 0, 1000) on the end cells. Tip displacement 1.000 within 2 %. Mean von Mises 10.0 within 2 %. `reaction` equals (0, 0, -1000) within 0.1 %.
  - `cantilever_tip`: the same bar, `E = 2000`, `nu = 0.3`, `h = 1.25`, force (0, -10, 0) at the end. Tip deflection within 8 % of 2.000 (`F L^3 / (3 E I)`, `I = 833.33`).
  - `rotated_bar_matches`: the axial bar turned 30 degrees about X, `h = 2.0`: tip displacement along the bar axis within 5 % of 1.000.
  - `plate_with_hole`: plate 100 x 40 x 4 with a centre hole of diameter 10, pulled with 1 000 N, `h = 0.5`: the largest von Mises divided by the net-section stress `1000 / ((40 - 10) * 4)` is between 2.0 and 3.2.
  - `gravity_bar`: a bar 10 x 10 x 100 fixed at the top, density 7.85: the reaction equals the weight `78.5 g * 9.80665e-3 N/g` within 1 %.
  - `free_body_is_refused`: no support returns `The study has no fixture. Add a fixed face.` One `Slide` support only returns `The fixtures leave the part free to move. Add a fixed face.`
  - `loose_island_is_refused`: two boxes with a gap, one fixed: `<n> cells have no connection to a fixture.`
  - `no_convergence_is_an_error`: `max_iterations = 3` returns `The solver did not converge in 3 iterations (residual <r>).`
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-sim --test elastic -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** Find loose cells with a flood fill from the fixed cells before the solve.
- [ ] **Step 4: Run** in release for speed: `cargo test --offline -j 4 --release -p faber-sim --test elastic -- --test-threads=1`. Expected: pass. Record the time of each test.

### Task 3: Study data and tools

**Files:**
- Modify: `crates/faber-model/src/lib.rs` (`Study`, `ProjectData.studies`, `Edit::SetStudy`, `Edit::RemoveStudy`)
- Modify: `crates/faber-files/src/layout.rs` (`analysis.json` gains `studies`)
- Create: `crates/faber-wiring/src/study.rs`
- Modify: `crates/faber-wiring/src/lib.rs`, `crates/faber-core/src/batch.rs`, `src/server.rs`
- Test: `crates/faber-wiring/tests/study.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
pub struct Study<N = f64> { pub part: String, pub fixtures: Vec<Fixture<N>>, pub loads: Vec<StudyLoad<N>>,
    pub material: Option<String>, pub resolution_mm: Option<N>, pub print_direction: Option<[N; 3]>, pub note: String }
pub struct Fixture<N> { pub face: FaceSelector<N>, pub kind: FixtureKind } // fixed | slide
pub enum StudyLoad<N> { Force { face: FaceSelector<N>, newtons: [N; 3] }, Pressure { face: FaceSelector<N>, mpa: N },
                        Gravity { direction: [N; 3] } }
```

```rust
// faber-wiring/src/study.rs
pub struct StressReport { pub study: String, pub part: String, pub material: String,
    pub max_von_mises_mpa: f64, pub max_displacement_mm: f64, pub min_safety_factor: f64,
    pub hot_spots: Vec<HotSpot>,            // 5 at most, at least 3 cells apart: point, von_mises_mpa, safety_factor
    pub reaction_n: [f64; 3], pub mass_g: f64,
    pub method: String, pub resolution_mm: f64, pub cells: usize, pub iterations: usize, pub limits: Vec<String> }
pub fn run_stress(project: &Project, id: &str, resolution_mm: Option<f64>) -> Result<(StressReport, StudyField)>;
pub struct StudyField { pub vertex_von_mises: Vec<f32>, pub vertex_displacement: Vec<[f32; 3]> } // on the body mesh
```

- Safety factor = yield / stress. With `print_direction` set and a polymer,
  the reply also gives `layer_safety_factor`: yield x layer factor divided by
  the largest normal stress along the print direction.
- MCP tools: `set_study {id, study}`, `remove_study {id}`, `run_study {id, resolution_mm?, render?: {path, width?, height?, deform_scale?}}`, and `quick_stress {part, fixed: FaceSelector, force: {face, newtons}, material?}` that runs with no saved study. `batch` accepts `set_study` and `remove_study`.
- The result is in the read cache for the revision, the study and the resolution.

- [ ] **Step 1: Write failing tests.**
  - `study.rs::bracket_report`: an L bracket (two boxes joined), PLA, fixed back face, 50 N down on the end face: `min_safety_factor > 0`, `hot_spots[0]` is within 3 mm of the inner corner, `reaction_n[2]` equals 50 within 1 %.
  - `study.rs::study_survives_a_parameter_change`: change the arm length; the same study runs and the displacement grows.
  - `study.rs::load_on_a_fixed_face_is_refused`.
  - `study.rs::missing_material_is_named`: `Part bracket has no material. Set a material or give one in the study.`
  - `study.rs::purchased_part_can_be_studied` (a study reads; it changes nothing).
  - `mcp.rs::quick_stress_replies_small`: the reply is below 2 000 characters and has `limits`.
  - `mcp.rs::run_study_writes_the_picture`: the PNG exists and has red and blue pixels.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.** Colour map: blue (0) through green and yellow to red (largest value), on the body mesh by the nearest active cell. The render uses the vertex colours and draws a legend with five values.
- [ ] **Step 4: Run** `cargo test --offline -j 4 --release -p faber-wiring --test study -- --test-threads=1` and `cargo test --offline -j 4 --test mcp --test context_size -- --test-threads=1`. Expected: pass.

### Task 4: Viewer study panel

**Files:**
- Create: `crates/faber-viewer/src/app/study.rs`
- Modify: `crates/faber-viewer/src/app.rs`, `crates/faber-viewer/src/analysis.rs`
- Test: in `study.rs` with `egui_kittest`

- Panel "Stress study", docked at the right: material, a list of fixtures and loads, "Pick face" for each, the force vector fields, resolution, "Run".
- The solve runs on a worker thread. The panel shows the iteration count and has "Stop".
- After the run: the colour map on the part, a legend, the five hot spots as markers, a "Deformed shape" switch with a scale field, and the three numbers (largest stress, largest displacement, least safety factor). A safety factor below 1 is red text.
- The limits are shown below the numbers.

- [ ] **Step 1: Write failing tests:** `study_panel_runs_and_shows_the_factor` (headless; the bracket example; the label with the safety factor appears), `stop_ends_the_worker`, `result_clears_after_a_geometry_change`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer study -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Example, guide and timing

**Files:**
- Create: `examples/shelf-bracket-study/` (project with one saved study)
- Modify: `docs/modelling-guide.md` (section `## Stress study`), `crates/faber-build/src/faber.libsonnet` (`study`, `fixed`, `force` helpers), `README.md`, `docs/specs/faber-progress.md`

- [ ] **Step 1:** Make the example: a PLA shelf bracket, fixed at two screw faces, 100 N on the shelf face.
- [ ] **Step 2:** Run `run_study` in a release build at the default resolution and at half the cell size. Record: cells, iterations, time, largest stress and safety factor for both. Target: the default run takes less than 20 s. The two largest stress values differ by less than 20 %; record the difference in the guide as the expected accuracy.
- [ ] **Step 3:** Write the guide section: the three steps (material, fixture, load), how to read the reply, and the limits.
