# Review fixes, speed, AI use and modular project files: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Correct the defects of the 2026-11-18 review, make changes and reads
fast for an AI client, reduce the MCP context cost, and store a project as
modular files with an optional Jsonnet source, as Lamina does.

**Architecture:** `ProjectData` stays the one in-memory model. The file store
adapter maps it to several files. The core keeps an unplaced shape and mesh for
each part, so a placement change does not tessellate. The MCP server sends
small schemas and small replies and gives the detail on request.

**Tech Stack:** Rust 2024, OCCT 7.9.3 through `faber-kernel-occt`, rmcp,
schemars, egui/wgpu, jrsonnet 0.5.0-pre98 (as in Lamina).

**Spec:** `docs/specs/2026-11-17-faber-design.md` (section 5.2 gives the file
layout). Review evidence: the three review reports of 2026-11-18, summarised
in each task. Lamina reference: `C:\Users\mranderson\projects\lamina`
(`crates/lamina-build`, `crates/lamina-ports/src/source.rs`).

## Global Constraints

- Read `AGENTS.md` first. All text is ASD-STE100 with British spelling.
- No AI attribution anywhere.
- **No task makes a commit.** Several workers share one working tree. The
  coordinator commits at the end. A `faber-model` change goes in its own commit.
- `faber-core` and the domain crates never name a kernel, a file format or a
  vendor. Add a port method in `faber-ports`, implement it in an adapter, plug
  it in `faber-wiring`.
- Cargo: `--offline`, `-j 4`, one cargo process at a time for each worker.
  Tests run with `-- --test-threads=1`. Tests never use the network.
- Never open a window. Use headless tests (`egui_kittest`) and `faber render`.
- A change that cannot be built is refused and leaves the files unchanged.
- Old projects (one `faber.json` with inline parts) must still open.
- Edit shared files with small unique anchors. Never rewrite a whole file.
  If a build fails in a file of another worker, wait and try again.
- Each task ends with: `cargo fmt --all`, then
  `cargo clippy --offline -j 4 --workspace --all-targets -- -D warnings`,
  then the test commands of the task.

## Review Focus

1. A person opens a schema 1 project, makes one change and opens it with an
   older viewer build: the split layout must be complete and `faber.json` must
   say schema 2, so the old build refuses with a clear message (Task 7).
2. An AI edits `parts/<id>.json` as text with a syntax error: the next tool
   call must name the file and the JSON position, and must not lose the other
   parts (Task 7).
3. A part is moved 1 000 times: bounds and mesh must not drift, because each
   placement is applied to the unplaced mesh, not to the last placed mesh
   (Task 3).
4. A reply limit hides data: every limited list must give `total`, so the AI
   knows that more items exist (Tasks 4 and 6).
5. Two ring drags about different axes in Move/Copy: the saved orientation
   must equal the previewed orientation (Task 1).

---

### Task 1: Move/Copy tool correctness

**Files:**
- Modify: `crates/faber-viewer/src/app/move_tool.rs`
- Modify: `crates/faber-core/src/placement.rs` (make `tidy` public; refuse
  copies of purchased parts)
- Test: tests in `move_tool.rs`, unit tests in `placement.rs`, `tests/mcp.rs`

**Interfaces:**
- Produces: `pub fn tidy(value: f64) -> f64` in `faber_core` (re-export in
  `lib.rs`). `Project::move_part` returns
  `Err("Part <id> is a purchased part. A copy is not permitted.")` when
  `copy_as` is set and the geometry is `Geometry::Imported` with `purchased`.

Defects to correct (review findings 1 to 5, 8, 10):

