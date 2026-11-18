# Viewer usability: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the viewer behave like Fusion for windows, keys, selection,
camera and forms, so a person can work with the mouse without dead ends.

**Architecture:** All changes are in `crates/faber-viewer`. No kernel call, no
new project file and no `faber-model` change. Window state moves into one
small module so that every window closes, cancels and reports errors the same
way. Keys go through one dispatcher that knows the focus.

**Tech Stack:** Rust 2024, egui/eframe 0.37, `egui_kittest` for headless
tests, `faber-scene` (camera, picking).

**Spec:** `docs/specs/fusion-parity/viewer-usability-audit.md` (the defect
list with file and line) and the owner reports below. Shared rules:
`docs/specs/fusion-parity/00-roadmap.md`. Run this chunk first, before chunk 01:
each later chunk adds panels to the same viewer code.

Owner reports (2026-11-18):
- "The Move window opens but I cannot close it."
- "Many mouse controls of Fusion are missing or are not user friendly."
- The section handle sits at a strange place in a close view. A separate fix
  is in progress (`section_anchor`). Do not change that code in this chunk.

## Global Constraints

- All shared rules of `00-roadmap.md` apply (ASD-STE100, British spelling, no
  AI attribution, `--offline -j 4`, one cargo process, `--test-threads=1`, no
  window on the screen of the owner).
- No change to `faber-model`, `faber-ports`, `faber-core` or the kernel.
- Each test is a headless `egui_kittest` test or a unit test. Put new viewer
  tests in new files `src/app/<topic>_tests.rs`, declared in `src/app.rs` with
  `#[cfg(test)] mod <topic>_tests;` (as `suppress_tests.rs`).
- The mouse map stays as Task 10 of the review plan set it: left drag on empty
  space = rectangle select, middle drag = pan, Shift + middle or Alt + left =
  orbit, wheel = zoom.
- Fusion key map (decision U2): `Escape` cancels, `Enter` applies, `Delete`
  deletes the selection, `Ctrl+Z` undo, `Ctrl+Y` and `Ctrl+Shift+Z` redo, `F`
  fit (Faber), `M` move, `S` opens the command search. Section on and off moves
  to the header button and to `Shift+S`.
- A delete has no confirmation dialog (decision U3, as Fusion). It shows a
  notice "Deleted <names>. Ctrl+Z restores it." for 6 seconds.
- Escape order (decision U1), one level for each press:
  1. close an open popup, combo box or menu;
  2. stop an active pick step (Move "From"/"To", later face picks);
  3. cancel the open tool window (same result as its Cancel button);
  4. clear the selection.
- A shortcut never fires while a text field has the keyboard focus, except
  `Escape` and `Enter`. While a tool window is open, only `Escape`, `Enter`,
  `Ctrl+Z` and `Ctrl+Y` reach the app.

## Review Focus

- A window that is taller than the screen (768 px high viewport) still shows
  its OK and Cancel buttons (Task 1: `sketch_window_fits_a_768_pixel_screen`).
- An error of one window does not show in the next window after Cancel
  (Task 1: `cancel_clears_the_window_error`).
- `Delete` in a text field deletes text, not the selected part (Task 2:
  `delete_in_a_text_field_keeps_the_part`).
- `Ctrl+Z` after a group move restores all parts in one step (Task 2:
  `ctrl_z_undoes_a_group_move_in_one_step`).
- A rectangle that touches only the empty corner of a large part's bounding
  box does not select it (Task 3: `crossing_rectangle_uses_triangles_not_bounds`).

---

### Task 1: One behaviour for every window

**Files:**
- Create: `crates/faber-viewer/src/app/windows.rs`, `src/app/windows_tests.rs`
- Modify: `crates/faber-viewer/src/app.rs` (windows at the lines in the audit,
  section A), `src/app/move_tool.rs` (panel at line 975), `src/sketch.rs`

**Interfaces:**

```rust
pub(crate) enum ToolWindow { CreateProject, MoveRotate, Import, Export, Checks, Parameters, Feature, Sketch, MoveCopy }
pub(crate) struct WindowState { pub open: Option<ToolWindow>, pub error: Option<String>, pub dirty: bool }
impl WindowState {
    pub fn open(&mut self, w: ToolWindow);   // closes the previous one (asks first if dirty)
    pub fn cancel(&mut self);                // closes, clears error and draft values
    pub fn finish(&mut self);                // closes after a success, clears error
}
fn tool_window(ctx: &egui::Context, title: &str, state: &mut WindowState, body: impl FnOnce(&mut egui::Ui)) -> WindowEvent;
pub(crate) enum WindowEvent { None, Apply, Cancel }
```

- Every tool window uses `tool_window`: title bar X, `Escape` (decision U1),
  a Cancel button, OK or Apply on `Enter`. The body is in a `ScrollArea`. The
  window is constrained to the viewport rect and is movable.
- The global `self.error` stays only for the status bar. A window shows only
  `WindowState.error`.
