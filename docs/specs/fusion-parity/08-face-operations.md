# Face operations: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Change a solid through its faces: remove faces (holes, fillets,
bosses), move faces (press and pull), split a face, and emboss a profile or a
text on a flat or a cylindrical face.

**Architecture:** Each operation is a variant of `faber_model::Geometry` in a
part history. Remove, move and split are new kernel calls. Emboss is built in
`faber-core` from a sketch on the face plane, or from a new kernel call that
wraps a flat face on a cylinder.

**Tech Stack:** Rust 2024, OCCT 7.9.3 (`BRepAlgoAPI_Defeaturing`,
`BRepOffset_MakeOffset` with a per-face offset, `BRepFeat_SplitShape`,
`Geom_CylindricalSurface` with 2D curves).

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` and the Modify table of
`docs/specs/2026-11-18-fusion-feature-gap.md` (Press Pull, Split Face, Remove
Faces, Emboss). "Replace Face" is done with the surface split of chunk 13.

## Global Constraints

- All shared rules of `00-roadmap.md` apply.
- Task 1 changes `faber-model` (all variants in one commit). Tasks 2 to 5
  change `bridge.cpp`.
- These operations work on a part with an exact shape. A mesh-only part is
  refused: `Part <id> is a mesh. Convert it to a solid first.`
- A solid from an imported file that is not purchased can be the base of a
  history. A purchased part is refused.
- Face selection uses the existing `FaceSelector`. The result keeps the face
  history (`face_sources`) so later selectors work.

## Review Focus

- A face removal that the kernel cannot heal is refused and the part stays
  unchanged (Task 2: `removal_that_cannot_heal_is_refused`).
- A move that makes the solid invalid (a face moved through the opposite
  face) is refused with the distance and the available thickness (Task 3:
  `move_through_the_opposite_face_is_refused`).
- A split tool that does not cross the face is refused (Task 4:
  `plane_that_misses_the_face_is_refused`).
- An emboss profile larger than the face is refused, not clipped in silence
  (Task 5: `profile_outside_the_face_is_refused`).
- A wrapped profile wider than the circumference is refused (Task 5:
  `wrap_longer_than_the_circumference_is_refused`).

---

### Task 1: Model variants

**Files:**
- Modify: `crates/faber-model/src/lib.rs`, `crates/faber-geometry/src/lib.rs` (not built by the native kernel)
- Test: unit tests in `crates/faber-model/src/lib.rs`

**Interfaces:**

```rust
RemoveFaces { body: String, faces: Vec<FaceSelector<N>> }
MoveFaces   { body: String, faces: Vec<FaceSelector<N>>, distance: N }   // positive adds material
SplitFace   { body: String, face: FaceSelector<N>, tool: SplitTool<N> }  // Plane { plane: String } | Sketch { sketch: String }
Emboss      { body: String, face: FaceSelector<N>, sketch: String, depth: N, mode: EmbossMode /* raise | engrave */,
              at: Option<[N; 2]> /* cylinder: arc length and axis position of the sketch origin */ }
```

- [ ] **Step 1: Write failing tests:** JSON round trip with the `op` names `remove_faces`, `move_faces`, `split_face`, `emboss`; unknown fields are refused.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-model -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Remove faces

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`, `crates/faber-core/src/history.rs`
- Test: `crates/faber-wiring/tests/remove_faces.rs`

**Interfaces:**

```rust
fn remove_faces(&self, _shape: &Shape, _faces: &[u32]) -> Result<Shape>;
```