- [ ] **Step 1: Write failing tests** in `move_tool.rs`:
  - `mode_change_drops_hidden_translate`: Move mode, X distance 10, select
    Rotate, Z angle 90, OK. Saved translation equals the pure rotation result
    (no +10 in X).
  - `two_ring_drags_save_the_previewed_orientation`: ring Z by 30, then ring Y
    by 15. Saved rotation matrix equals `Ry(15) * Rz(30) * base` within 1e-9,
    and equals the preview pose.
  - `drags_respect_the_mode`: in Translate mode a ring drag changes nothing; in
    Rotate mode an arrow drag changes nothing.
  - `enter_reads_typed_fields_first`: type 5 in X distance and press Enter in
    the same frame sequence used by the app (`move_keys` before `move_panel`).
    Saved X shift is 5.
  - `copy_of_purchased_part_is_refused` (viewer and `tests/mcp.rs`).
  - `saved_angles_have_no_float_noise`: rotate Z 90 saves exactly `90.0`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer move_tool -- --test-threads=1`. Expected: the new tests fail.
- [ ] **Step 3: Implement.**
  - Store the turn as a `DQuat`. A ring drag does
    `turn = DQuat::from_axis_angle(world_axis, snapped_delta) * turn`. The six
    angle fields show `euler_degrees(turn)`; a typed angle field rebuilds
    `turn` from the three field values. Snap applies to the drag delta
    (15 degree steps from the drag start).
  - `select_mode` resets `translate` and `turn` to the values that the new mode
    shows. `result()` uses only the values of the current mode.
  - Arrow drags act in Free and Move modes only. Ring drags act in Free only.
  - `confirm_move` calls `read_fields()` first.
  - Pivot choices: "Bounding box centre", "Part origin" (part translation) and
    "World origin" (0,0,0). The MCP value `origin` stays the world origin; add
    the sentence "origin is the world origin" to the `move_part` description.
  - `compose` passes each saved value through `faber_core::tidy`.
- [ ] **Step 4: Run** the viewer tests, `cargo test --offline -j 4 -p faber-core placement -- --test-threads=1` and `cargo test --offline -j 4 --test mcp -- --test-threads=1`. Expected: all pass.

### Task 2: Core defects from the review

**Files:**
- Modify: `crates/faber-core/src/overhang.rs`
- Modify: `crates/faber-kernel-occt/src/lib.rs`, `crates/faber-kernel-occt/src/sketch_text.rs`
- Modify: `crates/faber-core/src/lib.rs` (`apply_history` only)
- Modify: the interference report code (find `allowed_contacts`), `src/server.rs` (direct-call error prefix), `src/input.rs`
- Test: unit tests in `overhang.rs`; `crates/faber-wiring/tests/sketch_text.rs`, `suppress.rs`, `contacts.rs`; `tests/error_paths.rs`

- [ ] **Step 1: Write failing tests.**
  - `overhang::tests::regions_join_across_face_seams`: two quads that meet at
    an edge but share no vertex index, both overhang. `regions_total == 1`.
  - `overhang::tests::non_finite_vertex_is_refused`: a NaN vertex gives `Err`.
  - `sketch_text.rs::extrude_refuses_a_non_text_open_surface`: an extrude of a
    solid-free shape that does not come from a text sketch returns the old
    refusal, not a union of prisms.
  - `suppress.rs::no_op_suppress_keeps_the_revision`: suppress twice; the
    second call leaves `revision` and the undo history unchanged.
  - `contacts.rs::declared_touch_is_reported_as_allowed`: parts that touch with
    a declared contact (`max_depth_mm: 0`) appear in `allowed_contacts` with
    kind `allowed_contact`.
  - `error_paths.rs::direct_call_error_has_no_batch_prefix`: `set_contact` with
    an unknown part returns `Unknown part: ghost` without `Batch operation 1:`.
  - `error_paths.rs::selector_list_error_names_the_cause`: `edges` given as a
    list returns a message that contains `edges takes one selector object, not a list`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
  - `overhang::analyse`: weld vertices by position rounded to 1e-6 mm before
    the edge map is made. `check` refuses non-finite vertices.
  - `extrude_regions` runs only when `sketch()` marked the shape as text
    regions (carry a marker in the adapter shape type; no port change).
  - `apply_history`: when the candidate equals `self.data` except `revision`,
    return `Ok(())` without a save. Apply the same rule to `SetPartInfo`.
  - Direct tool calls strip the `Batch operation 1: ` prefix.
- [ ] **Step 4: Run** `cargo test --offline -j 4 -p faber-core -- --test-threads=1`, `cargo test --offline -j 4 -p faber-wiring --test sketch_text --test suppress --test contacts -- --test-threads=1`, `cargo test --offline -j 4 --test error_paths -- --test-threads=1`. Expected: pass.

### Task 3: Fast changes: unplaced body cache and section reuse

Evidence: on `examples/trash-bin` each change takes about 4.5 s, a move
included. `build_part` (`crates/faber-core/src/lib.rs`) tessellates the placed
shape at 0.03 mm (98 838 triangles) and reads its properties for each change.
Three saved sections add about 0.5 s to each change.

**Files:**
- Create: `crates/faber-core/src/body_cache.rs`
- Modify: `crates/faber-core/src/lib.rs` (`prepare`, `build_part`, `Project` fields)
- Modify: `crates/faber-core/src/history.rs` (expose the cache key of the output)
- Test: `crates/faber-wiring/tests/body_cache.rs` (new), unit tests in `body_cache.rs`

**Interfaces:**
- Produces (crate-private):

```rust
/// The unplaced result of one part. Valid while the resolved geometry is equal.
pub(crate) struct LocalBody {
    pub geometry: faber_model::Geometry,   // resolved numbers
    pub shape: Option<faber_ports::Shape>, // unplaced exact shape
    pub mesh: faber_ports::Mesh,           // unplaced mesh, 0.03 mm
    pub volume: f64,
    pub bounds: [[f64; 3]; 2],             // unplaced bounds
}
pub(crate) fn place(local: &LocalBody, placement: &Placement, kernel: &dyn Kernel) -> Result<PlacedParts>;
pub(crate) struct PlacedParts { pub mesh: Mesh, pub bounds: [[f64; 3]; 2], pub shape: Option<Shape> }
```

- `Project` gains `locals: BTreeMap<String, LocalBody>`, kept across changes
  and dropped for removed parts. `Prepared` carries the next map.
- Env `FABER_TIMING=1` prints one line for each part build to stderr:
  `timing <part> history=<ms> properties=<ms> tessellate=<ms> place=<ms> sections=<ms>`.

- [ ] **Step 1: Write failing tests** in `crates/faber-wiring/tests/body_cache.rs`. Count kernel calls with a wrapper `Kernel` that delegates to `OcctKernel` and counts `tessellate` and `properties` calls (see how other wiring tests build a `Project` with a custom kernel; add a small test helper if none exists):
  - `placement_change_does_not_tessellate`: build a part, change only the
    placement. `tessellate` count stays the same. Bounds equal the bounds of a
    fresh build within 1e-6. Mesh vertex count is equal.
  - `thousand_moves_do_not_drift`: translate by +0.1 mm 1 000 times, then back
    by -100 mm. Bounds equal the start bounds within 1e-9.
  - `rotation_rotates_normals`: after a 90 degree turn about Z, each normal
    equals the turned unplaced normal within 1e-12.
  - `geometry_change_tessellates_once`.
  - `section_of_an_unchanged_part_is_not_recomputed`: count `faces` (or the
    port call that `section::world_plane` uses) across a change of another part.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test body_cache -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
  - `build_part` builds a `LocalBody` (history build, `properties` and
    `tessellate` of the **unplaced** shape) only when no local with equal
    resolved geometry exists. It then calls `place`.
  - `place`: `kernel.transform_shape` for the shape (with `provenance::track`
    as today), mesh vertices and normals by the placement matrix in Rust
    (R = Rz * Ry * Rx, then translation; the same code as the mesh-only path),
    bounds from the transformed mesh. Volume is copied.
  - `prepare`: verify a section only when the section itself, its part, or a
    parameter that the section uses changed since `self.data`. Otherwise keep it.
  - Persist the unplaced mesh through the existing `FeatureCache` port under
    key `mesh-0.03:<output key>` for history parts (bincode, as the feature
    cache does), so a warm `open` does not tessellate.
  - Add the `FABER_TIMING` lines.
- [ ] **Step 4: Run** the new tests, then `cargo test --offline -j 4 -p faber-wiring -- --test-threads=1`. Expected: pass.
- [ ] **Step 5: Measure.** Copy `examples/trash-bin` to a temporary folder. With the MCP driver (`scratchpad/fmcp.py`, see `docs/specs/faber-progress.md`) time `open` (warm), `move_part`, `set_param` and `suppress_feature` with `FABER_TIMING=1`. Put the before and after medians and the timing lines in the task report. Target: `move_part` below 500 ms in a debug build.

### Task 4: Fast reads: topology, mass, section, STL export, cache upkeep

**Files:**
- Modify: `crates/faber-core/src/lib.rs` (`selected_topology`, `mass_properties`), `crates/faber-core/src/section.rs`, `crates/faber-core/src/select.rs`
- Modify: `src/server.rs` (`topology`, `export` arguments), `src/main.rs` (CLI `export --tolerance`), `crates/faber-wiring/src/lib.rs` (STL export)
- Modify: `crates/faber-files/src/feature_cache.rs`
- Test: `crates/faber-wiring/tests/read_cache.rs` (new), `tests/mcp.rs`, `tests/cli_export.rs`, tests in `feature_cache.rs`

**Interfaces:**
- `topology` arguments: `limit` defaults to 50 for each list. The reply always has `total_edges` and `total_faces`. `kind: "edges" | "faces"` lists only that kind in the kernel too.
- `export` arguments: `tolerance_mm: Option<f64>`, default `0.01`, accepted range 0.001 to 1.0. The reply names the tolerance and the triangle count of each file.
- `Project` gains `read_cache: RefCell<ReadCache>` (or a `Mutex`), keyed by `(revision, tool, arguments)`, for topology lists, `mass_properties` and `section_info`. Each save clears it.

- [ ] **Step 1: Write failing tests.**
  - `read_cache.rs::second_topology_call_uses_the_cache`: the kernel face and edge list calls do not grow on the second equal call.
  - `read_cache.rs::edge_kind_does_not_list_faces`.
  - `read_cache.rs::mass_and_section_are_cached_per_revision`: equal results; the kernel call count grows once; a save clears the cache.
  - `mcp.rs::topology_default_limit_is_50_with_totals`: a cylinder-patterned part with more than 50 faces returns 50 faces and the true `total_faces`.
  - `cli_export.rs::stl_tolerance_changes_the_triangle_count`: a sphere of radius 20 at 0.1 mm has fewer triangles than at 0.01 mm; `2.0` is refused.
  - `feature_cache::tests::put_does_not_list_the_folder`: with a counter hook (or by design: a running total field), 100 puts list the folder at most once.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.** For `mass_properties`, keep the present numeric tolerance (do not trade accuracy; the cache gives the gain). In `feature_cache`, keep a running byte total in memory, list the folder once at open and when the total passes the limit; verify the SHA-256 only when the decode fails.
- [ ] **Step 4: Run** `cargo test --offline -j 4 -p faber-wiring --test read_cache -- --test-threads=1`, `cargo test --offline -j 4 -p faber-files -- --test-threads=1`, `cargo test --offline -j 4 --test mcp --test cli_export -- --test-threads=1`. Expected: pass.

### Task 5: Small MCP context: schemas and instructions

Evidence: `tools/list` is 203 494 characters. Six tools repeat the same
geometry definitions (about 28 to 37 KB each). `initialize` sends the whole
guide (48 005 characters) as instructions.

**Files:**
- Modify: `src/server.rs`
- Create: `src/schema_tool.rs` (declare in `src/main.rs`)
- Modify: `docs/modelling-guide.md` (add headings that `guide` can select)
- Test: `tests/mcp.rs`, `tests/context_size.rs` (new)

**Interfaces:**
- New tool `schema`: arguments `{ "kind": "part" | "feature" | "geometry" | "sketch_loop" | "selector" | "section" | "edit", "op": Option<String> }`. With `kind: "geometry"` and no `op` it returns the compact operation list: one line for each op with its field names and types, for example `pipe: path (id | curve), diameter, wall?`. With `op` it returns the JSON schema of that one operation with its definitions.
- The tools `put_part`, `put_parts`, `add_feature`, `edit_feature`, `batch` and `preview_batch` declare their recipe arguments as free JSON objects in `tools/list` (`#[schemars(with = "serde_json::Value")]` and a description that says "Read the shape with the schema tool"). The server still parses them with the same typed structures and returns the same path errors (`src/input.rs`).
- `guide` gains `topic: Option<String>`. Without a topic it returns the list of topics (headings) and the quick start. With a topic it returns that section.
- `initialize` instructions: a quick start of at most 2 500 characters: units and axes, the open/status/preview/batch loop, how to read `schema` and `guide`, the rule to verify with `measure` and `part_info`.

