# Design structure: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a large design a structure: components and sub-assemblies,
linked copies of a part, a bill of materials, more joint kinds, named
configurations, and parts that follow an external file (for example a Lamina
board).

**Architecture:** A component is a node with a placement and a parent. A part
names its component. The world placement of a part is the product of the
component placements above it and its own placement. An instance is a part
whose geometry is the result of another part. Configurations are named sets of
parameter values. A linked part remembers its source file and its hash.

**Tech Stack:** Rust 2024. No kernel change.

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` (shared rules) and the
Assemble table and "Configurations" row of
`docs/specs/2026-11-18-fusion-feature-gap.md`.

## Global Constraints

- All shared rules of `00-roadmap.md` apply.
- Tasks 1, 3, 4 and 5 change `faber-model`: each change is its own commit.
- An old project (no components, joints with one number for a pose) opens and
  saves with no new bytes until the user adds one of the new items.
- The part `group` field stays. It is a display label. It is not a component.
- Read `crates/faber-model/src/motion.rs` and the motion code of `faber-core`
  before Task 4.

## Review Focus

- A component whose parent chain forms a loop is refused with the ids in the
  loop (Task 1: `component_loop_is_refused`).
- The removal of a component with parts is refused, or moves the parts to its
  parent when `keep_parts` is set (Task 1: `remove_component_with_parts`).
- An instance of an instance and an instance loop are refused (Task 2:
  `instance_chain_is_refused`).
- The removal of a part that has instances is refused with the instance ids
  (Task 2: `source_with_instances_is_not_removed`).
- A linked file that is missing at open time does not stop the project: the
  part keeps its last asset and the status reports the link (Task 6:
  `missing_link_keeps_the_last_asset`).

---

### Task 1: Components

**Files:**
- Modify: `crates/faber-model/src/lib.rs` (`Component`, `ProjectData.components`, `Part.component`, `Edit::SetComponent`, `Edit::RemoveComponent`)
- Modify: `crates/faber-files/src/layout.rs` (`assembly.json` gains `components`; the part file gains `component`)
- Modify: `crates/faber-core/src/lib.rs` (world placement), `crates/faber-core/src/placement.rs`, `crates/faber-core/src/batch.rs`, `src/server.rs`
- Test: `crates/faber-wiring/tests/components.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
pub struct Component { pub name: String, pub parent: Option<String>, pub placement: Placement, pub note: String }
// faber-core
pub fn world_placement(data: &ProjectData, part: &str) -> Result<glam::DAffine3>;
pub fn component_parts(data: &ProjectData, component: &str, deep: bool) -> Vec<String>;
```

- The body cache places the unplaced mesh with `world_placement`. A component move does not tessellate.
- `move_part` and `align_parts` accept `component: <id>` in place of `id`: they change the component placement.
- MCP: `set_component {id, component}`, `remove_component {id, keep_parts?}`, `set_part_info` accepts `component`. `status` lists the component tree with the part count of each node. `batch` accepts the two new tools.
- Motion joints act on world placements as before.

- [ ] **Step 1: Write failing tests.**
  - `nested_placement`: component `a` at (10, 0, 0) turned 90 degrees about Z; a part in `a` at (5, 0, 0): the translation of `world_placement` is (10, 5, 0) within 1e-9, and the body bounds equal the unplaced bounds moved by that transform.
  - `component_move_moves_the_parts_in_one_revision`.
  - `component_move_does_not_tessellate` (count kernel calls, as the body cache tests do).
  - `component_loop_is_refused`: `Component a is its own parent through: a, b.`
  - `remove_component_with_parts`: refused with the part ids; with `keep_parts` the parts keep their world position.
  - `project_without_components_is_byte_identical`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test components -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Instances

**Files:**
- Modify: `crates/faber-model/src/lib.rs` (`Geometry::Instance { of: String }`), `crates/faber-core/src/lib.rs`, `crates/faber-core/src/body_cache.rs`, `crates/faber-core/src/placement.rs` (`copy_as` with `linked: true`), `src/server.rs`
- Test: `crates/faber-wiring/tests/instances.rs`

- An instance has its own placement, colour, component and note. It has no geometry of its own. It shares the unplaced body of its source.
- `move_part` with `copy_as` and `linked: true` makes an instance. A purchased part can have instances (a copy of the geometry is still refused).
- A feature tool on an instance is refused: `Part b is an instance of a. Change part a.`
- New tool `make_independent {id}` turns an instance into a full copy (refused for a purchased source).

- [ ] **Step 1: Write failing tests.**
  - `instance_follows_its_source`: change a feature of the source; the instance bounds change.
  - `instances_share_one_tessellation`: 4 instances give one `tessellate_faces` call.
  - `instance_chain_is_refused`: `Part c cannot be an instance of b: b is an instance.`
  - `source_with_instances_is_not_removed`: `Part a has instances: b, c.`
  - `make_independent_copies_the_recipe`.
  - `four_screws_from_one_purchased_part`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test instances -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Bill of materials

**Files:**
- Create: `crates/faber-core/src/bom.rs`
- Modify: `crates/faber-wiring/src/lib.rs`, `src/server.rs`, `src/main.rs` (`faber bom PROJECT [--format json|csv|md]`)
- Test: `crates/faber-wiring/tests/bom.rs`

**Interfaces:**

```rust
pub struct BomRow { pub item: u32, pub quantity: u32, pub name: String, pub parts: Vec<String>, pub material: Option<String>,
    pub mass_g: Option<f64>, pub source: BomSource /* Made | Purchased { designation, source, licence } */, pub component: Option<String> }
