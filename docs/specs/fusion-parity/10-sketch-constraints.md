# Sketch constraints: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a sketch that is defined by geometric constraints and
dimensions, with a solver that reports the free degrees of freedom and the
conflicts. Add the sketch edits: offset, corner fillet, trim and extend.

**Architecture:** A new pure Rust crate `faber-sketch` holds the entity model,
the solver and the profile detection. A solved sketch is a new feature kind
that gives the same output as the present `sketch_on_plane` feature, so every
feature that takes a sketch works with it. The typed loops stay (decision D9).

**Tech Stack:** Rust 2024. No dependency for linear algebra.

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` (decision D9) and the Sketch
table of `docs/specs/2026-11-18-fusion-feature-gap.md`.

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Needs chunk 04 (`inset_face`) for
  the offset of Task 4.
- Task 3 changes `faber-model`.
- The solver is deterministic: the same input gives the same output bit for
  bit. No random start, no thread-dependent order.
- A dimension value is a `Scalar`, so it can be a parameter expression. The
  core resolves it before the solve.
- Sketch coordinates are millimetres in the plane of the sketch. Angles are
  degrees, counter-clockwise.

## Review Focus

- A sketch with no constraint solves to its start positions and reports its
  full degree count (Task 2: `unconstrained_sketch_keeps_its_points`).
- Two constraints that say the same thing are reported as redundant, not as a
  conflict (Task 2: `redundant_constraint_is_named`).
- A dimension of zero or a negative radius is refused before the solve
  (Task 2: `non_positive_radius_is_refused`).
- Entities that do not close a loop give a clear error when a feature needs a
  profile (Task 3: `open_chain_is_not_a_profile`).
- A solve that needs more than the iteration limit returns an error with the
  largest residual and its constraint (Task 2:
  `no_solution_names_the_worst_constraint`).

---

### Task 1: Entities and residuals

**Files:**
- Create: `crates/faber-sketch/Cargo.toml`, `src/lib.rs`, `src/model.rs`, `src/residual.rs`
- Test: unit tests in `residual.rs`

**Interfaces:**

```rust
pub struct Sketch<N = f64> { pub points: BTreeMap<String, [N; 2]>,           // start positions
    pub entities: BTreeMap<String, Entity<N>>, pub constraints: Vec<Constraint<N>>, pub construction: Vec<String> }
pub enum Entity<N> { Line { a: String, b: String }, Circle { centre: String, radius: N },
                     Arc { centre: String, start: String, end: String } /* counter-clockwise */ }
pub enum Constraint<N> {
    Coincident { a: String, b: String }, PointOnLine { point: String, line: String }, PointOnCircle { point: String, circle: String },
    Midpoint { point: String, line: String }, Horizontal { line: String }, Vertical { line: String },
    Parallel { a: String, b: String }, Perpendicular { a: String, b: String }, Collinear { a: String, b: String },
    Tangent { a: String, b: String }, Equal { a: String, b: String }, Concentric { a: String, b: String },
    Symmetric { a: String, b: String, about: String }, Fix { point: String },
    Distance { a: String, b: String, value: N }, DistanceX { a: String, b: String, value: N }, DistanceY { a: String, b: String, value: N },
    PointLineDistance { point: String, line: String, value: N }, Angle { a: String, b: String, degrees: N },
    Radius { entity: String, value: N }, Diameter { entity: String, value: N }, Length { line: String, value: N } }
pub(crate) fn residuals(sketch: &Sketch, x: &[f64], out: &mut Vec<f64>);     // one or two numbers for each constraint
pub(crate) fn jacobian(sketch: &Sketch, x: &[f64]) -> Dense;                 // analytic derivatives
```

- Variables: X and Y of each point, then the radius of each circle. The radius of an arc is not a variable: an arc adds the hidden constraint that `start` and `end` are at equal distance from `centre`.
- Each residual is in millimetres or is scaled to millimetres (an angle residual is multiplied by the mean length of the two lines).

- [ ] **Step 1: Write failing tests:** for each constraint kind, `residual_is_zero_when_satisfied` and `jacobian_matches_finite_differences` (step 1e-6, error below 1e-5) on a seeded set of 20 positions.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-sketch residual -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Solver and diagnosis

**Files:**
- Create: `crates/faber-sketch/src/solve.rs`, `crates/faber-sketch/src/dense.rs`
- Test: `crates/faber-sketch/tests/solve.rs`

**Interfaces:**

```rust
pub struct Solution { pub points: BTreeMap<String, [f64; 2]>, pub radii: BTreeMap<String, f64>,
    pub status: Status, pub dof: usize, pub free_points: Vec<String>, pub redundant: Vec<usize>, pub conflicting: Vec<usize>,
    pub iterations: usize, pub residual: f64 }