- [ ] **Step 1: Write failing tests** in `tests/context_size.rs` (use the session helper of `tests/mcp.rs`):
  - `tools_list_is_small`: the serialised `tools/list` result is below 60 000 characters, and no single tool is above 6 000.
  - `instructions_are_short`: at most 2 500 characters and they name `guide` and `schema`.
  - `schema_tool_gives_one_op`: `schema {kind:"geometry", op:"pipe"}` has `diameter` and `wall` and is below 4 000 characters; an unknown op returns an error that lists the nearest names.
  - `op_list_names_every_op`: the compact list has one line for each `Geometry` variant (compare with the names from the schemars schema, so a new variant cannot be forgotten).
  - `typed_errors_survive`: `put_part` with a wrong field in a nested feature still returns the JSON path error.
  - `guide_topic_returns_one_section`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 --test context_size -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** Generate the compact op list from the schemars schema of `Geometry` at start-up, not by hand. Unknown names use the edit distance to suggest up to three names (`Unknown op boxx. Nearest: box.`); use the same helper for unknown tool names in `batch`.
- [ ] **Step 4: Run** `cargo test --offline -j 4 --test context_size --test mcp --test error_paths --test batch_errors -- --test-threads=1`. Expected: pass.

### Task 6: Batch coverage, small replies, feature inspection

