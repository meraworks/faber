# Direct manipulation: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pick faces and edges in the 3D view, feed them to the feature tools,
and change a solid by a drag (press-pull), as in Fusion.

**Architecture:** Face picking uses the face index of each triangle from
chunk 01 (`Body.face_of_triangle`). Edge picking needs a new kernel port method
that returns each edge as a polyline with its edge index. A pick becomes a
`FaceSelector` or `EdgeSelector` that the existing recipe operations already
accept, so no `faber-model` change is needed. Press-pull adds a `thicken` of
the picked face and a `union` or `difference` to the part history.

**Tech Stack:** Rust 2024, egui 0.37, `egui_kittest`, `faber-scene`,
`faber-core` (preview without save), OCCT through `faber-kernel-occt`.

**Spec:** `docs/specs/fusion-parity/viewer-usability-audit.md` (section B,
items "Missing"), `docs/specs/2026-11-18-fusion-feature-gap.md` (marking menu
and Press Pull). Shared rules: `00-roadmap.md`.

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Needs chunk 01 (Task 3: face
  meshes) and chunk 17.
- Task 2 changes `bridge.cpp`. Say so before the build starts (long build).
- A pick never writes a face or edge index into the project files. It writes a
  selector (`near_centre`, `normal`, `cylinder_radius`, `in_box`, `expect`)
  that still finds the same face or edge after an upstream change. Indexes are
  for the current build only.
- A purchased part can be picked for a measure or a sketch plane, never as the
  target of a change. A change is refused with "Part <id> is a purchased part.
  It cannot change."
- A drag preview never saves. Only release or `Enter` saves, as one revision.
- Selection filter (decision M1): Body, Face, Edge. The default is Body. A tool
  that needs faces or edges switches the filter while it is open and restores
  it on close.

## Review Focus

- A selector made from a pick finds exactly the picked face after a parameter
  change that moves that face (Task 3: `fillet_from_picked_edges_survives_a_resize`).
- A pick through a section cut picks the cut-away side never (Task 1:
  `pick_ignores_the_cut_away_half`).
- A press-pull of a curved face is refused with a clear text (Task 4:
  `press_pull_refuses_a_curved_face`).
- A press-pull of minus the full thickness is refused, not an empty solid
  (Task 4: `press_pull_through_the_part_is_refused`).
- Escape during a drag restores the solid and saves nothing (Task 4:
  `escape_during_a_drag_saves_nothing`).

---

### Task 1: Face pick and face hover

**Files:**
- Create: `crates/faber-viewer/src/app/face_pick.rs`, `src/app/face_pick_tests.rs`
- Modify: `crates/faber-scene/src/lib.rs` (`pick_triangle`), `crates/faber-viewer/src/app.rs`, `src/app/selection.rs`

**Interfaces:**

```rust
// faber-scene: returns the hit distance and the triangle index
pub fn pick_triangle(mesh: &Mesh, origin: DVec3, direction: DVec3, cut: Option<Section>) -> Option<(f64, usize)>;
// viewer
pub(crate) enum Pick { Body(String), Face { part: String, face: u32 }, Edge { part: String, edge: u32 } }
pub(crate) enum SelectFilter { Body, Face, Edge }
```

- With the Face filter, hover tints the face under the cursor; a click selects
  it; Ctrl and Shift add as for bodies. The selection list in the inspector
  shows "Face <n> of <part>" with its area and its type (plane, cylinder ...).
- A mesh-only part (no face index) can only be picked as a body.

- [ ] **Step 1: Write failing tests:** `face_filter_picks_the_top_face_of_a_box`
  (state holds `Pick::Face` whose `Kernel::faces` normal is +Z),
  `face_hover_tints_only_that_face`, `pick_ignores_the_cut_away_half`,
  `mesh_part_is_picked_as_a_body`, unit test `pick_triangle_returns_the_nearest_triangle`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer face_pick_tests -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Edge polylines and edge pick

**Files:**
- Modify: `crates/faber-ports/src/lib.rs` (`EdgeLines`, `Kernel::tessellate_edges`), `crates/faber-kernel-occt/src/bridge.cpp`, `crates/faber-kernel-occt/src/lib.rs`, `crates/faber-core/src/body_cache.rs`, `crates/faber-core/src/lib.rs` (`Body.edge_lines`)
- Create: `crates/faber-wiring/tests/edge_lines.rs`, `crates/faber-viewer/src/app/edge_pick_tests.rs`

**Interfaces:**

```rust
pub struct EdgeLines { pub points: Vec<[f64; 3]>, pub ranges: Vec<std::ops::Range<u32>> } // ranges[i] = points of edge i, as in Kernel::edges
fn tessellate_edges(&self, _shape: &Shape, _tolerance: f64) -> Result<EdgeLines>; // default: not supported
pub fn pick_edge(lines: &EdgeLines, transform: DMat4, view: Mat4, rect: [[f32; 2]; 2], cursor: [f32; 2], max_px: f32) -> Option<u32>;
```

- The edge lines are stored with the mesh cache entry and move with the
  placement like the mesh (no new kernel call on a move).
- Edge pick takes the nearest edge within 8 px of the cursor. The viewer draws
  the edges of the hovered body, and the hovered edge thicker.

