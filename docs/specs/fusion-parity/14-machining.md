# Machining: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make toolpaths for a 3-axis mill and for 2D cutting machines from a
part, check them against the part and the stock, and write G-code.

**Architecture:** A new pure Rust crate `faber-cam` works on meshes. 2D
operations slice the part at a depth and offset the loops by the tool radius.
3D operations use a height map of the part and a drop-cutter. A second height
map of the stock is cut by each move to verify the result. Setups, tools and
operations are project data in `manufacture.json`.

**Tech Stack:** Rust 2024, `i_overlay` (polygon offset and Boolean), `rayon`,
`faber-geometry::bvh`, `faber-meshtools::cut::slice`.

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` (decision D3).

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Needs chunks 01 and 05.
- Task 1 changes `faber-model`.
- Not in this chunk: turning, 4 and 5 axes, adaptive clearing, tool holder
  collision, rest machining, tabs. The guide says so.
- A toolpath is a list of moves in the work coordinate system of its setup:
  millimetres, feed in mm/min, Z up, Z 0 at the setup origin.
- A program is written only when its verification has no gouge and no rapid
  move through material. `force: true` writes it with the problems as comments.
- G-code safety rule: the first motion of each operation is a rapid to the
  clearance height. The spindle starts before the first plunge.

## Review Focus

- A tool larger than the pocket is refused with both sizes (Task 3:
  `tool_larger_than_the_pocket_is_refused`).
- A depth below the bottom of the stock is refused (Task 2:
  `depth_below_the_stock_is_refused`).
- A rapid move never passes through material: the verification finds one if
  it is added by hand (Task 5: `rapid_through_material_is_found`).
- A flute length shorter than the cut depth gives a warning with both values
  (Task 5: `short_flute_warns`).
- A part with an undercut gives a 3D path for the reachable surface and
  reports the hidden area (Task 4: `undercut_area_is_reported`).

---

### Task 1: Setup, tool and operation data

**Files:**
- Modify: `crates/faber-model/src/lib.rs` (or new `crates/faber-model/src/machining.rs`), `crates/faber-files/src/layout.rs` (`manufacture.json` gains `machining`), `crates/faber-core/src/batch.rs`, `src/server.rs`
- Test: `crates/faber-files/tests/layout.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
pub struct Machining { pub tools: BTreeMap<String, Tool>, pub setups: BTreeMap<String, Setup>, pub operations: Vec<Operation> }
pub struct Tool { pub kind: ToolKind /* flat ball bull drill chamfer laser */, pub diameter: f64, pub corner_radius: f64, pub flute_length: f64,
                  pub flutes: u32, pub feed_mm_min: f64, pub plunge_mm_min: f64, pub spindle_rpm: f64, pub number: u32 }
pub struct Setup { pub part: String, pub stock: Stock /* Offset { sides, top, bottom } | Box { size } */,
                   pub origin: Origin /* stock_top_centre | stock_top_corner | part_bottom_corner */, pub up: [f64; 3] /* part direction that is machine +Z */ }
pub struct Operation { pub id: String, pub setup: String, pub tool: String, pub heights: Heights /* clearance, retract, top, bottom (optional) */,
                       pub stepdown: Option<f64>, pub kind: OperationKind }
pub enum OperationKind { Face { stepover: f64 }, Contour { z: Selection, side: Side /* outside | inside */, stock_to_leave: f64 },
    Pocket { floor: Selection, stepover: f64, stock_to_leave: f64 }, Drill { holes: Holes /* auto | list */, peck: Option<f64> },
    Rough3d { stepover: f64, stock_to_leave: f64 }, Parallel3d { stepover: f64, angle_deg: f64 },
    Cut2d { kerf: f64, lead_in: f64, power: f64 } }