**Files:**
- Modify: `crates/faber-core/src/batch.rs`, `crates/faber-core/src/placement.rs` (pure functions that return an `Edit`), `src/server.rs`, `docs/modelling-guide.md`
- Test: `crates/faber-wiring/tests/batch.rs`, `tests/mcp.rs`, `tests/batch_errors.rs`

**Interfaces:**
- `batch` and `preview_batch` operations add: `move_part`, `align_parts`, `suppress_feature`, `set_part_info`, `set_contact`, `set_section`, `remove_section`. The names equal the tool names. A later operation sees the result of an earlier one (a move after a feature change uses the new bounds).
- Reply of each changing tool, default: `{schema, revision, changed: [part summary], part_count}`. A part summary is `{id, volume_mm3, bounds_mm, triangles}`. `verbose: true` returns all parts as today. `set_param` returns `{revision, param: {name, expression, value}, changed: [...]}`.
- Every changing tool accepts `expected_revision`.
- New read tool `feature_info {part, feature}`: for a solid feature `{kind, volume_mm3, bounds_mm, faces, edges, suppressed, depends_on}` in part coordinates; for a plane, axis or point feature `{kind, origin, x, y, normal}` or `{origin, direction}` or `{point}`.
- `part_info` gains `features: "full" | "ids"` (default `"full"`).
- `render`: the image content block is sent only when `inline: true` (default `false`). The reply always has the path.
- Error texts carry the values: `Pipe wall 6 mm must be less than the radius 5 mm.`, `Text height -5 mm must be between 0.000001 and 1000000 mm.`

