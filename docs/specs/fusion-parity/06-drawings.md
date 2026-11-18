# Drawings: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a 2D drawing of a design: views with hidden lines, section
views, dimensions that follow the model, a title block and a parts list.
Export SVG, PDF, DXF and PNG.

**Architecture:** A drawing is project data in `drawings/<id>.json`. It holds
views and dimensions that refer to the model, not to copied numbers. A new
kernel call projects the edges of a shape for one view direction. A new pure
Rust crate `faber-drawing` lays out the sheet and makes a `VectorDoc`
(chunk 01). `faber-files` writes it.

**Tech Stack:** Rust 2024, OCCT 7.9.3 (`HLRBRep_Algo`), `tiny-skia` (PNG
preview), `quick-xml` (tests).

**Spec:** `docs/specs/fusion-parity/00-roadmap.md` (decision D4; shared rules).

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Needs chunk 01 (vector documents).
- Task 1 changes `faber-model`. Task 2 changes `bridge.cpp`.
- A dimension stores a reference to model geometry. Its number is measured at
  each render. A stored number is never the source.
- Sheet units are millimetres. Sheet sizes: A4 (297 x 210), A3 (420 x 297),
  A2 (594 x 420), A1 (841 x 594), landscape or portrait.
- A mesh-only part shows its outline only. The reply says so in `limits`.

## Review Focus

- A dimension whose reference no longer exists after a model change is drawn
  as broken and reported; it does not stop the drawing (Task 4:
  `lost_reference_is_reported`).
- A view larger than the sheet at its scale is reported with the size and the
  sheet size (Task 3: `view_larger_than_the_sheet_is_reported`).
- First-angle and third-angle projection place the views on opposite sides
  (Task 3: `projection_angle_places_the_top_view`).
- Text with `<`, `&` or non-ASCII letters is correct in SVG, PDF and DXF
  (Task 5: `special_text_is_correct_in_each_format`).
- A part with a turn in its placement is drawn as placed (Task 2:
  `view_uses_the_placement`).

---

### Task 1: Drawing data

**Files:**
- Modify: `crates/faber-model/src/lib.rs` (or new `crates/faber-model/src/drawing.rs`), `crates/faber-files/src/layout.rs` (`drawings/<id>.json`)
- Test: `crates/faber-model` unit tests, `crates/faber-files/tests/layout.rs`

**Interfaces:**

```rust
pub struct Drawing { pub title: String, pub sheet: Sheet, pub projection: Projection /* first | third */, pub scale: f64,
    pub views: BTreeMap<String, View>, pub dimensions: BTreeMap<String, Dimension>, pub notes: Vec<Note>,
    pub title_block: BTreeMap<String, String>, pub parts_list: Option<PartsList> }
pub struct View { pub at: [f64; 2], pub scale: Option<f64>, pub hidden_lines: bool, pub label: Option<String>, pub kind: ViewKind }
pub enum ViewKind { Base { parts: Vec<String> /* empty = all */, direction: Direction },
                    Projected { from: String, side: Side /* left right top bottom */ },
                    Section { from: String, line: [[f64; 2]; 2] /* in the view of `from`, model mm */ },
                    Detail { from: String, centre: [f64; 2], radius: f64 } }
pub enum Direction { Front, Back, Left, Right, Top, Bottom, Iso, Custom { view: [f64; 3], up: [f64; 3] } }
pub struct Dimension { pub view: String, pub offset: f64, pub text: Option<String>, pub kind: DimensionKind }
pub enum DimensionKind { Linear { a: Anchor, b: Anchor, along: Along /* horizontal | vertical | aligned */ },
                         Diameter { edge: Anchor }, Radius { edge: Anchor }, Angle { a: Anchor, b: Anchor } }
pub enum Anchor { Vertex { part: String, near: [f64; 3] }, Edge { part: String, edge: EdgeSelector },
                  CylinderAxis { part: String, face: FaceSelector } }
```

- `Edit::SetDrawing { id, drawing }`, `Edit::RemoveDrawing { id }`. `ProjectData.drawings`.

- [ ] **Step 1: Write failing tests:** JSON round trip of a drawing with one of each view and dimension kind; `drawing_files_round_trip` and `removed_drawing_file_is_deleted` in the layout tests; the fingerprint changes after a drawing edit.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-model -p faber-files -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Edge projection

**Files:**
- Modify: `crates/faber-ports/src/lib.rs`, `crates/faber-kernel-occt/bridge.cpp`, `bridge.h`, `src/lib.rs`
- Test: `crates/faber-kernel-occt/tests/projection.rs`