pub enum Selection { Z(f64), Face(FaceSelector) }
```

- MCP: `set_tool`, `set_setup`, `set_operation`, `remove_operation`, `machining` (read: the list with ids). `batch` accepts them.

- [ ] **Step 1: Write failing tests:** `machining_round_trips_in_manufacture_json`, `operation_with_an_unknown_tool_is_refused`, `operation_with_an_unknown_setup_is_refused`, `mcp.rs::set_operation_then_machining_lists_it`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-files --test layout -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: 2D toolpaths: face, contour, drill, cut

**Files:**
- Create: `crates/faber-cam/Cargo.toml`, `src/lib.rs`, `src/path.rs`, `src/region.rs`, `src/two_d.rs`, `src/drill.rs`
- Test: `crates/faber-cam/tests/two_d.rs`

**Interfaces:**

```rust
pub enum Move { Rapid { to: [f64; 3] }, Feed { to: [f64; 3], feed: f64 }, Dwell { seconds: f64 } }
pub struct Toolpath { pub operation: String, pub tool: String, pub moves: Vec<Move> }
impl Toolpath { pub fn cut_length(&self) -> f64; pub fn rapid_length(&self) -> f64; pub fn minutes(&self, rapid_mm_min: f64) -> f64; pub fn bounds(&self) -> [[f64; 3]; 2]; }
pub struct Region { pub outer: Vec<[f64; 2]>, pub holes: Vec<Vec<[f64; 2]>> }
pub fn regions_at(mesh: &Mesh, z: f64) -> Vec<Region>;                 // section of the part at z
pub fn offset(regions: &[Region], distance: f64) -> Vec<Region>;      // round joins, chord error 0.01 mm
pub fn face(stock: &StockBox, tool: &Tool, op: &Operation) -> Result<Toolpath>;
pub fn contour(part: &Mesh, tool: &Tool, op: &Operation) -> Result<Toolpath>;
pub fn drill(holes: &[Hole], tool: &Tool, op: &Operation) -> Result<Toolpath>;
pub fn cut2d(part: &Mesh, tool: &Tool, op: &Operation) -> Result<Toolpath>;
pub fn find_holes(faces: &[FaceInfo], up: [f64; 3]) -> Vec<Hole>;     // cylinders along `up`: centre, diameter, top, bottom
```

- Depth passes: from `top` down by `stepdown` to `bottom`; the last pass is at `bottom` exactly.
- Each pass starts with a plunge at the plunge feed. Between passes the tool retracts to the retract height.
- `Cut2d`: one pass at Z 0, offset by half the kerf, with a lead-in of the given length from the waste side. Inner loops are cut before outer loops.

- [ ] **Step 1: Write failing tests.**
  - `contour_of_a_rectangle`: part 40 x 20 x 10, flat tool of diameter 6, outside: one pass length is `2 * (40 + 20) + 2 * pi * 3 = 138.85` within 0.5 %.
  - `contour_depth_passes`: bottom -10, stepdown 3 gives passes at -3, -6, -9, -10.
  - `inside_contour_of_a_hole`: a hole of diameter 20 with the same tool: a circle of radius 7.
  - `face_covers_the_stock`: 50 x 50 stock, tool 10, stepover 6: each point of the top is within 5 mm of a pass line.
  - `drill_finds_four_holes`: 4 through holes of diameter 5: 4 cycles; with `peck: 2` each hole has retract moves every 2 mm.
  - `cut2d_cuts_inner_loops_first` and `cut2d_offsets_by_half_the_kerf`.
  - `depth_below_the_stock_is_refused`: `Bottom -30 mm is below the stock bottom -25 mm.`
  - `every_path_starts_at_the_clearance_height`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-cam --test two_d -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Pocket

**Files:**
- Create: `crates/faber-cam/src/pocket.rs`
- Test: `crates/faber-cam/tests/pocket.rs`

**Interfaces:**

```rust
pub fn pocket(part: &Mesh, stock: &StockBox, tool: &Tool, op: &Operation) -> Result<Toolpath>;
pub(crate) fn clear(region: &[Region], tool_radius: f64, stepover: f64) -> Vec<Vec<[f64; 2]>>; // contour-parallel rings, inside out
```

- The region to clear at a depth is the stock section minus the part section at that depth, limited to the area above the selected floor.
- Entry: a helix ramp of 2 degrees inside the first ring when the ring is wide enough; else a plunge at the plunge feed.

- [ ] **Step 1: Write failing tests.**
  - `rectangular_pocket_is_cleared`: pocket 30 x 20, depth 5, tool 6, stepover 3: after the verification of Task 5 the floor has no excess above 0.01 mm outside the four corners; the uncut corner area is `4 * (9 - pi * 9 / 4) = 7.73 mm2` within 10 %.
  - `pocket_with_an_island_keeps_the_island`: no move enters the island offset by the tool radius.
  - `stock_to_leave_moves_the_wall_pass`: 0.3 leaves 0.3 within 0.02 on the walls.
  - `tool_larger_than_the_pocket_is_refused`: `Tool diameter 12 mm does not fit the pocket width 10 mm.`
  - `rings_are_ordered_inside_out`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-cam --test pocket -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: 3D toolpaths

**Files:**
- Create: `crates/faber-cam/src/height_map.rs`, `src/three_d.rs`
- Test: `crates/faber-cam/tests/three_d.rs`

**Interfaces:**

```rust
pub struct HeightMap { pub origin: [f64; 2], pub cell: f64, pub n: [usize; 2], pub z: Vec<f32> }
pub fn height_map(mesh: &Mesh, bvh: &Bvh, cell: f64, floor: f64) -> HeightMap;        // highest point of the part in each cell
pub fn tool_surface(map: &HeightMap, tool: &Tool, stock_to_leave: f64) -> HeightMap;  // lowest tool tip height with no gouge
pub fn rough3d(part: &HeightMap, stock: &StockBox, tool: &Tool, op: &Operation) -> Result<Toolpath>;
pub fn parallel3d(part: &HeightMap, tool: &Tool, op: &Operation) -> Result<Toolpath>;
pub fn hidden_area(mesh: &Mesh, bvh: &Bvh, up: [f64; 3]) -> f64;
```

- Cell size: the smaller of 0.25 mm and a tenth of the tool diameter; at most 16 million cells, else refused with the count.
- `rough3d`: at each level, the region where the tool surface is below the level is cleared with `clear`.
- `parallel3d`: lines at `stepover` and `angle_deg`, the tip follows the tool surface; alternate lines run in opposite directions.

- [ ] **Step 1: Write failing tests** (a half sphere of radius 20 on a 50 x 50 x 5 base, stock 50 x 50 x 25):
  - `tool_surface_never_gouges`: for a flat and a ball tool, the tool body at each cell is at or above the part within 0.01 mm.
  - `roughing_leaves_at_most_a_step`: after the verification, the excess is at most `stepdown + 0.1` and the gouge is 0.
  - `parallel_scallop_height`: ball tool of diameter 6, stepover 0.5: the excess after finishing is at most `0.0104 + 0.05 mm`.
  - `parallel_lines_alternate`.
  - `undercut_area_is_reported`: a mushroom shape gives `hidden_area > 0` and the limit `<n> mm2 of the surface is not reachable from this direction.`
  - `too_many_cells_are_refused`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 --release -p faber-cam --test three_d -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Verification

**Files:**
- Create: `crates/faber-cam/src/verify.rs`
- Test: `crates/faber-cam/tests/verify.rs`

**Interfaces:**

```rust
pub struct Verification { pub gouge_mm: f64, pub gouge_at: Option<[f64; 3]>, pub max_excess_mm: f64, pub removed_mm3: f64,
    pub rapid_collisions: Vec<[f64; 3]>, pub warnings: Vec<String>, pub minutes: f64, pub stock: HeightMap }