- [ ] **Step 1: Write failing tests:** `box_has_twelve_edge_lines`,
  `cylinder_has_three_edge_lines` (two circles and the seam),
  `edge_lines_move_with_the_part` (no `tessellate_edges` call on a move;
  counting kernel), `edge_filter_picks_the_edge_near_the_cursor`,
  `edge_pick_ignores_hidden_parts`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test edge_lines -- --test-threads=1` and the viewer tests. Expected: fail.
- [ ] **Step 3: Implement.** Say before the build that `bridge.cpp` changed.
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Picks in the feature tools, loops and chains

**Files:**
- Create: `crates/faber-core/src/pick_selector.rs`, `crates/faber-wiring/tests/pick_selector.rs`, `crates/faber-viewer/src/app/pick_tools_tests.rs`
- Modify: `crates/faber-viewer/src/feature_form.rs`, `src/app/feature_tools.rs`

**Interfaces:**

```rust
pub fn face_selector(faces: &[FaceInfo], picked: &[u32]) -> Result<FaceSelector<Scalar>>;  // exact match count in `expect`
pub fn edge_selector(edges: &[EdgeInfo], picked: &[u32]) -> Result<EdgeSelector<Scalar>>;
pub fn edge_loop(edges: &[EdgeInfo], faces: &[FaceInfo], face: u32, near: u32) -> Vec<u32>;    // boundary loop of a face that holds the edge
pub fn tangent_chain(edges: &[EdgeInfo], start: u32) -> Vec<u32>;
```

- The selector functions build the smallest selector that matches exactly the
  picked set on the current shape, and check it by running it on the shape.
  If no selector matches exactly, they refuse with "The picked edges cannot be
  described by one selector. Pick fewer edges." (The form then keeps the
  manual fields.)
- Fillet, Chamfer: an "Edges" pick field (edge filter). Shell, Draft, Plane on
  face, Thicken: a "Faces" pick field. Hole: a "Face" field and a click point
  that becomes the hole placement. Sketch plane: a planar face.
- Double-click on an edge selects its tangent chain. Double-click on a face
  edge with the Face filter selects the boundary loop.

- [ ] **Step 1: Write failing tests:** `selector_from_four_top_edges_of_a_box`
  (`expect: 4`, matches exactly those edges), `fillet_from_picked_edges_survives_a_resize`
  (pick, apply, change the box parameter, the fillet still builds and has the
  same edge count), `double_click_selects_the_tangent_chain` (a slot outline:
  all lines and arcs), `hole_from_a_picked_point`, `sketch_on_a_picked_face`,
  `selector_refusal_text`, `purchased_part_cannot_be_the_target`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Press-pull with a distance arrow

**Files:**
- Create: `crates/faber-viewer/src/app/press_pull.rs`, `src/app/press_pull_tests.rs`
- Modify: `crates/faber-viewer/src/app.rs`, `crates/faber-wiring/src/lib.rs` (`preview_feature`)

**Interfaces:**

```rust
// faber-wiring: build the candidate without a save; the viewer draws its mesh
pub fn preview_feature(project: &Project, part: &str, features: &[Feature<Scalar>]) -> Result<Body>;
```

- `Q` (or the toolbar "Press pull") on a picked planar face shows an arrow on
  the face normal and a distance box with a unit. A drag moves the arrow; the
  box follows; typing in the box moves the arrow.
- The result adds two features to the part history: `thicken` of the face
  selector by `|d|`, then `union` (d > 0) or `difference` (d < 0).
- The preview runs on a worker thread. A new drag value cancels the old run.
  At most one preview runs. The last good preview stays on screen while the
  next one builds.
- `Enter` or mouse release saves one revision. `Escape` restores the solid.

- [ ] **Step 1: Write failing tests:** `press_pull_adds_ten_mm`
  (box 20 mm, top face, d = 10: volume grows by 20 * 20 * 10 within 1e-6),
  `press_pull_cuts_five_mm`, `press_pull_refuses_a_curved_face` ("Press pull
  needs a planar face."), `press_pull_through_the_part_is_refused`
  ("The cut removes the whole part."), `escape_during_a_drag_saves_nothing`
  (revision unchanged, files byte-equal), `drag_is_one_revision`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer press_pull_tests -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Marking menu

**Files:**
- Create: `crates/faber-viewer/src/app/marking_menu.rs`, `src/app/marking_menu_tests.rs`
- Modify: `crates/faber-viewer/src/app.rs` (right-click at 1682)

- A right-click press shows a radial menu with eight entries around the
  cursor: Repeat last command, Press pull, Undo, Redo, Move/Copy, Hole, New
  sketch, Delete. Below the ring, the context entries of chunk 17 Task 3 for
  the item under the cursor.
- A right-drag in the direction of an entry and release runs it (gesture).
  A click then a click on an entry also runs it. Escape closes the menu.
- Disabled entries are grey and do nothing.

- [ ] **Step 1: Write failing tests:** `right_click_shows_eight_entries`,
  `gesture_up_runs_the_top_entry`, `repeat_runs_the_last_command`,
  `disabled_entry_does_nothing`, `escape_closes_the_marking_menu`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer marking_menu_tests -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the full workspace tests, Clippy. Expected: pass.

## Close

- `docs/modelling-guide.md` section `## Picking and press pull`.
- `docs/specs/faber-progress.md`, then release, installer, merge and push as in
  `00-roadmap.md`.
- Out of scope: sketching directly on the 3D view (chunk 10 owns sketch
  editing), face move and offset tools (chunk 08).