**Interfaces:**

```rust
pub struct ViewFrame { pub origin: [f64; 3], pub x: [f64; 3], pub y: [f64; 3] } // the view looks along -(x cross y)
pub struct Projection2 { pub visible: Vec<Curve2>, pub hidden: Vec<Curve2>, pub bounds: [[f64; 2]; 2] }
pub enum Curve2 { Line([[f64; 2]; 2]), Circle { centre: [f64; 2], radius: f64 },
                  Arc { centre: [f64; 2], radius: f64, start_deg: f64, end_deg: f64 }, Polyline(Vec<[f64; 2]>) }
fn project_edges(&self, _shapes: &[Shape], _frame: &ViewFrame, _hidden: bool) -> Result<Projection2>;
fn section_face(&self, _shape: &Shape, _origin: [f64; 3], _normal: [f64; 3]) -> Result<Shape>; // planar face(s) of the cut
```

- A curve that is not a line or a circle is a polyline with a chord error of 0.01 mm.

- [ ] **Step 1: Write failing tests.**
  - `box_front_view`: box 40 x 20 x 10, front: the visible bounds are 40 x 10; 4 visible lines; no hidden line.
  - `hole_is_a_circle_from_the_top`: a through hole of diameter 10: one visible `Circle` of radius 5 in the top view.
  - `hole_is_two_hidden_lines_from_the_front`.
  - `view_uses_the_placement`: the box turned 90 degrees about Z has front bounds 20 x 10.
  - `two_parts_hide_each_other`: a box in front of a cylinder hides its lines.
  - `section_face_of_a_tube`: a tube cut through its axis gives 2 rectangles with the wall area.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-kernel-occt --test projection -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Views and sheet layout

**Files:**
- Create: `crates/faber-drawing/Cargo.toml`, `src/lib.rs`, `src/view.rs`, `src/sheet.rs`, `src/hatch.rs`
- Test: `crates/faber-drawing/tests/views.rs`

**Interfaces:**

```rust
pub trait ModelSource { // faber-wiring implements it with the project and the kernel
    fn project(&self, parts: &[String], frame: &ViewFrame, hidden: bool) -> Result<Projection2>;
    fn section(&self, parts: &[String], origin: [f64; 3], normal: [f64; 3]) -> Result<Vec<Vec<[f64; 2]>>>; // loops in the plane
    fn resolve(&self, anchor: &Anchor) -> Result<Resolved>; // point, or circle with centre, axis and radius
    fn bounds(&self, parts: &[String]) -> Result<[[f64; 3]; 2]>;
}
pub struct Rendered { pub doc: VectorDoc, pub problems: Vec<String>, pub limits: Vec<String> }
pub fn render(drawing: &Drawing, model: &dyn ModelSource) -> Result<Rendered>;
pub fn frame(direction: &Direction) -> ViewFrame;
```

- Layers: `VISIBLE` (0.5 mm), `HIDDEN` (0.25 mm, dashed), `SECTION` (0.7 mm), `HATCH` (0.18 mm), `DIMENSION` (0.25 mm), `FRAME`, `TEXT`.
- A projected view takes its direction from its parent and the projection angle. It is aligned with the parent.
- A section view shows the cut loops with a 45 degree hatch at 3 mm spacing and the visible edges behind the plane. The parent view gets the cutting line with arrows and letters.
- A detail view is the parent content inside the circle, at its own scale.
- The sheet has a border and a title block at the lower right with the fields of `title_block`, the scale and the projection symbol.

- [ ] **Step 1: Write failing tests** (with a test `ModelSource` that returns fixed data):
  - `base_view_is_scaled_and_placed`: a 40 x 10 projection at scale 2 at (100, 100) has sheet bounds 80 x 20 with centre (100, 100).
  - `projection_angle_places_the_top_view`: with `third` the top view is above the front view; with `first` it is below.
  - `projected_view_is_aligned`: equal X centre for top and front.
  - `section_view_has_a_hatch`: hatch lines are inside the loops, 45 degrees, 3 mm apart.
  - `view_larger_than_the_sheet_is_reported`: `View front is 400 x 100 mm at scale 2. The sheet is 297 x 210 mm.`
  - `title_block_has_the_fields`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-drawing --test views -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 4: Dimensions and automatic drawing

**Files:**
- Create: `crates/faber-drawing/src/dimension.rs`, `crates/faber-drawing/src/auto.rs`
- Test: `crates/faber-drawing/tests/dimensions.rs`

**Interfaces:**