- [ ] **Step 1: Write failing tests** (box 20 mm):
  - `hole_is_removed`: a through hole of diameter 5; remove its cylinder face: volume 8000 within 1e-6, 6 faces.
  - `fillet_is_removed`: one edge fillet of radius 2; remove the fillet face: volume 8000, 6 faces.
  - `boss_is_removed`: a cylinder boss on the top; remove its side and top faces: volume 8000.
  - `removal_that_cannot_heal_is_refused`: remove one side face of the box: error `The faces cannot be removed: the neighbour faces do not close the solid.`; the saved files are byte-equal.
  - `later_selector_still_works`: after a removal, a fillet by an edge selector on another edge builds.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test remove_faces -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Move faces

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`, `crates/faber-core/src/history.rs`
- Test: `crates/faber-wiring/tests/move_faces.rs`

**Interfaces:**

```rust
/// Offset the listed faces along their normals. The other faces extend or trim to meet them.
fn move_faces(&self, _shape: &Shape, _faces: &[u32], _distance: f64) -> Result<Shape>;
```

- [ ] **Step 1: Write failing tests** (box 20 mm):
  - `top_face_up_by_two`: volume 8800 within 1e-6; the top face Z grows by 2.
  - `top_face_down_by_five`: volume 6000.
  - `hole_gets_larger`: a through hole of diameter 5, distance -0.5 on its face: volume `8000 - pi * 9 * 20 = 7434.51` within 0.1 %.
  - `two_opposite_faces`: both side faces by 1: X size 22.
  - `face_next_to_a_fillet`: the top face of a box with a fillet on a top edge moves up by 2; the fillet face is still present.
  - `move_through_the_opposite_face_is_refused`: distance -25: `Distance -25 mm removes the body: the thickness at the face is 20 mm.`
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test move_faces -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** For one planar face, a prism of the face joined or cut is an accepted method if the general offset fails; the result must pass the same tests.
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Split face

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`, `crates/faber-core/src/history.rs`
- Test: `crates/faber-wiring/tests/split_face.rs`

**Interfaces:**

```rust
/// Split one face along the section with a tool shape (a plane face or the prism of a sketch). The solid is unchanged.
fn split_face(&self, _shape: &Shape, _face: u32, _tool: &Shape) -> Result<Shape>;
```

- [ ] **Step 1: Write failing tests** (box 20 mm):
  - `plane_splits_the_top`: 7 faces; the two new faces have area 200 each; volume 8000.
  - `sketch_circle_splits_the_top`: 7 faces; one has area `pi * 25` within 0.1 %.
  - `split_face_can_be_drafted`: draft one of the two new faces by 5 degrees; the volume changes and the other half stays flat.
  - `split_face_can_be_moved`: `move_faces` on the circle face by 1 gives a boss of volume `pi * 25`.
  - `plane_that_misses_the_face_is_refused`.
  - `face_selectors_find_both_halves`: `created_by` of both halves names the split feature.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test split_face -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Emboss

**Files:**
- Create: `crates/faber-core/src/emboss.rs`
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`, `crates/faber-core/src/history.rs`
- Test: `crates/faber-wiring/tests/emboss.rs`

**Interfaces:**

```rust
/// Map a flat face onto a cylinder: sketch X is arc length, sketch Y is the axis direction.
fn wrap_on_cylinder(&self, _flat: &Shape, _cylinder: &CylinderFace, _at: [f64; 2]) -> Result<Shape>;
```

- Flat face: the sketch (loops or text) is on the face plane. Raise: extrude by `depth` and join. Engrave: extrude into the body and cut.
- Cylinder face: `wrap_on_cylinder` makes the wrapped face; `thicken` by `depth` (outward to raise, inward to engrave); then join or cut.
- A face that is not flat and not a cylinder is refused: `Emboss needs a flat or a cylindrical face.`

- [ ] **Step 1: Write failing tests.**
  - `raised_rectangle_on_a_flat_face`: 10 x 5 raised 1 on the top of a 20 mm box: growth 50 within 1e-6.
  - `engraved_text`: text `AB` of height 6 engraved 0.6: the volume falls by the text area times 0.6 within 1 % (the area from the same sketch extruded alone).
  - `raised_rectangle_on_a_cylinder`: cylinder of radius 20 and height 30; rectangle 10 (arc) x 5 raised 1: growth 51.25 within 2 % (`0.5 * (21^2 - 20^2) * 0.5 * 5`).
  - `engraved_text_on_a_cylinder`: the volume falls; one solid; the bounds are unchanged.
  - `profile_outside_the_face_is_refused`.
  - `wrap_longer_than_the_circumference_is_refused`: `The profile is 130 mm wide. The circumference is 125.66 mm.`
  - `emboss_follows_a_text_parameter`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test emboss -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 6: Viewer tools, guide, example

**Files:**
- Modify: `crates/faber-viewer/src/app.rs` (face context menu), `crates/faber-viewer/src/app/feature_tools.rs`, `crates/faber-viewer/src/feature_form.rs`, `crates/faber-build/src/faber.libsonnet`, `docs/modelling-guide.md` (section `## Face operations`)
- Create: `examples/label-bottle/` (a bottle with an embossed label on its cylinder)

- A right click on a face shows "Remove face", "Move face" (with a drag arrow along the normal and a number field), "Split face" and "Emboss".

- [ ] **Step 1: Write failing viewer tests:** `face_menu_has_the_four_items`, `move_face_drag_previews_and_saves_one_feature`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer face -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.