- [ ] **Step 1: Write failing tests.**
  - `batch.rs::assembly_in_one_batch`: three `put_part`, two `align_parts` (centre, then `min_to_max`), one `move_part` rotate and one `set_contact` in one batch give one revision step and the same placements as the single calls.
  - `batch.rs::batch_move_uses_the_new_bounds`: `edit_feature` that doubles a box height, then `align_parts` of a second part on top: the second part sits on the new top.
  - `mcp.rs::changing_tools_reply_with_changed_parts_only` (project with 3 parts; `set_part_info` reply has one entry in `changed`; with `verbose` it has `parts` with 3).
  - `mcp.rs::every_changing_tool_checks_expected_revision` (loop over the changing tools with a stale revision; each returns `expected <n>, current <m>`).
  - `mcp.rs::feature_info_reads_a_solid_and_a_plane`.
  - `mcp.rs::render_sends_no_image_by_default`.
  - `batch_errors.rs::value_errors_name_the_values`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.** Correct the guide: the list of batch tools, the meaning of text `at` (baseline start; only the horizontal position follows `align`), a short example for a face that an earlier feature split (`in_box`).
- [ ] **Step 4: Run** `cargo test --offline -j 4 -p faber-wiring --test batch -- --test-threads=1`, `cargo test --offline -j 4 --test mcp --test batch_errors --test context_size -- --test-threads=1`. Expected: pass (the size limits of Task 5 still hold).

