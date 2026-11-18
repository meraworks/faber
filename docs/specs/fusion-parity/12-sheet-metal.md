# Sheet metal: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Model a bent sheet part as a tree of flat walls and bends, build its
folded solid, and give its exact flat pattern with bend lines as DXF and SVG.

**Architecture:** A sheet metal part is one recipe: a rule (thickness, bend
radius, K factor), a base wall and a list of flanges. Each flange names the
wall and the edge it grows from. A new pure Rust crate `faber-sheet` computes
the wall frames, the folded outline of each wall and the flat pattern from the
same tree. `faber-core` builds the folded solid from prisms and bend segments
with existing kernel calls.

**Tech Stack:** Rust 2024, `i_overlay` (2D union and difference), existing
kernel calls (`sketch`, `extrude_shape`, `revolve_shape`, `combine`).

**Spec:** `docs/specs/fusion-parity/00-roadmap.md`. Fusion scope covered:
sheet rule, flange, bend, unfold and flat pattern. Not covered: hem, jog,
lofted flange, conversion of a solid to sheet metal, holes across a bend.

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Needs chunk 01 (vector documents).
- Task 1 changes `faber-model`.
- Bend allowance: `BA = angle_rad * (radius + k_factor * thickness)`, where
  `radius` is the inside radius. Default `k_factor` is 0.44. The rule holds
  the value; a reply names it.
- A flange `length` is the flat length of the wall after the bend, measured
  from the end of the bend.
- Angles are bend angles in degrees: 0 is flat, 90 is a right angle. A
  positive angle bends toward the top side of the parent wall.

## Review Focus

- Two flanges that overlap in the folded state are refused with their ids
  (Task 2: `colliding_flanges_are_refused`).
- Two flanges that overlap in the flat pattern are reported: the part cannot
  be cut from one sheet (Task 3: `flat_overlap_is_reported`).
- A bend radius below the thickness gives a warning with both values (Task 2:
  `tight_radius_warns`).
- A flange on an edge that is not straight is refused (Task 2:
  `flange_on_an_arc_edge_is_refused`).
- A hole closer to a bend than `radius + 2 * thickness` gives a warning with
  the distance (Task 2: `hole_near_a_bend_warns`).

---

### Task 1: Sheet recipe

**Files:**
- Modify: `crates/faber-model/src/lib.rs`, `crates/faber-geometry/src/lib.rs`
- Test: unit tests in `crates/faber-model/src/lib.rs`

**Interfaces:**

```rust
Geometry::SheetMetal { rule: SheetRule<N>, base: SketchProfile<N> /* outline and holes on XY */, flanges: Vec<Flange<N>> }
pub struct SheetRule<N> { pub thickness: N, pub bend_radius: N, pub k_factor: Option<N>, pub relief: Relief<N> /* None | Rectangular { width, depth } | Round { diameter } */ }
pub struct Flange<N> { pub id: String, pub on: String /* "base" or a flange id */, pub edge: u32 /* straight outline edge of that wall, in outline order */,
    pub length: N, pub angle_deg: Option<N> /* default 90 */, pub inset_start: Option<N>, pub inset_end: Option<N>,
    pub holes: Vec<SketchLoop<N>> /* in wall coordinates: X along the bend line, Y away from it */ }
```

- [ ] **Step 1: Write failing tests:** JSON round trip with `op: "sheet_metal"`; a flange with an unknown field is refused.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-model -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Wall tree and folded solid

**Files:**
- Create: `crates/faber-sheet/Cargo.toml`, `src/lib.rs`, `src/tree.rs`, `src/fold.rs`
- Create: `crates/faber-core/src/sheet.rs`
- Modify: `crates/faber-core/src/history.rs` (build `Geometry::SheetMetal`)
- Test: `crates/faber-sheet/tests/fold.rs`, `crates/faber-wiring/tests/sheet_metal.rs`

**Interfaces:**

```rust
pub struct Wall { pub id: String, pub outline: Vec<[f64; 2]>, pub holes: Vec<Vec<[f64; 2]>>, pub frame: glam::DAffine3 /* wall XY to part space, folded */ }
pub struct Bend { pub flange: String, pub line: [[f64; 3]; 2] /* axis of the inside radius, folded */, pub angle_deg: f64,
                  pub radius: f64, pub allowance: f64, pub width: f64 }
pub struct Folded { pub walls: Vec<Wall>, pub bends: Vec<Bend>, pub warnings: Vec<String> }
pub fn fold(rule: &SheetRule, base: &Outline, flanges: &[Flange]) -> Result<Folded>;
```

- The flange wall is a rectangle: the edge length minus the insets, by `length`, with its holes.
- With a relief, the parent wall gets a relief cut at each end of a flange that does not reach the end of its edge.
- `faber-core`: each wall is the extrude of its outline by the thickness, placed by its frame; each bend is the revolve of the thickness rectangle about the bend axis by the angle; all are joined.

