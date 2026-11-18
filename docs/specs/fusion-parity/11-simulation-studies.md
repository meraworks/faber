# Simulation studies: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add three study kinds on the grid of the static stress study:
natural frequencies, steady heat flow with thermal stress, and shape
optimisation that removes the material a load case does not need.

**Architecture:** All three use the voxel grid, the element code and the
conjugate gradient solver of `faber-sim`. Modal analysis adds a lumped mass
and a subspace iteration. Thermal analysis adds a scalar element. Shape
optimisation runs the static solver in a loop and changes a density for each
cell; the result is a mesh from the density field.

**Tech Stack:** Rust 2024, `rayon`, `faber-sim`.

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` (decision D1). This chunk is
the Fusion "Simulation" study list without buckling, non-linear and event
studies, and it stands for "Generative Design" in a reduced form.

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Needs chunk 02.
- Task 4 changes `faber-model` (`Study` gains a `kind`).
- A saved static study of chunk 02 has no `kind` field. It reads as `static`.
- Dynamics use the unit set mm, N, s and tonne: density in tonne/mm3 is the
  g/cm3 value times 1e-9. Frequencies are in hertz in every reply.
- Buckling, non-linear material, contact and transient studies are not in this
  chunk. The guide says so.

## Review Focus

- A modal study with no fixture returns the six rigid modes near 0 Hz and says
  so, in place of a refusal (Task 1: `free_body_has_six_zero_modes`).
- A request for more modes than the grid has degrees of freedom is refused
  (Task 1: `too_many_modes_are_refused`).
- A thermal study with heat input and no fixed temperature or convection is
  refused: it has no steady state (Task 2: `no_heat_sink_is_refused`).
- A shape study whose kept regions already exceed the target volume is refused
  with both numbers (Task 3: `kept_regions_above_the_target_are_refused`).
- A shape result with loose pieces is reported with the count of pieces
  (Task 3: `loose_pieces_are_reported`).

---

### Task 1: Natural frequencies

**Files:**
- Create: `crates/faber-sim/src/modal.rs`, `crates/faber-sim/src/eigen.rs`
- Test: `crates/faber-sim/tests/modal.rs`

**Interfaces:**

```rust
pub struct ModalCase { pub grid: Grid, pub material: Elastic, pub density_g_cm3: f64, pub supports: Vec<Support>, pub modes: usize }
pub struct Mode { pub frequency_hz: f64, pub shape: Vec<[f32; 3]> /* per node, largest length 1 */, pub mass_share: [f64; 3] }
pub fn solve_modal(case: &ModalCase) -> Result<Vec<Mode>>;
pub(crate) fn symmetric_eigen(a: &mut Dense) -> Vec<f64>; // Jacobi rotations, for the small projected problem
```

- Mass: lumped at the nodes, `fill * density * h^3 / 8` from each cell.
- Method: subspace iteration with a block of `modes + 4` vectors. Each step solves `K x = M y` with the solver of chunk 02, then a Rayleigh-Ritz step. Stop when each wanted frequency changes by less than 0.1 %.

- [ ] **Step 1: Write failing tests.**
  - `cantilever_first_frequency`: bar 10 x 10 x 100, `E = 200000`, `nu = 0.3`, density 7.85, fixed at one end, `h = 1.25`: the first frequency is within 8 % of 815.4 Hz. The first two modes are a pair (bending in X and in Y) within 1 %.
  - `axial_mode`: the first axial mode of the same bar is within 5 % of `sqrt(E / rho) / (4 L) = 12 619 Hz` (with `nu = 0`).
  - `modes_are_sorted_and_orthogonal`: the mass-weighted product of two different shapes is below 1e-6.
  - `free_body_has_six_zero_modes`: no support: 6 modes below 1 Hz, then the first elastic mode; the reply has the limit `No fixture: the first 6 modes are rigid motion.`
  - `too_many_modes_are_refused`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 --release -p faber-sim --test modal -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** For a free body, shift the stiffness by a small multiple of the mass so the solver has a solution.
- [ ] **Step 4: Run.** Expected: pass. Record the times.

### Task 2: Heat flow and thermal stress

**Files:**
- Create: `crates/faber-sim/src/thermal.rs`
- Modify: `crates/faber-sim/src/solve.rs` (`Load::Temperature { field: Vec<f32>, reference_c: f64, expansion_um_mk: f64 }`)
- Test: `crates/faber-sim/tests/thermal.rs`

**Interfaces:**

```rust
pub enum ThermalBc { Temperature { cells: Vec<usize>, celsius: f64 }, Power { cells: Vec<usize>, watts: f64 },
                     Convection { triangles: Vec<[[f64; 3]; 3]>, w_m2k: f64, ambient_c: f64 } }
pub struct ThermalCase { pub grid: Grid, pub conductivity_w_mk: f64, pub conditions: Vec<ThermalBc> }
pub struct ThermalResult { pub temperature: Vec<f32> /* per node */, pub max_c: f64, pub min_c: f64, pub iterations: usize }
pub fn solve_thermal(case: &ThermalCase) -> Result<ThermalResult>;
```

- Conductivity in W/(mm K) is the W/(m K) value divided by 1000.
- Thermal stress: the temperature field becomes an initial strain `expansion * (T - reference)` in each cell of the static case.

- [ ] **Step 1: Write failing tests** (bar 10 x 10 x 100, conductivity 200):
  - `linear_temperature_in_a_bar`: 100 C at one end, 20 C at the other: the middle is 60 C within 0.5.
  - `power_gives_the_gradient`: 10 W in at one end, 20 C at the other: the hot end is 70 C within 1 (`P L / (k A)` = 50 K).
  - `convection_cools_a_block`: 5 W in, one face with 25 W/(m2 K) to 20 C air: the mean face temperature is `20 + 5 / (25e-6 * area)` within 3 %.
  - `no_heat_sink_is_refused`: `The study has heat input and no fixed temperature or convection.`
  - `clamped_bar_thermal_stress`: both ends fixed, `E = 68900`, `nu = 0`, expansion 23.6, uniform rise of 50 K: the axial stress is 81.3 MPa within 3 %.
  - `free_bar_grows_without_stress`: one end fixed: the tip moves `23.6e-6 * 50 * 100 = 0.118 mm` within 2 % and the largest von Mises is below 1 MPa.
- [ ] **Step 2: Run** `cargo test --offline -j 4 --release -p faber-sim --test thermal -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Shape optimisation