### Task 7: Modular project files

The layout follows the design (section 5.2) and Lamina: recipe data and
placement data are in different files.

```
faber.json        {schema: 2, revision, name, params}
parts/<id>.json   {schema: 1, name, colour, group, note, geometry}
placement.json    {schema: 1, parts: {<id>: {translation, rotation}}}
assembly.json     {schema: 1, motion, contacts}      (absent when empty)
analysis.json     {schema: 1, sections}              (absent when empty)
```

**Files:**
- Create: `crates/faber-files/src/layout.rs`
- Modify: `crates/faber-files/src/lib.rs` (`read`, `save`, `load`), `crates/faber-files/src/history.rs` (no format change; snapshots stay whole `ProjectData`)
- Modify: `crates/faber-viewer/src/app.rs`, `app/assembly.rs`, `edit.rs` (change detection), `crates/faber-wiring/src/lib.rs`, `crates/faber-wiring/src/archive.rs`
- Modify: `src/main.rs` (CLI `faber migrate <dir>`), `examples/*/` (migrated files), `docs/specs/2026-11-17-faber-design.md` (section 5.2), `docs/modelling-guide.md`, `README.md`
- Test: `crates/faber-files/tests/layout.rs` (new), `crates/faber-wiring/tests/files.rs`, the tests that read `faber.json` directly

**Interfaces:**

```rust
// crates/faber-files/src/layout.rs
/// Reads a project folder. Accepts schema 1 (one file) and schema 2 (split).
pub fn read(root: &Path) -> Result<ProjectData>;
/// Writes the split layout. Writes only files whose bytes change. Removes
/// part files of removed parts. Writes faber.json last.
pub fn write(root: &Path, data: &ProjectData) -> Result<Vec<PathBuf>>; // changed paths
/// A value that changes when a layout file changes (newest mtime and total length).
pub fn fingerprint(root: &Path) -> Result<(std::time::SystemTime, u64)>;
```

- `ProjectData` does not change. `schema` in memory stays as today; the store maps it.
- The viewer uses `faber_files::fingerprint` (through `faber_wiring`) in place of the mtime of `faber.json`.

- [ ] **Step 1: Write failing tests** in `crates/faber-files/tests/layout.rs`:
  - `split_round_trip`: write a project with 3 parts, a contact, a section and a joint; the five kinds of file exist; `read` returns equal data.
  - `schema_1_opens_and_migrates_on_save`: a folder with only an old `faber.json` reads; after `write` the split files exist, `faber.json` has `"schema": 2` and no `parts` key.
  - `move_writes_only_placement_and_manifest`: change one placement; the returned paths are exactly `placement.json` and `faber.json`.
  - `removed_part_file_is_deleted`.
  - `empty_assembly_and_analysis_files_are_absent`.
  - `bad_part_file_names_the_file`: a syntax error in `parts/lid.json` gives an error that contains `parts/lid.json` and the line and column.
  - `part_file_without_placement_gets_identity` and `placement_of_unknown_part_is_refused` (message names the id).
  - `part_ids_are_safe_file_names`: an id with `/` or `..` is refused at write (ids already pass `valid_id`; assert it).
  - `fingerprint_changes_after_a_part_edit`.
  - In `crates/faber-wiring/tests/files.rs`: `failed_change_leaves_every_file_unchanged` (hash all layout files before and after a refused edit) and `undo_restores_the_split_files`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-files -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement `layout.rs`** and switch `FileStore` to it. Sorted keys and two-space pretty JSON with a final newline, as today. Each file is written with the present `atomic_write`.
