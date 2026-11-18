# Plastic features: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the features that printed and moulded parts use most: rib, web,
boss, snap hook, lip and groove. One recipe operation makes each one.

**Architecture:** Each feature is a variant of `faber_model::Geometry` inside a
part history. `faber-core` expands boss, snap hook, lip and groove to existing
kernel calls. Rib and web need one new kernel call that grows a wall to the
next faces of the body. Lip and groove need one new kernel call that insets
the outline of a face.

**Tech Stack:** Rust 2024, OCCT 7.9.3 (`BRepFeat_MakeLinearForm`,
`BRepOffsetAPI_MakeOffset`).

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` (shared rules 3, 8, 13) and
the Create table of `docs/specs/2026-11-18-fusion-feature-gap.md` (Rib, Web).

## Global Constraints

- All shared rules of `00-roadmap.md` apply.
- Task 1 changes `faber-model` (all new variants in one commit).
- Tasks 2 and 5 change `bridge.cpp`: say so before the kernel build.
- Each feature works with `suppress_feature`, `edit_feature`, parameters and
  the feature cache, as the `pipe` feature does.
- Each new feature has a row in the viewer feature list and a form.

## Review Focus

- A rib line that does not reach a wall on one side is refused with the open
  side named (Task 2: `rib_that_misses_the_wall_is_refused`).
- A rib thicker than the space it must fill is refused, not built as garbage
  (Task 2: `rib_thicker_than_the_cavity_is_refused`).
- A boss with a hole diameter equal to or above its diameter is refused with
  both values (Task 3: `boss_hole_must_be_smaller`).
- A snap hook check with no material reports the strain and says that the
  limit is unknown (Task 4: `hook_strain_without_material`).
- A lip on a face with an inner loop (a rim) follows the selected loop only
  (Task 5: `lip_follows_the_inner_loop`).

---

### Task 1: Model variants

**Files:**
- Modify: `crates/faber-model/src/lib.rs`
- Test: unit tests in `crates/faber-model/src/lib.rs`

**Interfaces** (fields are `N` where a number is given; `body` is a feature id):

```rust
Rib   { body, plane: String /* plane feature */, lines: Vec<[[N; 2]; 2]>, thickness: N, side: RibSide /* symmetric | left | right */, draft_deg: Option<N> }
Web   { body, plane: String, lines: Vec<[[N; 2]; 2]>, thickness: N, draft_deg: Option<N> }
Boss  { body, plane: String, at: [N; 2], diameter: N, height: N, hole_diameter: Option<N>, hole_depth: Option<N>,
        draft_deg: Option<N>, ribs: Option<BossRibs<N>> /* count, thickness, length, height */, fillet: Option<N> }
SnapHook { body, plane: String, at: [N; 2], angle_deg: N, length: N, width: N, thickness: N, tip_thickness: Option<N>,
           undercut: N, hook_length: N, lead_deg: N /* default 30 */, return_deg: N /* default 90 */, root_fillet: Option<N> }
Lip    { body, face: FaceSelector<N>, loop_: LipLoop /* outer | inner */, side: LipSide /* inside | outside */, width: N, height: N }
Groove { body, face: FaceSelector<N>, loop_: LipLoop, side: LipSide, width: N, depth: N, clearance: N }
```

- A rib grows in the plane of the sketch, at a right angle to each line, to
  the next faces of the body. A web grows along the plane normal to the next
  faces of the body.

- [ ] **Step 1: Write failing tests:** each variant round-trips through JSON with the `op` names `rib`, `web`, `boss`, `snap_hook`, `lip`, `groove`; an unknown field is refused.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-model -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** Add each variant to the "not built by the native kernel" list of `crates/faber-geometry/src/lib.rs`.
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Rib and web

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`, `crates/faber-core/src/history.rs`
- Test: `crates/faber-wiring/tests/rib.rs`

**Interfaces:**

```rust
/// Grow a wall from an open profile to the next faces of the body.
/// `direction` is the growth direction. `thickness` is split by `offset` (-1..=1; 0 is symmetric).
fn linear_form(&self, _body: &Shape, _profile: &Shape, _direction: [f64; 3], _thickness: f64, _offset: f64) -> Result<Shape>;
```

- [ ] **Step 1: Write failing tests** (body: box 40 x 40 x 20 with a 2 mm shell, top open; inner size 36 x 36 x 18):
  - `web_across_the_box`: sketch plane at the top, one line from wall to wall at Y 20, thickness 2: the volume grows by 1296 within 1 % (36 x 2 x 18).
  - `two_crossing_webs`: two lines in a cross: growth `2 * 1296 - 2 * 2 * 18` within 1 %.
  - `rib_in_a_corner`: a vertical plane through the box, one inclined line from the floor to a wall, thickness 3: the volume grows and the result has one solid.
  - `rib_that_misses_the_wall_is_refused`: a line that ends 5 mm before the wall returns `The rib does not reach the body at the end of line 1.`
  - `rib_thicker_than_the_cavity_is_refused`.
  - `rib_follows_a_parameter`: change the box width; the web grows with it.
  - `suppressed_rib_gives_the_shell`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test rib -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** Draft: apply the existing `draft` call to the two side faces of the new wall when `draft_deg` is set.
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Boss