pub fn bom(data: &ProjectData, bodies: &[Body], scope: Option<&str>, flat: bool) -> Vec<BomRow>;
```

- One row for each group of parts with equal resolved geometry and equal material: an instance and its source are one group; two purchased parts with equal designation are one group.
- `scope` limits the list to one component. `flat: false` gives one list for each component level.
- Rows are sorted: made parts first, then purchased, then by name.

- [ ] **Step 1: Write failing tests:** `four_instances_are_quantity_four`, `equal_recipes_group_without_instances`, `different_material_is_a_new_row`, `purchased_rows_have_the_designation`, `csv_has_a_header_and_quoted_names`, `scope_limits_to_a_component`, `mass_is_the_sum_of_the_group`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test bom -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** MCP tool `bom {scope?, flat?, format?}`.
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Joint kinds

**Files:**
- Modify: `crates/faber-model/src/motion.rs`, the motion code of `crates/faber-core`, `crates/faber-viewer/src/app/assembly.rs`, `crates/faber-viewer/src/app/motion_relations.rs`
- Test: `crates/faber-wiring/tests/joints.rs`

**Interfaces:**

```rust
pub enum JointKind { Revolute, Slider, Rigid, Cylindrical, PinSlot, Planar, Ball }
// Joint gains (all optional, default absent):
//   second_axis: Option<[f64; 3]>            // slot direction of PinSlot; X direction of Planar
//   limits2: Option<[f64; 2]>, limits3: Option<[f64; 2]>
// A pose value is one number or a list, by joint kind:
//   Revolute [angle]  Slider [travel]  Rigid []  Cylindrical [angle, travel]
//   PinSlot [angle, travel along second_axis]  Planar [x, y, angle]  Ball [rx, ry, rz] (rotation vector, degrees)
```

- A single number still loads and saves as a number for `Revolute` and `Slider`.
- `Ball` limit: `limits` is the cone half angle range from the `axis`.
- `Rigid` joins parts so that they move as one in a study; it has no value.
- Gears and four-bar linkages accept `Revolute` and `Slider` joints only, as before.

- [ ] **Step 1: Write failing tests.**
  - `cylindrical_turns_and_slides`: pose `[90, 5]` moves a point at radius 10 to the turned position plus 5 along the axis, within 1e-9.
  - `pin_slot_slides_along_the_second_axis`.
  - `planar_moves_in_its_plane`: pose `[3, 4, 90]`.
  - `ball_turns_about_a_vector`: pose `[0, 0, 90]` equals a revolute of 90 about Z.
  - `ball_limit_is_a_cone`: a pose outside the cone is refused with the angle and the limit.
  - `rigid_group_moves_as_one`: parts joined by `Rigid` to a slider part follow the slider.
  - `old_single_number_pose_loads_and_saves_identical`.
  - `wrong_value_count_is_refused`: `Joint hinge is cylindrical. A pose needs 2 values. It has 1.`
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test joints -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** The viewer shows one slider for each value of a joint.
- [ ] **Step 4: Run** the new tests and the existing motion tests. Expected: pass.

### Task 5: Configurations

**Files:**
- Modify: `crates/faber-model/src/lib.rs` (`Configuration`, `ProjectData.configurations`, `ProjectData.active_configuration`, `Edit::SetConfiguration`, `Edit::UseConfiguration`), `crates/faber-files/src/layout.rs` (`faber.json`), `crates/faber-core/src/lib.rs`, `crates/faber-wiring/src/lib.rs` (export), `crates/faber-build/src/lib.rs`, `src/server.rs`, `src/main.rs`
- Test: `crates/faber-wiring/tests/configurations.rs`, `crates/faber-build/tests/build.rs`

**Interfaces:**

```rust
pub struct Configuration { pub params: BTreeMap<String, String /* expression */>, pub suppressed: Vec<SuppressRef /* part, feature */>, pub note: String }
```

- `use_configuration {name}` sets the listed parameters and the suppress states in one revision and records `active_configuration`. A parameter that is not in the list keeps its value. A name that is not a parameter is refused.
- `export` accepts `configuration: <name>` and `all_configurations: true` (one sub-folder for each). The export does not change the saved project.
- With a Jsonnet source, `design.jsonnet` can give `configurations` in its result; `use_configuration` then is the only change tool for parameters that works on a generated project.
- MCP: `set_configuration {name, configuration}`, `remove_configuration {name}`, `use_configuration {name}`; `status` lists the names and the active one.

- [ ] **Step 1: Write failing tests:** `use_sets_two_params_in_one_revision_and_undo_restores`, `unknown_param_is_refused`, `configuration_can_suppress_a_feature`, `export_of_a_configuration_leaves_the_project_unchanged`, `all_configurations_export_one_folder_each`, `configuration_that_does_not_build_is_refused_and_the_active_one_stays`, `generated_project_takes_configurations_from_the_source`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test configurations -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 6: Linked parts

**Files:**
- Modify: `crates/faber-model/src/lib.rs` (`Geometry::Imported` gains `link: Option<Link>`), `crates/faber-wiring/src/lib.rs`, `src/server.rs`
- Test: `crates/faber-wiring/tests/links.rs`

**Interfaces:**

```rust
pub struct Link { pub path: String /* relative to the project folder or absolute */, pub hash: String }
```

- `import_part` accepts `link: true`: it stores the path and the hash of the file with the asset.
- `links` (read) lists each linked part with `current | changed | missing`. `refresh_links {parts?}` imports each changed file again as a new asset, in one revision, and keeps the placement.
- A linked part is not a purchased part, but geometry tools refuse it: `Part board follows the file <path>. Change the file, then call refresh_links.`
- The guide documents the Lamina use: export the board as STEP from Lamina to a fixed path, link it once, call `refresh_links` after each board change.

- [ ] **Step 1: Write failing tests:** `changed_file_is_reported`, `refresh_updates_the_asset_and_keeps_the_placement`, `refresh_is_one_revision_and_undo_restores_the_old_asset`, `missing_link_keeps_the_last_asset`, `geometry_tool_on_a_linked_part_is_refused`, `relative_path_follows_the_project_folder`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test links -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** Use a STEP file written by `Kernel::write_shape` in the test. No network.
- [ ] **Step 4: Run.** Expected: pass.

### Task 7: Viewer browser, guide, example

**Files:**
- Modify: `crates/faber-viewer/src/app.rs` (browser tree), `crates/faber-viewer/src/app/assembly.rs`, `docs/modelling-guide.md` (sections `## Components and instances`, `## Configurations`, `## Linked parts`), `crates/faber-build/src/faber.libsonnet` (`component`, `instance`, `configuration`)
- Create: `examples/structured-assembly/`

- The browser shows the component tree. A part can be dragged to a component. "Isolate" shows one component. The Move/Copy panel has a "Linked copy" switch. A "Parts list" panel shows the bill of materials. A configuration selector is in the toolbar.

- [ ] **Step 1: Write failing viewer tests:** `browser_shows_the_tree`, `isolate_hides_the_other_parts`, `linked_copy_makes_an_instance`, `configuration_selector_changes_the_size`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** all viewer tests and `cargo test --offline -j 4 --test mcp --test context_size -- --test-threads=1`. Expected: pass.