```rust
pub fn auto_drawing(model: &dyn ModelSource, parts: &[String], sheet: Sheet, title: &str) -> Result<Drawing>;
pub fn format_length(mm: f64) -> String; // 40 -> "40", 12.5 -> "12.5", 3.14159 -> "3.14"
```

- A linear dimension has two extension lines, a dimension line with arrows at `offset` from the geometry, and centred text. A diameter text starts with the diameter sign; a radius text with `R`.
- `auto_drawing`: front, top and right views and an isometric view; a scale from the list 10, 5, 2, 1, 0.5, 0.2, 0.1 so that all views fit; overall width, height and depth; a diameter for each cylinder face that is a circle in a view, with equal diameters counted once (`4x` prefix); the position of each hole from the lower left corner of the view.

- [ ] **Step 1: Write failing tests.**
  - `linear_dimension_shows_the_measured_value`: anchors 40 mm apart give the text `40`.
  - `dimension_follows_the_model`: the same drawing with a model where the distance is 50 gives the text `50`.
  - `diameter_of_a_hole`: text is the diameter sign and `10`.
  - `four_equal_holes_are_one_dimension`: text starts with `4x`.
  - `lost_reference_is_reported`: a `resolve` error gives a red dimension with the text `?`, and `problems` has `Dimension d3: the edge reference was not found.`
  - `auto_drawing_fits_the_sheet`: a 300 mm part on A4 gets scale 0.5 and no problem.
  - `text_override_is_used`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-drawing --test dimensions -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: PDF and PNG writers, tools

**Files:**
- Create: `crates/faber-files/src/pdf.rs`, `crates/faber-wiring/src/drawing.rs`
- Modify: `crates/faber-files/src/vector.rs` (`.pdf`, `.png`), `crates/faber-wiring/src/lib.rs`, `crates/faber-core/src/batch.rs`, `src/server.rs`, `src/main.rs` (`faber drawing PROJECT ID FILE`)
- Test: `crates/faber-files/tests/vector.rs`, `crates/faber-wiring/tests/drawing.rs`, `tests/mcp.rs`

- PDF: version 1.4, one page, MediaBox in points (1 mm = 72 / 25.4 pt), lines and Bezier arcs, the standard Helvetica font with WinAnsi encoding. A letter outside WinAnsi is drawn as outlines through the `skrifa` font code of the sketch text feature.
- PNG: raster of the `VectorDoc` with `tiny-skia` at a given width in pixels.
- MCP tools: `set_drawing {id, drawing}`, `remove_drawing {id}`, `auto_drawing {id, parts?, sheet?, title?}` (saves a drawing made by `auto_drawing`), `export_drawing {id, path, width_px?}` (format from the extension; the reply has `problems` and `limits`; a PNG is inline only with `inline: true`), `drawings` (list).
- The parts list of a drawing: item, quantity, name, material, mass. Equal resolved geometry counts as one item.

- [ ] **Step 1: Write failing tests.**
  - `pdf_page_size_is_a4`: the file starts with `%PDF-1.4` and has `/MediaBox [0 0 841.89 595.28]`.
  - `pdf_has_one_stroke_for_each_polyline`.
  - `special_text_is_correct_in_each_format`: the title `Bracket <A&B> 10 µm` is valid in SVG, present in the PDF content stream or as outlines, and in DXF.
  - `drawing.rs::auto_drawing_of_the_trash_bin_example`: SVG export has four views and at least three dimensions and no problem.
  - `drawing.rs::drawing_follows_a_parameter_change`: change a width parameter; the exported SVG has the new number.
  - `mcp.rs::export_drawing_reports_problems`.
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests and `cargo test --offline -j 4 --test context_size -- --test-threads=1`. Expected: pass.

### Task 6: Viewer preview, guide, example

**Files:**
- Create: `crates/faber-viewer/src/app/drawing.rs`
- Modify: `crates/faber-viewer/src/app.rs`, `docs/modelling-guide.md` (section `## Drawings`), `crates/faber-build/src/faber.libsonnet`
- Create: `examples/trash-bin/drawings/overview.json` (made by `auto_drawing`)

- The viewer has a "Drawings" list in the browser. A click opens a read-only sheet view (the `VectorDoc` painted with egui shapes), with zoom, pan and "Export".

- [ ] **Step 1: Write failing viewer tests:** `drawing_tab_shows_the_sheet`, `drawing_updates_after_a_model_change`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-viewer drawing -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.** The guide section shows `auto_drawing`, one hand-made dimension and the export.
- [ ] **Step 4: Run.** Expected: pass.