- [ ] **Step 4: Update the readers of `faber.json`.** Viewer change detection uses `fingerprint`. The file picker still accepts `faber.json`. Tests that compare the bytes of `faber.json` to prove "unchanged after an error" compare the fingerprint or all layout files. Tests that write a schema 1 file as a fixture stay as they are (they prove the migration path).
- [ ] **Step 5: Add `faber migrate <dir>`** (reads and writes the layout; prints the changed files). Run it on each folder in `examples/` and keep the results.
- [ ] **Step 6: Run** `cargo test --offline -j 4 --workspace --all-targets -- --test-threads=1`. Expected: pass.
- [ ] **Step 7: Update the documents** named above. State the write order and the recovery rule: the history snapshot of the last revision is written before the layout files; a mixed state after a crash is read as an external edit.

### Task 8: Jsonnet design source

Copy the pattern of `lamina-build`. No path dependency on a Lamina crate.

**Files:**
- Create: `crates/faber-ports/src/source.rs`, `crates/faber-build/` (`Cargo.toml`, `src/lib.rs`, `src/port.rs`, `src/faber.libsonnet`), `vendor/jrsonnet-stdlib/` (copy of `lamina/vendor/jrsonnet-stdlib`, with its licence file)
- Modify: root `Cargo.toml` (workspace member, the jrsonnet dependencies and the `[patch]` entry exactly as in `lamina/Cargo.toml` lines 47 to 52), `crates/faber-wiring/src/lib.rs`, `crates/faber-core/src/lib.rs` (refuse edits of generated data), `src/server.rs` (`build` tool, `open` and `status` report the source), `src/main.rs` (`faber build <dir>`), `docs/modelling-guide.md`, `README.md`
- Create: `examples/modular-rack/design.jsonnet` with `lib/bracket.libsonnet`
- Test: `crates/faber-build/tests/build.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
// crates/faber-ports/src/source.rs  (same shape as lamina-ports/src/source.rs)
pub struct Report { pub source: String, pub written: Vec<String>, pub parts: usize,
    pub added_parts: Vec<String>, pub removed_parts: Vec<String>,
    pub warnings: Vec<String>, pub inputs: Vec<String> }
pub trait DesignSource: Send + Sync {
    fn file_name(&self) -> &str;                       // "design.jsonnet"
    fn has_source(&self, root: &Path) -> bool;
    fn ensure_built(&self, root: &Path) -> Result<Option<Report>>;
    fn build(&self, root: &Path) -> Result<Report>;
    fn template(&self, name: &str) -> String;
}
```

- `design.jsonnet` evaluates to `{ name?, params?, parts: { <id>: {name, colour, group?, note?, geometry} } }`.
- The source owns `faber.json` (name and params) and `parts/*.json`. It never writes `placement.json`, `assembly.json` or `analysis.json`. A removed part loses its placement; the report lists it.
- The build record is `.faber/build.json`: the hash of each input file. `ensure_built` builds only when an input changed.
- With a source present, the tools that change parts, features or parameters return: `This project is generated from design.jsonnet. Edit the source, then call build.` Placement, contact, section and motion tools work as before.
- `faber.libsonnet` (imported as `import 'faber.libsonnet'`): `part(name, features, output, colour=[..])`, `feature(id, op, fields)`, `box`, `cylinder`, `union`, `difference`, `pattern` helpers, `merge(fragments)` that refuses duplicate part ids, and `prefix(fragment, text)` that prefixes part ids for a reused block.
- Imports resolve from the project folder, then from `lib/` in the project folder.