pub enum Status { Solved, Under, Over }
pub fn solve(sketch: &Sketch) -> Result<Solution>;
pub fn drag(sketch: &Sketch, point: &str, target: [f64; 2]) -> Result<Solution>; // soft pull on one point
```

- Method: Levenberg-Marquardt. Normal equations with a Cholesky factor in `dense.rs`. At most 100 iterations. Converged when the largest residual is below 1e-9 mm.
- Degrees of freedom: variables minus the rank of the Jacobian at the solution (QR with column pivoting, threshold 1e-9 times the largest diagonal).
- `Over`: no convergence. `conflicting` is found by solving again with one constraint left out at a time, last added first; the first sets that converge are reported.
- `redundant`: a constraint whose removal does not change the rank.
- `free_points`: points with a non-zero part in the null space.

- [ ] **Step 1: Write failing tests.**
  - `rectangle_is_fully_constrained`: 4 lines, coincident corners, 2 horizontal, 2 vertical, width 40, height 20, one fixed point: `Solved`, `dof == 0`, the corners are (0, 0), (40, 0), (40, 20), (0, 20) within 1e-9.
  - `rectangle_without_height_has_one_freedom`: `Under`, `dof == 1`, `free_points` names the two top points.
  - `contradiction_is_named`: width 40 and width 50 on the same line: `Over`, `conflicting` has both indexes.
  - `redundant_constraint_is_named`: horizontal on a line plus `DistanceY 0` between its points.
  - `slot_with_tangent_arcs`: two lines and two arcs, tangent, length 30, radius 5: the area of the profile is `30 * 10 + pi * 25 = 378.54` within 1e-6.
  - `start_far_away_still_solves`: the rectangle with each start position moved by up to 30 % (seeded).
  - `unconstrained_sketch_keeps_its_points`: `dof` equals twice the point count.
  - `non_positive_radius_is_refused`: `Radius of circle c1 is 0 mm. It must be above 0.`
  - `no_solution_names_the_worst_constraint`.
  - `same_input_same_bits`: two solves give equal `f64` bits.
  - `two_hundred_constraints`: a chain of 50 rectangles solves in at most 50 iterations.
  - `drag_moves_a_free_point_and_keeps_the_constraints`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-sketch --test solve -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Solved sketch feature

**Files:**
- Create: `crates/faber-sketch/src/profile.rs`
- Modify: `crates/faber-model/src/lib.rs` (`Geometry::SolvedSketch { plane: String, sketch: SolvedSketchData<N> }`), `crates/faber-core/src/history.rs`, `crates/faber-geometry/src/lib.rs`
- Test: `crates/faber-sketch/tests/profile.rs`, `crates/faber-wiring/tests/solved_sketch.rs`

**Interfaces:**

```rust
pub fn loops(sketch: &Sketch, solution: &Solution) -> Result<Vec<Loop>>;   // closed chains of non-construction entities
pub struct Loop { pub segments: Vec<Segment>, pub area: f64 }               // Line | Arc | Circle, counter-clockwise
pub fn regions(loops: &[Loop]) -> Vec<Region>;                              // outer loop with its holes
```

- `faber-core` solves the sketch at build time and converts each region to the present sketch loop types, so `extrude_sketch`, `revolve_sketch`, `loft_sketches` and `sweep_sketch` take a solved sketch with no change.
- A sketch that is `Under` builds with its solved positions and adds a check warning with the degree count. A sketch that is `Over` is refused.
- The solution is cached with the feature cache key of the feature.

- [ ] **Step 1: Write failing tests.**
  - `profile.rs::rectangle_with_a_circle_is_one_region_with_one_hole`.
  - `profile.rs::open_chain_is_not_a_profile`: `The entities l1, l2, l3 do not close a loop. The open ends are at p1 and p4.`
  - `profile.rs::construction_lines_are_ignored`.
  - `solved_sketch.rs::extrude_of_a_solved_rectangle`: 40 x 20 extruded 10 has volume 8000 within 1e-6.
  - `solved_sketch.rs::dimension_from_a_parameter`: change the parameter `w` from 40 to 50; the volume is 10000.
  - `solved_sketch.rs::over_constrained_sketch_is_refused_and_files_stay`.
  - `solved_sketch.rs::under_constrained_sketch_warns`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Sketch edits

**Files:**
- Create: `crates/faber-sketch/src/edit.rs`
- Modify: `crates/faber-model/src/lib.rs` (`Geometry::OffsetSketch { sketch: String, distance: N }`), `crates/faber-core/src/history.rs`, `crates/faber-wiring/src/lib.rs`, `src/server.rs`
- Test: `crates/faber-sketch/tests/edit.rs`, `crates/faber-wiring/tests/solved_sketch.rs`

**Interfaces:**

```rust
pub enum SketchOp { AddPoint { .. }, AddEntity { .. }, AddConstraint { .. }, Remove { id: String },
    Fillet { corner: String, radius: f64 },        // replaces the corner point by an arc with two tangent constraints
    Chamfer { corner: String, distance: f64 },
    Trim { entity: String, near: [f64; 2] },       // removes the piece between the two nearest intersections
    Extend { line: String, end: String },          // to the nearest entity along the line
    Mirror { entities: Vec<String>, about: String }, // adds copies with symmetric constraints
    Pattern { entities: Vec<String>, count: u32, step: [f64; 2] } }