- `dirty` is true after the first typed change. Opening another window or
  another feature form while `dirty` asks "Discard the changes in <title>?"
  with Discard and Keep. A background reload never closes a dirty window;
  it shows "The project changed on disk. Apply is disabled until you reopen
  the form." instead.
- Export and Checks become tool windows. A running export continues after
  close and reports in the status bar.
- The MOVE/COPY panel keeps its place but gets a scroll area and stays in the
  viewport.

- [ ] **Step 1: Write failing tests** in `windows_tests.rs`:
  - `every_window_closes_with_escape`: for each `ToolWindow`, open it, press `Escape` once, assert `open == None` and no error text in the UI.
  - `every_window_has_a_close_button`: for each window the title bar has a close button; a click closes it.
  - `move_rotate_window_closes`: open "Move / rotate part" (the owner report); X, Cancel and Escape each close it.
  - `cancel_clears_the_window_error`: make Import fail (a missing file), Cancel, open Parameters: no error label.
  - `sketch_window_fits_a_768_pixel_screen`: harness 1024 x 768; open Create sketch; the "Create solid" and "Cancel sketch" buttons are inside the window rect after a scroll and the window rect is inside the viewport.
  - `move_panel_buttons_stay_in_view`: harness 1024 x 600; open Move/Copy; OK and Cancel can be reached by scroll inside the viewport.
  - `switching_feature_asks_before_losing_values`: type into a feature form, click another feature: the discard question shows; Keep keeps the typed value.
  - `escape_in_a_pick_step_stops_only_the_pick`: Move/Copy in "To" pick; first Escape leaves the tool open with no pick; second Escape closes the tool.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer windows_tests -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command and then the full viewer tests. Expected: pass.

### Task 2: Keys and the command search

**Files:**
- Create: `crates/faber-viewer/src/app/keys.rs`, `src/app/command_search.rs`, `src/app/keys_tests.rs`
- Modify: `crates/faber-viewer/src/app.rs` (key code at 2917-2928), `src/app/selection.rs` (Escape at 114)

**Interfaces:**

```rust
pub(crate) enum Command { Undo, Redo, Delete, Fit, Move, Search, ToggleSection, Escape, Apply, SelectAll }
pub(crate) fn command_for(input: &egui::InputState, text_focus: bool, window_open: bool) -> Option<Command>;
pub(crate) struct CommandSearch { pub query: String, pub open: bool }
pub(crate) fn search_commands(query: &str) -> Vec<&'static CommandEntry>;   // name, shortcut, enabled
```

- `command_for` is the only place that reads shortcut keys. It applies the
  focus rule of the Global Constraints.
- `Delete` removes the selected parts in one revision (one `batch` of
  `remove_part`) and shows the notice of decision U3. A purchased part can be
  deleted (removal does not change the part).
- The command search (`S`) is a small popup at the cursor with a text box and
  a filtered list of every toolbar and menu command with its shortcut.
  `Enter` runs the first enabled entry.
- Undo and Redo buttons get tooltips "Undo (Ctrl+Z)" and "Redo (Ctrl+Y)".

- [ ] **Step 1: Write failing tests** in `keys_tests.rs`:
  - `ctrl_z_and_ctrl_y_undo_and_redo`: change a parameter; Ctrl+Z restores the revision value; Ctrl+Y applies it again; Ctrl+Shift+Z also redoes.
  - `ctrl_z_undoes_a_group_move_in_one_step`: move two selected parts as a group; one Ctrl+Z restores both placements.
  - `delete_removes_the_selection_in_one_revision`: select two parts; Delete; both are gone, the revision grows by one, the notice text shows.
  - `delete_in_a_text_field_keeps_the_part`: focus the parameter text field, press Delete: the part count is unchanged.
  - `shortcuts_do_not_fire_under_a_tool_window`: with the feature form open, F, M and S change nothing.
  - `s_opens_the_command_search_and_enter_runs_it`: S, type "fit", Enter: the camera equals the result of the Fit button.
  - `shift_s_toggles_the_section`.
  - Update `section_handle_stays_in_view_when_zoomed_in` and every other test that pressed `S` for the section to press `Shift+S`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer keys_tests -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the full viewer tests. Expected: pass.

### Task 3: Selection on the canvas

**Files:**
- Modify: `crates/faber-viewer/src/app/selection.rs`, `src/app.rs` (canvas input near 2616, context menu at 1682), `crates/faber-scene/src/lib.rs`
- Create: `crates/faber-viewer/src/app/selection_tests.rs`

**Interfaces:**

```rust
// faber-scene
pub fn mesh_in_rect(mesh: &Mesh, transform: glam::DMat4, view: glam::Mat4, rect: [[f32; 2]; 2], mode: RectMode) -> bool;
pub enum RectMode { Window /* every vertex inside */, Crossing /* any triangle overlaps */ }
```

- Shift + click on the canvas adds to the selection (Fusion), Ctrl + click
  toggles, as now in the tree.