- [ ] **Step 1: Write failing tests** in `crates/faber-build/tests/build.rs`:
  - `builds_parts_and_params`: a source with one parameter and two parts writes `faber.json` and two part files; a second `ensure_built` returns `None`.
  - `a_changed_import_rebuilds`.
  - `function_block_used_twice_gives_two_parts` (uses `prefix` and `merge`).
  - `duplicate_part_id_is_refused_with_the_ids`.
  - `jsonnet_error_names_file_and_line`.
  - `placement_survives_a_rebuild` and `removed_part_loses_its_placement`.
  - `invalid_recipe_is_refused_and_files_stay`: a part with an unknown op; no file changes.
  - In `tests/mcp.rs`: `generated_project_refuses_feature_edits_and_allows_moves`, `build_tool_reports_the_changes`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-build -- --test-threads=1`. Expected: fail (crate absent). If cargo cannot resolve jrsonnet offline, copy the dependency lines and the vendor folder exactly as Lamina has them and try again; report the result if it still fails.
- [ ] **Step 3: Implement** the crate, the port, the wiring and the tools. The generated data passes the same validation and build as a tool change (a recipe that does not build is refused and no file changes).
- [ ] **Step 4: Write the example** `examples/modular-rack` (a bracket block used three times with different parameters) and document the method in the guide (one section, with the example source).
- [ ] **Step 5: Run** `cargo test --offline -j 4 -p faber-build -- --test-threads=1` and `cargo test --offline -j 4 --test mcp -- --test-threads=1`. Expected: pass.

### Task 10: Multiple selection and group move in the viewer

Owner request of 2026-11-18: select several parts with the mouse and move them
together, as Fusion does. Runs in round 3 with Task 4 (different files).

**Files:**
- Modify: `crates/faber-viewer/src/app.rs` (selection state, picking, browser list), `crates/faber-viewer/src/app/move_tool.rs` (draft for several parts)
- Modify: `crates/faber-core/src/placement.rs` (one edit list for several parts), `src/server.rs` (`move_part` accepts `ids`)
- Test: viewer tests with `egui_kittest`, `crates/faber-wiring/tests/batch.rs`, `tests/mcp.rs`

**Interfaces:**
- Selection is a sorted set of part ids with one primary part (the last one clicked).
- Click selects one part. Ctrl + click adds or removes a part. Shift + click in the browser list selects a range. A left drag on empty space draws a rectangle: left to right selects the parts fully inside; right to left selects the parts that the rectangle touches. Ctrl + A selects all visible parts. Escape clears the selection.
- Move/Copy acts on the whole selection: one triad at the centre of the common bounds (pivot choices as now), one rigid motion for all parts, one preview, one revision, one undo step. Copy makes one copy of each part with the id suffix `-copy`; a purchased part in the selection refuses the copy and names the part.
- Delete, Hide, Isolate and "Exact placement" (relative fields only) act on the whole selection.
- `faber_core::move_edits(data, ids, motion) -> Result<Vec<Edit>>`; MCP `move_part` accepts `ids: [..]` in place of `id` and returns one entry in `changed` for each part.
- The status line shows `<n> parts selected`.

- [ ] **Step 1: Write failing tests:** `ctrl_click_adds_and_removes`, `rectangle_left_to_right_selects_inside_only`, `rectangle_right_to_left_selects_touched`, `shift_click_selects_a_range_in_the_browser`, `group_move_keeps_the_relative_positions` (two parts, translate 10 mm and turn 90 degrees about the common centre: the distance between their centres is unchanged, both saved in one revision), `group_move_is_one_undo_step`, `group_copy_with_a_purchased_part_is_refused`, `escape_clears_the_selection`, `mcp.rs::move_part_with_ids_moves_all`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** A group rotation turns each part about the common pivot: new placement = motion * old placement.
- [ ] **Step 4: Run** the viewer tests, `cargo test --offline -j 4 -p faber-wiring --test batch -- --test-threads=1`, `cargo test --offline -j 4 --test mcp --test context_size -- --test-threads=1`. Expected: pass.

### Task 9: Close (coordinator)

- [ ] Run `cargo fmt --all`, workspace Clippy and all tests in sequence.
- [ ] Repeat the timing table of the performance review on `examples/trash-bin` and record before and after values in `docs/specs/faber-progress.md`.
- [ ] Build release, make the installer (`packaging/windows.ps1`) and install it.
- [ ] Commit (model changes in their own commit) and push.

## Order and file ownership

| Round | Tasks in parallel | Shared file risk |
| --- | --- | --- |
| 1 | 1, 2, 5, 7 | Task 7 edits many tests; Tasks 2 and 7 both touch `faber-wiring/tests` (different files) |
| 2 | 3, 6 | Task 3 owns `faber-core/src/lib.rs` build code; Task 6 owns `src/server.rs` |
| 3 | 4, 8 | Task 4 edits `src/server.rs` (two tools); Task 8 adds one tool |