pub fn apply(sketch: &Sketch, ops: &[SketchOp]) -> Result<Sketch>;
```

- `OffsetSketch` is a feature: it takes the result of a sketch feature and moves each loop by `distance` with `Kernel::inset_face`. Its result is a sketch result.
- MCP: `solve_sketch {sketch}` (dry run: positions, status, `dof`, free points, conflicts) and `edit_sketch {part, feature, ops, expected_revision?}` (applies the ops to a `solved_sketch` feature, solves, builds, saves one revision).

- [ ] **Step 1: Write failing tests.**
  - `fillet_of_a_rectangle_corner`: radius 5 on one corner of the 40 x 20 rectangle: `dof` stays 0 after a `Radius` constraint; area `800 - (25 - pi * 25 / 4)` within 1e-6.
  - `trim_between_two_crossings`: a line across a circle, trimmed inside: two lines remain, each ends on the circle with a `PointOnCircle` constraint.
  - `extend_to_the_next_line`.
  - `mirror_adds_symmetric_constraints`: move the source; the copy follows.
  - `offset_sketch_of_a_rectangle`: distance -2 gives 36 x 16; the extrude of the ring between both has volume `(800 - 576) * 10`.
  - `mcp::solve_sketch_reports_free_points`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Viewer sketch mode

**Files:**
- Modify: `crates/faber-viewer/src/sketch.rs`, `crates/faber-viewer/src/app.rs`
- Modify: `docs/modelling-guide.md` (section `## Constrained sketches`), `crates/faber-build/src/faber.libsonnet`
- Create: `examples/constrained-plate/`
- Test: in `crates/faber-viewer/src/sketch.rs` with `egui_kittest`

- "Edit sketch" on a `solved_sketch` feature shows the sketch in its plane: entities, points, a glyph for each constraint, each dimension with its value.
- Free geometry is blue, fully constrained geometry is black, a conflict is red. The status line shows the degree count.
- A point drag calls `drag` each frame. A double click on a dimension edits its value or expression. A toolbar adds a line, a circle, an arc and each constraint for the selected entities. "Finish" saves one revision.

- [ ] **Step 1: Write failing tests:** `sketch_mode_shows_the_degree_count`, `dragging_a_free_point_keeps_the_width`, `editing_a_dimension_rebuilds_the_part`, `conflict_is_shown_and_finish_is_disabled`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer sketch -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.