- The rectangle uses `mesh_in_rect`, not the bounding box. Window mode is a
  left-to-right drag, crossing mode is right-to-left, as now.
- Hover highlight: the body under the cursor draws with a light outline or a
  tint when no drag runs. The hover pick runs at most once for each frame and
  only after the cursor moved.
- Right-click on empty space shows: Fit, Show all, New sketch, Paste
  placement (only if a placement was copied), Section on/off. Right-click on a
  part adds "Edit features" (opens the feature list of that part) to the
  existing part menu.
- The inspector hint text gives the real mouse map (one text, no
  contradiction).

- [ ] **Step 1: Write failing tests** in `selection_tests.rs`:
  - `shift_click_on_the_canvas_adds_to_the_selection`.
  - `crossing_rectangle_uses_triangles_not_bounds`: an L-shaped part; a right-to-left rectangle over the empty inner corner of its bounds selects nothing; one over the leg selects it.
  - `window_rectangle_needs_every_vertex_inside`.
  - `hover_marks_the_body_under_the_cursor`: move the pointer over a part: the state `hovered == Some(id)`; move it to empty space: `None`.
  - `empty_space_menu_has_fit_and_new_sketch`.
  - `part_menu_has_edit_features`.
  - Unit tests in `faber-scene` for `mesh_in_rect` with a unit cube and a known camera.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer selection_tests -- --test-threads=1` and `cargo test --offline -j 4 -p faber-scene -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both. Expected: pass.

### Task 4: Camera and view cube

**Files:**
- Create: `crates/faber-viewer/src/app/view_cube.rs`, `src/app/camera_tests.rs`
- Modify: `crates/faber-scene/src/lib.rs` (`Camera`, `View`), `crates/faber-viewer/src/app.rs` (header buttons at 330-360)

**Interfaces:**

```rust
// faber-scene
pub enum View { Front, Back, Left, Right, Top, Bottom, Iso, Corner([i8; 3]), Edge([i8; 3]) }
impl Camera { pub fn zoom_at(&mut self, aspect: f32, ndc: [f32; 2], factor: f32); }
```

- A view cube in the top right corner of the viewport: 6 faces, 12 edges and
  8 corners can be clicked; each gives its view direction. A home button
  above the cube gives Iso and fits.
- "Iso" gives a true isometric direction (1, -1, 1) and fits. It is no longer
  equal to Fit.
- Wheel zoom keeps the world point under the cursor fixed on the screen.
- A double click of the middle button fits.

- [ ] **Step 1: Write failing tests** in `camera_tests.rs` and in `faber-scene`:
  - `zoom_keeps_the_point_under_the_cursor`: unit test; the projected screen point of the picked world point moves less than 0.5 px after `zoom_at`.
  - `iso_is_not_fit`: the view direction after Iso is `normalize(1, -1, 1)` within 1e-6.
  - `cube_face_click_gives_the_face_view`: click the "Top" face of the cube: the view direction is -Z.
  - `cube_corner_click_gives_a_corner_view`.
  - `double_middle_click_fits`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Forms, parameters and feedback

**Files:**
- Modify: `crates/faber-viewer/src/feature_form.rs`, `src/app.rs` (parameters at 823 and 2068, tool fallback at 1873, file dialog at 420, header)
- Create: `crates/faber-viewer/src/app/forms_tests.rs`

- Each number field shows its unit (`mm`, `°`, `mm²`) after the box. An
  expression field shows its resolved value and unit below it.
- "Edit parameters" is always in the menu. The window can add a parameter
  (name and expression) and delete a parameter that no expression uses. A
  delete of a used parameter is refused with "Parameter <name> is used by
  <part>/<feature>."
- With no selection, tool buttons that need a part are disabled with the
  tooltip "Select a part first." There is no fallback to the first part.
- The header shows "Strict" when strict edits are on. A click turns it off.
- Native file dialogs run off the UI thread (`rfd::AsyncFileDialog`). The UI
  keeps drawing while the dialog is open.
- Every toolbar button has a tooltip with its shortcut, if any.

- [ ] **Step 1: Write failing tests** in `forms_tests.rs`:
  - `number_fields_show_units`: the fillet form shows "mm" after the radius field.
  - `parameters_can_be_added_in_an_empty_project`: a project with no parameters; Edit parameters, add `w = 40`, Apply: the project has `w`.
  - `used_parameter_cannot_be_deleted`: exact refusal text.
  - `tools_need_a_selection`: no selection; the Fillet button is disabled; a click does nothing.
  - `strict_mode_shows_in_the_header`.
  - `open_dialog_does_not_block_the_frame` (unit test of the async dialog wrapper with a fake dialog that resolves after two frames).
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer forms_tests -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the full viewer tests, workspace Clippy and all tests. Expected: pass.

## Close

- `docs/modelling-guide.md`: a section `## Viewer controls` with the mouse map,
  the key map and the Escape order.
- `docs/specs/faber-progress.md`: one section for this chunk.
- Release build, installer, install, merge and push as in `00-roadmap.md`.