**Files:**
- Create: `crates/faber-sim/src/shape.rs`, `crates/faber-sim/src/surface.rs` (marching cubes)
- Test: `crates/faber-sim/tests/shape.rs`

**Interfaces:**

```rust
pub struct ShapeCase { pub base: StaticCase, pub keep: Vec<usize> /* cells that stay solid */, pub target_volume: f64 /* 0.1..=0.9 of the part */,
    pub min_member_mm: f64 /* filter radius, default 3 cells */, pub max_iterations: usize /* 80 */, pub symmetry: Vec<Plane> }
pub struct ShapeResult { pub density: Vec<f32>, pub volume_share: f64, pub compliance: Vec<f64> /* per iteration */,
    pub mesh: Mesh, pub pieces: usize, pub iterations: usize, pub mass_g: f64 }
pub fn optimise_shape(case: &ShapeCase) -> Result<ShapeResult>;
pub fn iso_surface(grid: &Grid, values: &[f32], level: f32) -> Mesh; // closed, outward normals
```

- Method: density of each cell from 0.001 to 1, stiffness `density^3`, compliance as the goal, a sensitivity filter with radius `min_member_mm`, the optimality criteria update with a move limit of 0.2. Cells of fixtures, loads and `keep` stay at 1. Stop when the largest density change is below 0.01.
- The result mesh is the iso-surface at 0.5 of the filtered density, after two steps of Laplace smoothing that keep the `keep` cells unchanged.

- [ ] **Step 1: Write failing tests.**
  - `beam_reaches_the_target`: block 120 x 20 x 40, fixed at the two lower end edges, load down at the top centre, target 0.5: `volume_share` is 0.5 within 0.02; the compliance of the last iteration is below the compliance of iteration 5; `pieces == 1`.
  - `result_is_symmetric`: the density is equal across the mid plane within 0.05 for 99 % of the cells.
  - `kept_cells_stay_solid`: each `keep` cell has density 1.
  - `result_mesh_is_closed`: no open edge; volume within 5 % of `volume_share` times the block volume.
  - `kept_regions_above_the_target_are_refused`: `The kept regions are 62 % of the part. The target is 50 %.`
  - `loose_pieces_are_reported`: a case with two separate load paths and target 0.1 reports `pieces > 1` and the limit sentence.
  - `iso_surface_of_a_sphere_field`: volume within 2 % of the sphere.
- [ ] **Step 2: Run** `cargo test --offline -j 4 --release -p faber-sim --test shape -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass. Record the time for 50 000 cells.

### Task 4: Study kinds and tools

**Files:**
- Modify: `crates/faber-model/src/lib.rs` (`Study.kind: StudyKind`), `crates/faber-wiring/src/study.rs`, `src/server.rs`
- Test: `crates/faber-wiring/tests/study.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
pub enum StudyKind<N> { Static, Modal { modes: u32 },
    Thermal { conditions: Vec<ThermalCondition<N>>, stress: bool, reference_c: N },
    Shape { target_volume: N, keep: Vec<FaceSelector<N>>, min_member_mm: Option<N>, symmetry: Vec<String /* plane feature */> } }
```

- `run_study` returns a report by kind:
  - modal: `{modes: [{frequency_hz, direction}], mass_g, limits}`; `render` draws one mode shape (`mode: n`, `deform_scale`).
  - thermal: `{max_c, min_c, hot_spots, limits}` and, with `stress: true`, the static report fields.
  - shape: `{volume_share, mass_g, mass_saved_g, pieces, compliance_change, new_part, limits}`. The result mesh is saved as a new mesh part `<part>-shape` (a mesh asset; not purchased), in one revision, only with `save: true`.
- The shape reply has the limits `The result is a mesh. Model the final part over it or convert it to a solid.` and `Check the final part with a static study.`

- [ ] **Step 1: Write failing tests:** `old_static_study_reads_as_static`, `modal_report_of_the_bracket_has_three_frequencies_in_rising_order`, `thermal_study_with_stress_reports_both`, `shape_study_saves_a_mesh_part_only_with_save`, `shape_part_can_be_checked_with_a_static_study`, `mcp.rs::each_kind_replies_below_2500_characters`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Viewer, guide, example

**Files:**
- Modify: `crates/faber-viewer/src/app/study.rs`, `docs/modelling-guide.md` (sections `## Frequency study`, `## Thermal study`, `## Shape optimisation`), `crates/faber-build/src/faber.libsonnet`
- Create: `examples/optimised-bracket/` (a block, one shape study, and the saved result)

- The study panel gets a kind selector. Modal: a mode list and an animation of the selected shape. Thermal: a temperature map with a legend in Celsius. Shape: a density preview that updates after each iteration, a "Stop" button, and "Save as part".

- [ ] **Step 1: Write failing viewer tests:** `kind_selector_changes_the_form`, `mode_list_shows_frequencies`, `shape_preview_updates_and_stop_keeps_the_last_result`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer study -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.