- [ ] **Step 1: Write failing tests** (thickness 2, radius 2, K 0.44):
  - `fold.rs::l_bracket_frames`: base 50 x 40, one flange on the 40 mm edge, length 30, 90 degrees: the flange wall is vertical; the folded bounds are 54 x 40 x 34 within 1e-9 (`50 + radius + thickness`, `30 + radius + thickness`).
  - `sheet_metal.rs::l_bracket_volume`: 6776.99 within 0.1 % (`2 * 40 * 80 + (pi / 4) * (16 - 4) * 40`).
  - `sheet_metal.rs::u_channel_is_symmetric`: two flanges on opposite edges: the bounds are symmetric about the base centre.
  - `sheet_metal.rs::flange_on_a_flange`: a Z shape: one solid; the top wall is parallel to the base.
  - `sheet_metal.rs::forty_five_degree_flange`.
  - `fold.rs::inset_flange_gets_reliefs`: insets of 5 with a rectangular relief: the base outline has 2 relief cuts of the rule size.
  - `fold.rs::colliding_flanges_are_refused`: two 90 degree flanges on adjacent edges with no inset and no relief: `Flanges f1 and f2 overlap at the corner. Add an inset or a relief.`
  - `fold.rs::tight_radius_warns`, `fold.rs::flange_on_an_arc_edge_is_refused`, `fold.rs::hole_near_a_bend_warns`.
  - `sheet_metal.rs::thickness_parameter_changes_the_part`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-sheet -- --test-threads=1` and `cargo test --offline -j 4 -p faber-wiring --test sheet_metal -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Flat pattern

**Files:**
- Create: `crates/faber-sheet/src/flat.rs`
- Modify: `crates/faber-wiring/src/lib.rs`, `src/server.rs`, `src/main.rs` (`faber flat PROJECT PART FILE`)
- Test: `crates/faber-sheet/tests/flat.rs`, `crates/faber-wiring/tests/sheet_metal.rs`

**Interfaces:**

```rust
pub struct BendLine { pub flange: String, pub line: [[f64; 2]; 2], pub angle_deg: f64, pub direction: Direction /* Up | Down */, pub radius: f64 }
pub struct FlatPattern { pub outline: Vec<Vec<[f64; 2]>> /* one loop if valid */, pub holes: Vec<Vec<[f64; 2]>>, pub bends: Vec<BendLine>,
    pub size_mm: [f64; 2], pub area_mm2: f64, pub cut_length_mm: f64, pub problems: Vec<String> }
pub fn flatten(folded_input: (&SheetRule, &Outline, &[Flange])) -> Result<FlatPattern>;
pub fn to_vector(flat: &FlatPattern, title: &str) -> VectorDoc; // layers CUT, BEND (dashed), TEXT
```

- Each flange outline is placed in the plane of its parent, moved away from the parent edge by the bend allowance. The bend zone is a rectangle of the allowance width between them. The outline is the union of all walls and bend zones (`i_overlay`).
- The bend line is the centre line of the bend zone.
- MCP: `flat_pattern {part, path?}` returns the sizes, the bend table (`flange`, `angle_deg`, `direction`, `radius`, `allowance`) and the problems; with `path` it writes `.dxf` or `.svg`.

- [ ] **Step 1: Write failing tests.**
  - `l_bracket_flat_size`: 84.5239 x 40 within 1e-4 (`50 + (pi / 2) * (2 + 0.88) + 30`), one bend line at X `50 + 4.5239 / 2`.
  - `flat_area_is_the_wall_area_plus_the_bend_zones`.
  - `u_channel_has_two_bend_lines`.
  - `holes_move_with_their_wall`: a hole in the flange is at its wall position plus the offset of the wall in the pattern.
  - `flat_overlap_is_reported`: a box with four flanges and four corner flanges that overlap: `problems` has `Walls f5 and f6 overlap in the flat pattern.`
  - `dxf_has_cut_and_bend_layers`: the L bracket file has one closed polyline in `CUT` and one line in `BEND`.
  - `bend_direction_follows_the_sign`: a negative angle gives `Down`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-sheet --test flat -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Viewer, guide, example

**Files:**
- Create: `crates/faber-viewer/src/app/sheet.rs`
- Modify: `crates/faber-viewer/src/app.rs`, `crates/faber-viewer/src/app/feature_tools.rs`, `crates/faber-build/src/faber.libsonnet` (`sheet`, `flange`), `docs/modelling-guide.md` (section `## Sheet metal`)
- Create: `examples/sheet-bracket/`

- A sheet metal part has a panel with the rule, a flange list (edge, length, angle, insets) and "Add flange" by a click on a straight edge. A "Flat pattern" switch shows the pattern with its bend lines in place of the folded part. "Export flat pattern" writes DXF or SVG.

- [ ] **Step 1: Write failing viewer tests:** `add_flange_by_edge_click`, `flat_pattern_switch_shows_the_bend_lines`, `rule_change_rebuilds_the_part`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer sheet -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.