pub fn verify(part: &HeightMap, stock: &StockBox, paths: &[(Toolpath, Tool)], tolerance: f64 /* 0.02 */) -> Verification;
pub fn stock_mesh(v: &Verification) -> Mesh; // the cut stock, for the viewer and the render
```

- [ ] **Step 1: Write failing tests:** `clean_face_pass_removes_the_top_layer` (removed volume equals area times depth within 1 %), `gouge_is_found_and_located` (a hand-made move 1 mm below the part), `rapid_through_material_is_found`, `short_flute_warns` (`Cut depth 12 mm is more than the flute length 8 mm of tool t1.`), `time_is_length_over_feed`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-cam --test verify -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 6: G-code and tools

**Files:**
- Create: `crates/faber-files/src/gcode.rs`, `crates/faber-wiring/src/machining.rs`
- Modify: `crates/faber-ports/src/lib.rs` (`trait ProgramExport`), `crates/faber-wiring/src/lib.rs`, `src/server.rs`, `src/main.rs` (`faber gcode PROJECT SETUP FILE [--dialect NAME]`)
- Test: `crates/faber-files/tests/gcode.rs`, `crates/faber-wiring/tests/machining.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
pub enum Dialect { Grbl, LinuxCnc, Generic, GrblLaser }
pub trait ProgramExport: Send + Sync { fn write(&self, program: &Program, dialect: Dialect, path: &Path) -> Result<()>; }
pub struct Program { pub name: String, pub units_mm: bool, pub sections: Vec<Section> } // Section: tool number, spindle, coolant, moves
```

- Header: `G21`, `G90`, `G17`. Tool change: `T<n> M6` (left out for `Grbl`). Spindle: `M3 S<rpm>`. End: `M5`, `M30` (`M2` for `Grbl`). Rapid `G0`, feed `G1` with `F` only when it changes. Numbers have 3 decimals with no trailing zeros. `GrblLaser`: `M4 S<power>` and no Z word.
- MCP: `toolpaths {setup, operations?}` generates and verifies; the reply for each operation has `cut_length_mm`, `minutes`, `gouge_mm`, `max_excess_mm`, `warnings`; `render?` draws the paths over the cut stock. `write_gcode {setup, path, dialect?, force?}`.

- [ ] **Step 1: Write failing tests.**
  - `gcode.rs::grbl_program_text`: a two-move program equals a fixed text of 9 lines, byte for byte.
  - `gcode.rs::feed_word_is_written_once`.
  - `gcode.rs::laser_has_no_z`.
  - `machining.rs::plate_with_pocket_and_holes`: a setup with face, pocket, contour and drill gives four toolpaths, no gouge, and a program whose lowest Z equals the stock bottom allowance.
  - `machining.rs::gouge_blocks_the_program`: `write_gcode` is refused; with `force` the file starts with a comment line for each problem.
  - `mcp.rs::toolpaths_reply_is_small`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 7: Viewer, guide, example

**Files:**
- Create: `crates/faber-viewer/src/app/machining.rs`
- Modify: `crates/faber-viewer/src/app.rs`, `docs/modelling-guide.md` (section `## Machining`)
- Create: `examples/milled-plate/` (a plate with a pocket, four holes, one setup, four operations)

- "Manufacture" panel: setups, the tool table, the operation list with the time of each. The 3D view shows the stock as a transparent box, the paths (rapid moves yellow, feed moves blue), and "Simulate" that shows the cut stock with gouges in red.

- [ ] **Step 1: Write failing viewer tests:** `operation_list_shows_times`, `simulate_shows_the_cut_stock`, `gouge_is_marked_red`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer machining -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.