**Files:**
- Create: `crates/faber-core/src/plastic.rs`
- Modify: `crates/faber-core/src/history.rs`, `crates/faber-std/src/lib.rs` (`insert_hole(designation) -> Option<InsertHole>`)
- Test: `crates/faber-wiring/tests/boss.rs`

- The boss is a cylinder (a cone with `draft_deg`) on the plane, joined to the body, then the hole, then the ribs (boxes that the cone trims), then the fillet at the base.
- `hole: "M3-insert"` style input is not in this task. `faber-std` gets a table of heat-set insert holes (designation, hole diameter, depth, least boss diameter) with its source in a comment; `boss` takes `hole_diameter` from a parameter or a number.

- [ ] **Step 1: Write failing tests** (plate 40 x 40 x 2):
  - `plain_boss_volume`: diameter 8, height 10, hole 3 with depth 8: growth 446.11 within 0.5 % (`pi * 16 * 10 - pi * 2.25 * 8`).
  - `boss_with_four_ribs`: 4 ribs of thickness 1.5, length 6, height 6: one solid; growth above the plain boss; the bounds grow by the rib length.
  - `drafted_boss_is_narrower_at_the_top`: 2 degrees: the top face radius is `4 - 10 * tan(2 deg)` within 0.01.
  - `boss_hole_must_be_smaller`: `Boss hole 8 mm must be less than the boss diameter 8 mm.`
  - `insert_table_is_plausible`: each row has hole diameter below the least boss diameter.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test boss -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Snap hook

**Files:**
- Modify: `crates/faber-core/src/plastic.rs`, `crates/faber-core/src/history.rs`, `crates/faber-wiring/src/lib.rs`, `src/server.rs`
- Modify: `crates/faber-std/src/materials.rs` (`snap_strain_limit: Option<f64>`, with its source)
- Test: `crates/faber-wiring/tests/snap_hook.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
pub struct HookCheck { pub deflection_mm: f64, pub strain: f64, pub strain_limit: Option<f64>, pub ok: Option<bool>,
                       pub formula: &'static str, pub limits: Vec<String> }
pub fn hook_check(length: f64, thickness: f64, deflection: f64, limit: Option<f64>) -> HookCheck;
```

- The hook is a side profile (beam, hook head with the lead and return angles) that is extruded by `width` and joined to the body.
- Strain of a beam with constant rectangular section: `strain = 1.5 * thickness * deflection / length^2`. The deflection is the undercut.
- MCP: `feature_info` of a `snap_hook` feature adds the `HookCheck`. New tool `snap_check {length, thickness, undercut, material?}` for a quick answer.

- [ ] **Step 1: Write failing tests.**
  - `hook_volume`: length 15, width 5, thickness 2, undercut 1.5, hook length 3, lead 30, return 90: the growth equals `width * profile area` within 0.5 %, with the profile area computed in the test from the same numbers.
  - `hook_strain`: length 15, thickness 2, undercut 1.5 gives strain 0.02 within 1e-9.
  - `hook_strain_without_material`: `ok` is absent and `limits` has `No strain limit is known for this material.`
  - `hook_points_along_the_angle`: `angle_deg: 90` turns the bounds by a quarter turn.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Lip and groove

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`, `crates/faber-core/src/plastic.rs`, `crates/faber-core/src/history.rs`
- Test: `crates/faber-wiring/tests/lip.rs`

**Interfaces:**

```rust
/// A planar face made from one loop of a planar face, moved inward (negative) or outward (positive).
fn inset_face(&self, _face: &Shape, _loop_index: u32, _distance: f64) -> Result<Shape>; // loop 0 is the outer loop
```

- Lip: the ring between the loop and its inset by `width`, extruded by `height` along the face normal, joined to the body.
- Groove: the same ring, wider by `clearance` on each side and deeper by `clearance`, cut from the body.

- [ ] **Step 1: Write failing tests** (the open box of Task 2; its rim is one face with an outer and an inner loop):
  - `lip_on_the_outer_edge`: loop outer, side inside, width 1, height 2: growth 312 within 0.5 % (`(40^2 - 38^2) * 2`).
  - `lip_follows_the_inner_loop`: loop inner, side outside, width 1, height 2: growth `(38^2 - 36^2) * 2 = 296` within 0.5 %.
  - `groove_takes_the_lip`: a lid with a groove of clearance 0.15 on the box with the lip: `interference` reports no violation and the least distance between the two parts is 0.15 within 0.02.
  - `width_larger_than_the_rim_is_refused`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test lip -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 6: Viewer forms, helpers, example

**Files:**
- Modify: `crates/faber-viewer/src/app/feature_tools.rs`, `crates/faber-viewer/src/feature_form.rs`, `crates/faber-build/src/faber.libsonnet`, `docs/modelling-guide.md` (section `## Plastic features`)
- Create: `examples/snap-box/` (box with ribs, four bosses, a lip; lid with a groove and two snap hooks)

- [ ] **Step 1: Write failing viewer tests:** each of the six features appears in the feature list; the boss form builds a boss on the example.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer feature -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement** the forms, the Jsonnet helpers (`rib`, `web`, `boss`, `snap_hook`, `lip`, `groove`) and the example. The example passes `check` with no error and `print_check` if chunk 03 is present.
- [ ] **Step 4: Run** the viewer tests and `cargo test --offline -j 4 -p faber-build -- --test-threads=1`. Expected: pass.
