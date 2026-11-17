# Faber progress

Current state: work resumed on 2026-11-17. The owner set a new priority: an AI
must design parts through MCP fast and with few errors. Print output (3MF, print
profile, slicer) is postponed by the owner, not removed. The application and
active phases 1 to 10 are not complete. Phase 6 remains deferred.

## MCP evaluation round 1

Method: an AI modeller builds a part through stdio MCP only and reports each
failed call and workaround. Fixers then correct the reported gaps.

- The modeller built an improved bin: tapered ruled loft, R12 corners, cavity
  with R8 floor round, top lip, grip slots, vent hole and feet. Volume
  884 850 mm3. It found five gaps. All five are fixed:
  - A full-turn `circular_pattern` gave one copy. Angle ±360 now divides the
    circle into equal steps. Other angles keep the step rule.
  - The `bottom` render was empty. The opaque floor grid hid the part. The
    shader now discards the grid when the camera is below it.
  - Planar B-spline faces and straight B-spline edges had no normal or
    direction. The adapter now tests them by samples to 1e-7 mm.
  - No tool changed part metadata. `set_part_info` changes name, colour, group
    or note without a rebuild. It also works in `batch`.
  - The section side was not documented. The guide now states the kept side.
- Added `mass_properties`: volume, area, centre of mass, and mass and inertia
  from a density.
- Added the `faber-std` crate: ISO 261, ISO 273, ISO 4762 and ISO 10642 metric
  tables. It is not connected to a tool yet.
- Batch errors name the operation number, the tool and the field. A type error
  inside a nested `put_part` field does not name the field yet.
- Format, workspace Clippy and 179 sequential tests pass. Logs:
  `.cargo/round1-fmt.txt`, `.cargo/round1-clippy.txt`, `.cargo/round1-tests.txt`.
  No commit was made.

## MCP evaluation round 2

Four AI modellers built a bin, an enclosure with a lid, an L bracket and a jar
with a threaded cap. They made 49, 60, 42 and 77 calls, with 5, 6, 5 and 9
failed calls. The common causes and the fixes:

- Speed. Each process rebuilt all features. A persistent feature cache now
  stores exact shapes as opaque bytes in `.faber/cache`. The key hashes the
  resolved inputs, the input keys and the kernel version. Bad entries are
  rebuilt. Booleans and distance queries use parallel kernel modes. Debug
  build timings:

  | Operation | Before | After |
  |---|---|---|
  | Open, jar and cap, warm cache | 5.8 s | 0.45 s |
  | Open, enclosure, warm cache | 2.4 s | 0.28 s |
  | Parameter change, enclosure | 2.4 s | 0.73 s |
  | Parameter change, jar and cap | 4.2 s | 1.03 s |
  | Measure, jar and cap | 7.8 s | 1.31 s |

  The cache has no size limit yet. Parallel results are checked by the test
  suite only, not by a comparison with serial results.
- Contact. Touching parts gave an interference failure at a depth of 1e-15 mm.
  A pair with a depth and a common volume below 1e-6 is now contact. The new
  `set_contact` tool declares an intended contact. The export refusal names
  each pair. Model change: `ProjectData.contacts`.
- Selectors. A count error lists the matched items and the nearest rejected
  items with the filter that rejected them. New filters: `in_box` and
  `of_face`. `topology` accepts `kind`, a selector and `limit`. Model change:
  the new selector fields.
- Expressions. Added `pi`, `asin`, `acos`, `atan` and `atan2` in degrees. An
  unknown function error lists all functions. A parameter named `pi` is refused.
- Format, workspace Clippy and 209 sequential tests pass. Logs:
  `.cargo/round2-*.txt`. No commit was made.

## MCP evaluation round 3 and section analysis

- Shell accepts `thickness_overrides` per face. A bin gets a 4 mm floor and
  2.4 mm walls in one feature. A shell failure names the feature and gives
  the order rule. Model change: shell overrides.
- Input errors give a JSON path, for example
  `operations[1].feature.geometry.faces`. Build failures name the part, the
  feature and the operation kind. The path mapper is in `src/input.rs`.
- The guide is rewritten for AI readers: quick start, common mistakes, a
  frames and rotations table, and a checked thread example. `open` refuses a
  path that is not absolute and returns the resolved path.
- Sketch loops: `rounded_rectangle`, and `corner_radius` on polygons. The
  contact check uses an explicit empty intersection result, not a text match.
  Model change: the new loop and field.
- Section analysis, as in Fusion. `set_section` and `remove_section` save
  named sections from a plane, a reference plane or a planar face.
  `section_info` gives the exact cut area, perimeter, regions, holes and an
  approximate minimum wall. `render` accepts a section id. The viewer has an
  Analysis folder with show, flip and delete. Cut faces have a hatch in the
  part colour. Model change: `ProjectData.sections` and two edits.
- An independent review found five defects. All are fixed:
  - The cache key omitted the feature id. Equal features could restore the
    wrong face history.
  - The cache version did not follow code changes. It now includes the OCCT
    version and source hashes.
  - A declared contact allowed any overlap. It now has `max_depth_mm`,
    default 0.6 mm, confirmed by the exact kernel.
  - The cache had no size limit. The limit is now 512 MiB.
  - The clash package reader could overflow on bad input.
  The contact fixer also found that aligned overlapping boxes were reported
  as touching. Containment now runs for every pair.
- OCCT uses at most six threads.
- Known debt: `faber-clash` reads and writes files, although it is a domain
  crate. Read-only commands write cache entries.
- Format, workspace Clippy and 279 sequential tests pass. Logs:
  `.cargo/round3-*.txt`. No commit was made.

## Final trash bin example

`examples/trash-bin` holds the reference bin. An AI modeller built it only
through stdio MCP. The transcript is `mcp-transcript.jsonl`.

- One part with 22 features and 26 parameters: ruled tapered loft, shell
  with 2.4 mm walls and a 4 mm floor override, R10 inner and R12 outer
  corners, R8 floor round, rim lip, two grip slots, a vent hole and six feet.
- Volume 886 585 mm3. Mass 842.3 g at 0.95 g/cm3. Bounds 308.3 x 271.8 x
  345 mm. The lowest point is at Z = 0.
- Saved sections `centre` and `floor`. The floor cut gives one ring with a
  2.403 mm minimum wall. The hand value is 2.4 / cos 3 deg = 2.403 mm.
- Height 300 builds; height 340 gives the same volume again.
- The R8 floor round needs the R10 inner corner round first. A fillet
  before the shell fails in the kernel; the shell comes first.
- The viewer opened the result at the owner's request. The bin does not fit
  the A1 bed. Print output is postponed by the owner.
- The owner asked for hexagonal openings. Each wall now has a honeycomb of
  23 hexagonal cells, 22 mm across flats with a 5 mm web, 138 cells in all.
  Solid bands stay at the top and the bottom. Volume 747 570 mm3, mass
  710.2 g. Cell size, web, bands and margin are parameters. Grid counts are
  integers, so the row count does not follow a height change.
- The viewer showed each saved MCP step live while the cells were added.

## Logo and icon

- The mark is an F solid in isometric view. The front face uses the accent
  colour, the top faces a light tone and the sides a dark tone. A drilled
  hole is in the top arm. Files: `assets/logo.svg`, `assets/icon.svg` and
  `assets/icon.ico` (16 to 256 px).
- The viewer header, the start screen and the window icon draw the mark.
  Both programs embed the icon on Windows. The README shows the logo.
- Format, workspace Clippy and 281 sequential tests pass. Logs:
  `.cargo/round4-*.txt`. No commit was made.

## Sketch hole grids

- Added a grid loop with an item, two counts and two step vectors. The Rust
  adapter expands translated hole profiles before wire construction. Dimensions
  and steps accept parameter expressions. Steps can form an oblique grid.
- Expansion limits the sketch to 256 holes and 16 nested array levels. Zero
  counts, excessive copies, zero repeated steps and array outer boundaries
  are refused. Dedicated GUI controls and circular arrays remain open.
- Tests check all current loop and segment types, known volumes, parameter
  changes, unchanged source files on failure and reopen. The first test failed
  because the grid variant was unsupported.
- Format, workspace Clippy and all-target tests pass. Tests ran in sequence.
  Logs: `.cargo/sketch-arrays-clippy.txt` and `.cargo/sketch-arrays-tests.txt`.
  Model changes remain separate for a future commit. No commit was made.

## Closed spline control in the viewer

- The Spline tool has a Closed spline option. It switches between a periodic
  curve and a straight closing edge. Both modes use the saved profile geometry
  for their background preview.
- The preview key includes the closed state. A result for the previous state
  is not shown as current while its replacement is built.
- A headless GUI test draws four points, waits for the open preview, enables
  Closed spline, waits for the new preview and saves. Reopen checks one periodic
  segment and the resulting solid. The test first failed on the missing control.
- Format, workspace Clippy and all-target tests pass with sequential tests.
  Logs: `.cargo/periodic-gui-clippy.txt` and `.cargo/periodic-gui-tests.txt`.
  No window was opened and no commit was made.

## Periodic splines

- Sketch and spatial spline segments accept `periodic: true`. A periodic spline
  is the only segment in its path. It closes at the start without a closing line.
  Repeated start points and end-tangent constraints are refused.
- Tests check a closed single-edge profile, extrusion, scaling from expressions,
  refused input without file changes, reopen, spatial interpolation points and
  matching tangent directions on both sides of the join.
- The first tests failed because the field was unsupported. A seam test first
  assumed an axis-aligned tangent; it now measures both actual directions.
- Format, workspace Clippy and all-target tests pass with sequential tests.
  Logs: `.cargo/periodic-spline-clippy.txt` and `.cargo/periodic-spline-tests.txt`.
  Model type changes remain separate for a future commit. No commit was made.
  GUI periodic drawing controls remain open.

## Spline end tangents

- Sketch and spatial spline segments accept optional start and end tangent
  vectors. Components accept expressions. The kernel scales vector lengths;
  the supplied vectors set directions, not derivative magnitudes.
- Tests measure both spatial end directions with nearby probes, change sketch
  tangent expressions and check the resulting solid, refuse zero and out-of-range
  vectors, refuse unknown parameters without file changes and reopen the result.
- Existing point-only splines remain valid. The viewer sketch tool leaves
  tangents unset. Periodic splines and a dedicated tangent drawing tool remain open.
- The first test failed because tangents were unsupported. Format, workspace
  Clippy and all-target tests now pass. Tests ran in sequence.
  Logs: `.cargo/spline-tangents-clippy.txt` and `.cargo/spline-tangents-tests.txt`.
  Model type changes remain separate for a future commit. No commit was made.

## Loft and sweep face origins

- Loft and sweep from sketches assign new faces to the solid feature. This
  includes inner faces made from profile holes. Sketch-item origins remain open.
- Tests build tubes from annular sections, select and thicken the top face by
  creator, change height, check face ids, area-derived volume and position,
  refuse the sketch as creator without a file change and reopen the result.
- The new test failed with unavailable history first. Format, workspace Clippy
  and all-target tests now pass. Tests ran in sequence.
  Logs: `.cargo/loft-sweep-history-clippy.txt` and `.cargo/loft-sweep-history-tests.txt`.
  Face roles and other design requirements remain open. No window was opened
  or commit made.

## Split face origins

- Split records face history and filters the links to the solids kept in the
  result. Retained faces keep source ids. The cut face receives the split id.
- Tests cover positive, negative and both sides, cut-plane height changes,
  source face counts, cut area and position, a dependent thicken operation,
  wrong creator refusal, unchanged files on failure and file reopen.
- The unavailable-history check now uses a core kernel test without history
  support. It no longer depends on an operation awaiting implementation.
- The new split test failed with unavailable history first. Format, workspace
  Clippy and all-target tests now pass. Tests ran in sequence.
  Logs: `.cargo/split-history-clippy.txt` and `.cargo/split-history-tests.txt`.
  Full history requirements remain open. No window was opened or commit made.

## Thicken face origins

- Thicken retains the selected source face id. New side and offset faces receive
  the thicken feature id. History uses the solid after its orientation is fixed.
- Tests cover positive and negative thickness, a second thicken operation selected
  by creator and normal, parameter changes, face counts, volume and position,
  refused creator changes, unchanged files on failure and file reopen.
- The new test first failed with unavailable history. Format, workspace Clippy
  and all-target tests now pass. Tests ran in sequence.
  Logs: `.cargo/thicken-history-clippy.txt` and `.cargo/thicken-history-tests.txt`.
- The unavailable-history test now uses split. Full operation history remains
  open. No window was opened or commit made.

## Offset face origins

- Offset retains source ids through recorded modified and generated face links.
  New round transitions receive the offset feature id.
- Tests check inward and outward offsets, dependent top-face thickness and
  position, source and new face counts, wrong creator refusal, unchanged files
  on failure and file reopen. The test failed with unavailable history first.
- The unavailable-history test now uses thicken, which remains unsupported for
  face history. It still checks the explicit error and unchanged source file.
- Format, workspace Clippy and all-target tests pass with sequential tests.
  Logs: `.cargo/offset-history-clippy.txt` and `.cargo/offset-history-tests.txt`.
  No window was opened or commit made. Full operation history remains open.

## Shell face origins

- Shell records unchanged and modified face links. New inner faces receive the
  shell feature id; the outer bottom retains the source box id.
- The test selects and thickens the inner floor, changes wall thickness, checks
  floor area and creator ids, refuses an incorrect creator without a file change
  and reopens the result. It failed with unavailable history before the change.
- Format, workspace Clippy and all-target tests pass with sequential tests.
  Logs: `.cargo/shell-history-clippy.txt` and `.cargo/shell-history-tests.txt`.
  Other operation history remains open. No window was opened or commit made.

## Draft face origins

- Draft preserves source feature ids for changed faces and their neighbours.
  Its generated-face links describe the changed source faces. The adapter
  retains these links for draft; other operations keep their existing rules.
- The test first failed because history was unavailable. It then found that
  modified-face links alone omitted the two sloped faces. The final test checks
  all six face origins, both angle directions, a dependent cap, refused creator
  changes, unchanged source files on failure and file reopen.
- Format, workspace Clippy and all-target tests pass. Tests ran in sequence.
  Logs: `.cargo/draft-history-clippy.txt` and `.cargo/draft-history-tests.txt`.
  Full operation history remains open. No window was opened or commit made.

## Fillet and chamfer face origins

- Fillet and chamfer now use recorded face links. Unchanged and modified faces
  retain their source feature id. New faces receive the dress feature id.
- Tests select and thicken a modified top face by its source id, change the box
  width, check the new face area and creator, refuse an incorrect creator without
  changing the file and reopen the result.
- The new test failed with unavailable history before the adapter change.
  It passes with the change. Format, Clippy and all-target workspace tests pass.
  Logs: `.cargo/dress-history-clippy.txt` and `.cargo/dress-history-tests.txt`.
- Other dress-up operations, face roles and sketch-item origins remain open.
  No window was opened and no commit was made.

## Extrude and revolve face origins

- Extrude and revolve assign new faces to the solid feature. A selector can use
  this feature id with normal, centre and area filters. Sketch-item origins
  remain separate work.
- A regression test found that both planar caps of a revolved ring reported
  the same normal. Face normals now use surface derivatives and face orientation.
  This preserves the actual surface parameter direction.
- Tests check bottom normals, top-face selection and thickness, height changes,
  incorrect creator refusal, unchanged files on failure and file reopen.
- Format, workspace Clippy and all-target tests pass. Tests ran in sequence.
  Logs: `.cargo/generated-faces-clippy.txt` and `.cargo/generated-faces-tests.txt`.
  No window was opened and no commit was made. Full operation history remains open.

Current stopping boundary: complete the active work in phases 1 to 10, then
stop before phase 11. Phase 6 remains deferred by the owner.

For current scope and remaining requirements, see
[implementation audit](implementation-audit.md). The entries below are a work
record; earlier next-task statements can be superseded by later entries.

## Initial inspection

- The repository has a design and a Rust binary scaffold.
- The implementation plan and progress file were absent. They now exist.
- Cargo is set to at most six jobs.
- Lamina requires failing tests first in its parity plan. Its progress
  record also records failing regression tests.
- Section analysis acceptance checks are in phase 9.
- Phases 1 to 11 are not complete. The initial bin path is described below.

## Next task

Clashrs source was inspected through GitHub with `gh`. Its README records
3 to 8 seconds for 58 parts, 3.6 million triangles and 44 poses, compared
with 535 seconds for geometry booleans. This measurement has not been
repeated here. Phase 7 now includes reuse and tests for its query limits.

Start phase 1 with geometry port contracts and known-value tests. Inspect
the local kernel toolchain before the first long build.

## Assembly scan package

- Added `faber-clash` as a workspace crate with local source code. There is
  no dependency on a separate clash package or repository.
- Added `faber scan <directory> [--static-only] [--threads N] [--out FILE]`.
- Kept the MIT licence and copyright notice with the crate.
- Source revision: `c2fd66cb9c69e1b01dcd69243530f0bb2af21368`.
- Each scan uses its own worker pool, with at most six threads.
- CLI tests and the worker pool test failed before the changes and passed
  after them. Gap and contact tests also passed.
- `cargo fmt --all`, offline workspace Clippy with warnings refused, and
  offline workspace tests passed: five tests and one documentation test.
- This is the scan package integration only. It reads the existing mesh
  package format. Faber model conversion, strict input validation, schema
  changes, full containment tests and exact volume checks remain in phase 7.
- No performance benchmark was run. No commit was made.

## First CAD path

- The owner confirmed a plain, open-top hexagonal bin. No Yoink robot
  parts, frame or other mechanism. The viewer must open only at the end.
- Added model, ports, core, geometry, file, scene, render, wiring and viewer
  crates. Core operations use ports. No dependency on the Lamina repository.
- Added polygon extrusion and blind pockets, placement, bounds and volume.
  Both cap and wall construction use the same normalised profile points.
- Added atomic project saves, a writer lock, failed-build rollback and
  portable part ids. A second MCP open on the same path reuses the project.
- Added stdio MCP tools and a Rust client that builds the example through
  real tool calls. The bin is in `examples/hex-bin/faber.json`. Its transcript
  is in `examples/hex-bin/mcp-transcript.jsonl`.
- Added shared GPU rendering and section caps with holes. The native viewer
  has orbit, pan, zoom, selection, visibility, properties and live updates.
- Twenty tests passed, including one documentation test. The tests include
  the real MCP process and the actual viewer without a window.
- Format and workspace Clippy with warnings refused passed.
- Independent review found three issues: collinear cap boundaries, output
  names that differ only by case, and repeated open on the same project.
  Tests reproduced each issue and passed after the fixes.
- The first recorded bin build took 4.3 ms through MCP. This is one small
  example, not evidence for the full CAD performance targets.
- Exterior and section images were inspected. The floor is closed, the top
  is open, and the section leaves the cavity open.

## Remaining CAD work

The owner restated that Faber is a CAD program in the Fusion class. The bin
path is a first integration check, not the completed product. Continue the
main plan: exact solid handles, shape history, parameters, sketches,
booleans, dress-up operations, curved solids, exact exchange and the other
planned tools. Do not mark the active goal complete for the bin path alone.

The official OCCT 7.9.3 combined Windows SDK was downloaded to `.deps/`.
Its SHA-256 matches the published release digest. The compiler is already
installed. See `kernel-windows-research.md`.

## Exact solid adapter

- Added owned shape handles, measured properties and shape operations to
  the geometry port. The adapter catches exceptions at the C ABI boundary.
- Added exact boxes, cylinders, placement, union, difference, intersection,
  profile extrusion, blind pockets, tessellation and STEP exchange.
- Project builds now retain exact solids and use cached meshes for views.
- The full offline workspace test suite and Clippy passed. A further test
  passed for the hexagonal pocket: STEP write and read preserve its volume,
  bounds and single valid solid.
- Rebuilt the plain bin through stdio MCP. Its material volume is 952.88
  cm3 and its capacity is 18.92 litres. STL and STEP outputs are in
  `examples/hex-bin/out`. The MCP transcript records the calls.
- Inspected the actual viewer section image without a visible window.
  Corrected the texture colour conversion and verified it with a pixel test.
- Feature history, parameters, sketches and the remaining main-plan work
  are still required. The bin does not complete the CAD program.

## Parameter expression engine

- Added the independent Rust `faber-expr` crate. It has no dependencies.
- Added arithmetic, right-associative powers, names, brackets and the
  functions in the design. Trigonometric arguments are in degrees.
- Added sorted parameter evaluation with dependency reuse and cycle errors.
  Invalid names, unknown names and non-finite results are refused. Input
  length and recursion depth are limited.
- Four tests pass after the failing tests were recorded. Workspace Clippy
  passes with warnings refused. No model schema changed in this step.
- Expressions are not yet connected to part fields or MCP parameters.
  Function arguments currently evaluate before the function, including `if`.

## Parameter integration

- Added number-or-expression fields for profiles, height, floor and placement.
  Shared parameters are stored in the project. Numeric recipes still load.
- Added `params` and `set_param` MCP tools. Expressions remain in saved recipes.
- Tests verify changed solid volume, unchanged saved data after a failed
  parameter edit, and reuse of solids whose calculated dimensions do not change.
- The stdio MCP test sets a parameter and uses it to build an exact solid.
- The bin client now uses parameter expressions for all profile coordinates,
  height and floor. Local parameter scopes, limits and feature history remain.

## Ordered solid features

- Added ordered feature histories with named inputs and an output feature.
  Operations include box, cylinder, extrusion, pocket, transform, union,
  difference and intersection. Invalid references and duplicate ids fail
  before save. Each feature result must be a valid solid.
- Added a cache of exact intermediate shapes. A change rebuilds the changed
  feature and its dependants. Unchanged branches reuse their shapes. The
  cache is updated only after a successful project save.
- Tests cover volume changes, reopening, failed-change rollback and kernel
  call counts. The full offline workspace suite and Clippy pass.
- Added feature and parameter lists to the viewer. A headless interface
  test checks that the lists appear. These lists do not yet edit features.
- The bin client now builds outer and cavity extrusions, moves the cavity,
  and subtracts it. The real stdio MCP run, volume check and exports pass.
- The current history selects one output per part. Multiple output bodies,
  feature editing tools, topology references and dress-up operations remain.

## Feature editing tools

- Added core operations and MCP tools to add, replace, delete and move
  individual features. Insertion can select the new feature as the output.
- Added `part_info` to read the complete saved recipe and measured result.
- Changes use the same build-before-save transaction and feature cache.
  Deletion and movement fail if they break a required reference.
- Local tests cover ordered edits, exact volume, reopening and unchanged
  saved data after refused edits. The real stdio MCP test exercises all
  four edit tools and reads the resulting recipe.
- GUI feature editing, suppression, topology selectors and the remaining
  main-plan operations are still required.

## Curved solids

- Added exact sphere, cone, torus and profile revolve operations through
  the geometry port and the exception boundary.
- Added matching recipe variants. Each dimension, axis component and
  angle accepts a number or parameter expression.
- Tests compare primitive volumes with analytic values and check full,
  half and quarter revolutions. Tests also cover invalid dimensions and
  axes, tessellation, recipe loading and reopening.
- Profiles currently contain straight boundary segments. Sketch curves,
  loft, sweep, dress-up operations and topology selectors remain.

## Loft

- Added exact lofts through placed polygon profiles. The adapter supports
  ruled and smooth sides, with matching holes subtracted through the loft.
- Added the loft recipe and expression mapping for profiles and placements.
- Tests compare solid and hollow square transitions with analytic volume.
  A recipe test changes section height and verifies the rebuilt volume.
- The current sections contain straight edges. Guide curves, curved sketch
  edges, sweep and dress-up operations remain in the main plan.

## Sweep

- Added exact sweeps along paths of straight segments. Profiles can have
  holes. Placed profiles start normal to the first path segment; corners
  use mitred joins. Invalid path segments fail before the kernel call.
- Added the sweep recipe with expressions for the profile, placement and
  path. Tests verify solid and hollow volumes on straight and corner paths.
- A transaction test changes path length and refuses a zero-length segment
  without changing the project. Curved paths and sketch curves remain.

## Edge selection and dress-up

- Added exact edge queries and fillet and chamfer operations. Kernel edge
  indices are used only within a build and are not stored in recipes.
- Added geometric edge selectors with parallel direction, centre proximity,
  length and circular radius filters. Expected count and tolerance are
  required. Tests verify selection after parameter changes and refusal of
  an incorrect count without a file change.
- Added fillet and chamfer history features. Tests compare exact volumes
  and verify that failed operations leave the source shape valid.
- A failed dress-up operation tries at most five smaller values. The error
  reports the largest tested working value; no replacement is saved.
- Face provenance, role filters, face selectors, shell, draft and other
  main-plan operations remain. The current selector covers edges only.

## Face selection and shell

- Added exact face area, centre and planar normal queries. Added geometric
  face selectors with expected count checks and expression fields.
- Added inward shell operations with selected faces removed. Shell uses
  the same failure hint and transaction rules as fillet and chamfer.
- Tests verify box surface area, the upper face direction, open-box volume
  and shell rebuilding after a height change.
- Added the `topology` MCP tool for placed part measurements. Query indices
  are valid only for the returned revision; recipes still store selectors.
- Provenance, role filters, curved-surface selectors, draft, offsets and
  the remaining main-plan operations are not complete.

## Separate sketches

- Added sketch features with a placed plane and a closed outer loop with
  holes. Circles remain exact curves; rectangles and polygons use lines.
- Added extrusion from an earlier sketch with a world-space vector. The
  history accepts valid sketch faces as intermediate results and requires
  a valid solid for the final part output.
- Tests verify a circular tube, radius changes, invalid hole containment,
  and extrusion from a rotated sketch plane.
- The bin client now uses separate outer and cavity sketch features.
- Arc and spline sketch segments, multiple sketch regions, curve paths,
  sketch editing in the GUI and the remaining main-plan work are pending.

## Curved sketch paths

- Added closed sketch paths with line, three-point arc and cubic Bezier
  segments. Coordinates accept expressions. The kernel retains the curves.
- Tests compare half-circle and Bezier extrusion volumes with analytic
  values. Tests also verify holes and refusal of an open path.
- A test found inconsistent winding for a clockwise curved boundary with
  a hole. The adapter now normalises boundary direction before adding holes.
- General interpolated splines, 3D curve paths, multiple sketch regions
  and GUI sketch editing remain in the main plan.

## More sketch shapes

- Added exact ellipse loops and regular polygons defined by centre, corner
  radius, side count and rotation. Coordinates and dimensions accept expressions.
- Tests compare extruded ellipse and hexagon volumes with analytic values.
- The bin client now uses regular polygon sketches with six sides, instead
  of explicit vertex expressions. Its dimensions and simple form are unchanged.

## Solids from curved sketches

- Added revolve and loft operations that refer to earlier sketch features.
  These operations retain exact circles and other sketch curves.
- Sketch dependencies participate in the feature cache. A changed section
  placement rebuilds the dependent loft.
- Tests verify torus volume from a revolved circle, solid and hollow lofts
  between circles, parameter changes and refusal of mismatched holes.
- Sweep from curved sketches, 3D curve paths and the remaining main-plan
  operations are still required.

## Spatial curve paths

- Added 3D paths with line, arc and cubic Bezier segments. Curve features
  are intermediate wire results in the feature history.
- Added sweep from an earlier sketch and curve. The adapter checks that
  the sketch is normal to the start of the path and preserves holes.
- Tests verify quarter-torus volume on two planes, hollow sections,
  reopening and failed-change rollback for an invalid path segment.
- Helices, interpolated splines, curve editing and other main-plan tools
  remain incomplete.

## Persistent undo and redo

- Added journal records and immutable state snapshots in `.faber/history`.
  The journal is written before the atomic project replacement. A failed
  journal write leaves the model file and in-memory model unchanged.
- Added undo and redo to core and MCP. Targets rebuild before save. Each
  history move gets a new revision. New edits clear redo.
- Tests verify reopening, redo branches, empty history, external edits,
  failed journal writes and real stdio tool calls.
- Checked saves now compare the complete expected model, in addition to
  its revision.

## Named checkpoints

- Added checkpoint and restore to core, storage and MCP. Checkpoints persist
  across project sessions. Names are case insensitive; duplicates are refused.
- Restore builds and validates the target before save. It creates a new
  revision and supports undo. Invalid stored geometry leaves the model unchanged.
- Tests cover reopening, duplicate names, invalid paths, missing checkpoints,
  invalid geometry and checkpoint calls through a real stdio process.

## Atomic batches

- Added typed mixed-operation batches to core and MCP. Parameter, part and
  feature edits run in order on one candidate state. Only the final state builds.
- A successful batch creates one revision and one undo step. A failed edit or
  build leaves the project file, current state and history unchanged.
- Feature editing uses the same operations as batches. Tests cover dependent
  edits, failed builds, missing parts, undo, redo and real stdio calls.
- Batches do not include file output or history moves.

## Parameter editor

- Added a GUI parameter editor with expression fields and apply or cancel.
  A worker builds and saves the change while the view remains available.
- Failed edits retain the current model and the input fields. A stale editor
  refuses to overwrite a changed project. An active writer prevents GUI saves.
- Keyboard view shortcuts do not run while a text field has focus.
- Headless GUI tests open the editor, apply a change and check the saved file.
  Worker tests check changed solid volume, stale input and invalid dimensions.
- Direct sketch drawing and feature editing in the GUI remain incomplete.

## GUI history controls

- Added Undo and Redo buttons. A worker rebuilds the saved history target.
  Controls are disabled during a pending operation or parameter edit.
- Stale views refuse history changes until the current project loads.
- Headless GUI tests apply a parameter edit, undo it, redo it and check both
  the displayed data and the saved file. Worker tests cover stale views.

## GUI feature fields

- Feature rows now open an editor for existing dimensions, expressions and
  references. Apply builds in a worker through the core feature operation.
- Stale edits and invalid geometry are refused. Errors retain the input fields
  and the last valid model. Numeric fields accept parameter expressions.
- A headless GUI test types a new height, applies it, checks the new bounds and
  volume, then rebuilds from disk. It also checks rejection of a negative height.
- This editor does not yet add sketch segments, change operation types or edit
  Boolean settings. Direct sketch creation remains a separate task.

## GUI sketch creation

- Added New sketch with XY, XZ and YZ planes. Tools draw a rectangle from two
  corners, a circle from centre and radius, or a polygon from ordered vertices.
- The canvas has a 10 mm grid, 1 mm snapping, zoom, point removal and clear.
  Create solid accepts an extrusion expression and a body name.
- Saving creates separate sketch and extrusion features in one new part.
  The worker refuses stale input and never replaces an existing part.
- Tests check exact circle volume and rectangle orientation on all three planes.
  A headless GUI test clicks the canvas, creates a solid, checks its volume and
  rebuilds it from the saved recipe.
- Draft sketches currently save with their extrusion. Standalone sketch storage,
  constraints, face planes, multiple regions, holes and graphical vertex editing
  remain incomplete. This is not the complete sketch system in the main plan.

## Sketch dimension fields

- Rectangle width and height, and circle radius, accept numbers or parameter
  expressions during sketch creation. Empty fields retain the drawn dimensions.
- The preview evaluates these expressions against the opened project. The saved
  sketch retains the expressions so later parameter changes rebuild the solid.
- Tests check preview dimensions and rebuilt rectangle and circle volumes.
  These fields do not provide a general geometric constraint solver.

## Oblique section controls

- Added azimuth and tilt controls to the GUI section inspector. Base-plane
  buttons, offset and reversal remain available.
- Offset bounds use the unit plane normal. Angle tests check base and oblique
  normals. The headless GUI test changes both sliders and renders the cut.

## Conditional dimensions

- Expressions support `<`, `<=`, `>`, `>=`, `==` and `!=`. Comparisons run
  after arithmetic and return 1 or 0. Chained comparisons need brackets.
- Tests cover syntax, precedence and a conditional solid dimension that changes
  at a parameter threshold. Undo restores the previous solid.
- `if` calculates only its selected branch. Both branches are parsed, so bad
  syntax and incorrect function argument counts are still refused.
- Tests cover guarded division, nested conditions, skipped name lookup and
  active dependency cycles. Other functions evaluate all arguments.

## Mesh containment

- A regression test found that nested closed meshes passed when their surfaces
  did not intersect. Pair queries now also inspect contained vertices when the
  target mesh has oriented pseudo-normals.
- The test uses nested cubes and checks a non-zero penetration estimate.
  Depth remains approximate. Open or incorrectly oriented meshes do not provide
  a reliable solid-containment result. Edge crossings and input validation still
  need the remaining checks from phase 7.

## Scan input checks

- Scan rejects invalid proximity and depth values, duplicate or empty names,
  unknown rule parts, non-finite vertices and incomplete pose transforms.
- Pose matrices must contain finite values and an orthonormal rotation with
  positive determinant. Scale and reflection are refused before conversion.
- Regression tests first reproduced a negative-margin panic and acceptance of
  scaled matrices. Both now return errors before the scan workers start.

## Current-design interference

- Wiring converts placed CAD body meshes into oriented scan meshes with merged
  duplicate vertices. The core and model do not depend on the query adapter.
- Added read-only MCP interference with a proximity distance and revision-tagged
  report. Part ids identify findings. Depth is an approximate mesh measurement.
- Tests check nested bodies, separated placements, unchanged project data and
  calls through stdio. Persisted rules, strict save checks and GUI findings remain.

## GUI interference report

- Added a Checks window with a background interference scan and part selection
  from findings. It reports approximate depth and gap at 0.1 mm clearance.
- Results retain the complete source design. A changed design displays a stale
  report notice instead of the old findings.
- Headless GUI tests run the check and verify stale-result handling.
  Persisted rules and strict save checks remain incomplete.

## Strict batches

- Added a domain check port and a mesh-check adapter in wiring. Core compares
  error identities before and after a strict batch, before writing history.
- MCP batch accepts strict: true. A new interference or clearance finding
  refuses the change. Existing error identities do not block unrelated edits.
- Tests verify unchanged files, empty history and unchanged bodies after refusal.
- The current adapter uses 0.1 mm clearance and approximate mesh checks.
  Per-project rules and strict flags on individual edit tools remain incomplete.

## Strict MCP edits

- Added optional strict flags to set_param, put_part, put_parts, remove_part and
  feature add, edit, delete and move tools. Strict calls use the same checked
  batch path; the default remains unchanged.
- A real stdio test verifies that an enclosed part is refused with a strict
  error and that the saved file does not change. GUI strict controls remain.

## Strict GUI edits

- Added an optional check-error refusal setting in the Checks window.
  Sketch creation, parameter edits and feature edits capture this setting before
  starting their worker and use the checked core transaction when enabled.
- A worker test verifies refusal of overlapping geometry and unchanged saved
  data. Undo and redo retain their existing history behaviour.

## Checked exports

- MCP and CLI exports now run the current mesh checks before file output.
  Findings refuse export unless force is explicitly enabled.
- Tests verify refusal before output directory creation, forced file output and
  the same behaviour through real stdio calls. Checks use the current fixed
  0.1 mm clearance threshold; project-specific rules remain incomplete.

## GUI export

- Added an export window with target directory, STEP or STL and an explicit
  check override. The worker uses the same checked export as MCP and CLI.
- Exports retain the captured revision and all complete bodies, regardless of
  visibility or section settings. The result identifies the exported revision.
- The headless GUI test writes an actual STEP file from the export controls.

## CLI export format

- Added `--format step` and `--format stl` to the export command. STL remains
  the default. Both formats use the checked export path.
- A process test first failed on the missing option, then passed with actual
  STEP and STL output. Invalid and incomplete options refuse output.

## Transformed profiles

- Fixed history validation for placed sketches and curves. A rigid placement
  must retain the source face, edge and solid counts. Intermediate faces and
  curves can feed later features. The final part must still be a valid solid.
- A sweep test first failed on a placed sketch, then passed after the fix.
  It checks volume, bounds, parameter changes, reopening and rollback when a
  face or curve is selected as the final output.
- A further test checks two successive placements, rotation and a profile hole
  before extrusion.

## Linear solid patterns

- Added a linear pattern feature with a source, total count and XYZ step.
  Count and step accept parameter expressions. Overlapping copies unite.
- Tests cover separate copies, overlapping copies, parameter changes, reopening,
  a single copy and rollback for invalid counts or a zero step.
- The shared feature schema exposes the operation through MCP. A stdio test
  builds three copies and checks their total volume. Existing GUI feature forms
  can edit its fields. A GUI action to create patterns remains incomplete.
- Model type changes are in faber-model and must form a separate later commit.
  No commit was made.

## GUI linear patterns

- Added Linear pattern to the part panel. The form starts with two copies and
  accepts expressions for count and XYZ step. Step uses the part axes.
- Apply appends a feature and sets it as the output. A simple part is first
  wrapped in a history. Placement and other part fields stay unchanged.
- Work runs off the interface thread, checks the captured project state and
  honours strict mode. Failures keep the draft and saved data.
- A headless GUI test clicks the controls and checks the saved volume. A worker
  test covers primitive parts, rollback, stale views and undo.

## Circular solid patterns

- Added circular patterns with total count, axis origin, axis direction and
  angle between copies. All number fields accept expressions. Coordinates use
  the part frame; angles use degrees and the right-hand rule.
- Tests check a shifted oblique axis, volume, bounds, negative angles, parameter
  changes, reopening and refusal of a zero axis or zero angle for multiple copies.
- Added Circular pattern to the viewer part panel. A headless GUI test creates
  the feature and checks the saved volume. A stdio test edits a linear pattern
  into a circular pattern and checks volume and bounds.
- Model changes remain uncommitted and require a separate later commit.

## Solid mirror

- Added a mirror port and its adapter. The Rust adapter computes reflection
  across a plane from its origin and normal. Core can unite the source and copy.
- Tests check an oblique offset plane, volume, bounds, parameter changes,
  reopening, keeping the source and rollback for a zero normal.
- Added Mirror to the viewer. Feature forms now expose Boolean fields as
  checkboxes. A headless GUI test creates the mirror and checks saved volume.
- A real stdio call checks mirror volume and bounds. Model changes remain
  separate for a later commit; no commit was made.

## Imported model storage: first step

- Added an asset storage port and a file adapter. Files are copied without
  changes to `assets/<sha256>`. Writes publish a complete file without replacing
  an existing asset. Reads check the content hash before returning a path.
- Tests check byte preservation, duplicate imports, concurrent publication,
  removal of the original file, changed content, invalid keys and empty inputs.
- This is storage support only. Import recipes, project rebuilds, MCP and GUI
  import actions, and purchased-part records are not yet connected.

## Imported solid recipes and MCP

- Added an imported geometry recipe with a content key. Core resolves files
  through the asset port and reads shapes through the kernel port. No file
  format name was added to the model or core.
- Project open, sync and edits verify referenced content before reusing cached
  geometry. Both direct parts and imported features in a history are supported.
- Added import_part to core and MCP. It creates a new part, supports strict
  checks and preserves undo/redo. It refuses an existing part id. This command
  imports an owned model; purchased-part records and their restrictions remain.
- Tests check source removal, reopen, undo/redo, a pattern after import, corrupted
  assets, malformed files and an actual stdio import call.
- Failed geometry imports may leave an unreferenced content file. Saved design
  data stays unchanged. Asset cleanup and the GUI import action remain.

## GUI solid import

- Added Import STEP with a source path and part name. Relative paths use the
  project folder. The worker chooses a new body id and honours strict mode.
- The form checks the captured project state before import. Failed imports keep
  the form; successful imports close it and update the view.
- A headless GUI test types a real source path, imports a solid and reopens the
  saved project. Worker tests check stale state and strict interference refusal.
- A stdio test sends a malformed file, checks unchanged project data and then
  reads status through the same connection.

## GUI part placement

- Added Move / rotate to the part panel. Position and rotation accept project
  expressions. Rotation is about the part origin, in X, Y, Z order.
- The worker changes placement while preserving geometry and other part data.
  It checks the captured state and honours strict mode. Undo restores placement.
- A headless GUI test moves and rotates an imported solid, then checks its
  volume and bounds after reopening. Worker tests cover expressions, invalid
  input, strict interference, stale views and undo.

## GUI project changes

- Added New project and Open project in the project panel. Loading and initial
  creation run off the interface thread. Draft edits disable project changes.
- New project takes the writer lock and refuses an existing project file.
  Open project is read-only. A failed load retains the current design.
- A successful load resets selection, hidden parts, camera, section and stale
  check/export state, then watches the new project folder.
- Tests cover refusal to overwrite a project, missing paths and a headless GUI
  round trip from an existing design to a new project and back.
- The viewer still requires an initial project path on its command line.
  A start screen without a project is not yet implemented.

## Viewer start screen

- The viewer now starts without a project path. It shows New project and Open
  project, with no modelling tools until a project loads. It does not create an
  unnamed project or watch the current folder while on the start screen.
- A headless test renders the start screen, creates a project through its
  controls and checks that modelling tools become available.
- `faber-viewer shot --out FILE` captures the start screen without a window.

## Small-window layout

- A current bin capture showed that the parameter controls fell below the left
  panel. Added scrolling to both side panels.
- A 1000 by 650 headless test first reproduced the hidden parameter button and
  overlapping header controls. The header now uses two rows in narrow windows.
  Long project names are shortened in the header, with the full name on hover.
- The test scrolls to Edit parameters, opens the form and checks that Redo and
  Iso do not overlap.

## Crossings without interior vertices

- A regression test showed that crossing closed bars passed an allowed-contact
  rule because all tested vertices were outside and the depth estimate was zero.
- Added a separate triangle surface-crossing check. It requires both triangles
  to cross the other's plane and their plane-cut intervals to overlap.
- Crossings with no positive depth sample cannot pass contact or depth allowance
  rules. Reports expose surface_crossing; the viewer states that depth is not
  resolved instead of presenting zero as a measured depth.
- Tests cover crossing bars, coplanar contact, disjoint triangles and touching
  closed faces. Positive depth values remain estimates from vertices, not exact
  solid penetration depths. No performance improvement is claimed.

## GUI shell creation

- Added Shell to the part tools. The default removes one face with a positive
  Z normal in part coordinates and uses 1 mm inward thickness.
- The form edits the face selector and thickness, including expressions. Apply
  uses the existing background transaction, stale-state check and strict mode.
- A headless test applies Shell to a 10 by 20 by 30 mm box and checks the saved
  volume of 1824 mm3. Operation-specific help replaces the generic pattern text.
- Face selection by clicking the model is not yet available.

## Priority correction: AI-first CAD

- The owner clarified that Faber is an AI-first CAD application. The bin is an
  example used to test the application. Slicer settings, filament selection and
  printer operation are outside the task. Purchased parts remain deferred.
- Focus on fast, reliable modelling through stdio MCP and the shared GUI.

## Batch preview

- Extracted the shared build-and-check stage from saving. Preview reuses current
  bodies and feature caches but does not change them or write project/history
  files. Failed candidates are refused by the same checks as saved edits.
- Added preview_batch to MCP. It reports candidate bounds, volumes and the base
  revision. Batch and preview accept expected_revision to refuse stale requests.
- Tests cover candidate volume, unchanged data, strict refusal, unchanged undo
  order, an actual stdio preview and a refused stale batch.
- Preview accepts an optional render request, including a section plane. It
  writes and returns a PNG of the candidate without saving the design. The stdio
  test checks the image dimensions, returned bytes and unchanged project file.
- Preview does not retain candidate geometry for a later commit. Saving the
  candidate runs the shared build stage again.

## Current priority: AI-first modelling

- On 2026-11-17, the owner deferred purchased parts and assemblies. The pending
  tasks are recorded in [TODO](../TODO.md) and phase 6 of the plan.
- Focus current work on fast modelling through MCP and the shared GUI. Slicer
  settings and printer operation are outside the task. Assembly-specific checks,
  joints and pose work remain for later. Existing owned-model import stays.
- The example remains a plain hexagonal bin with no robot hardware.

## Edge tools in the viewer

- Added Fillet and Chamfer actions to the part tools. Both append an editable
  feature to the current history through the shared project worker.
- The form accepts an edge direction, expected count and a radius or distance.
  The default selects four straight edges parallel to the part Z axis. A count
  mismatch refuses the change. Direct edge selection in the view remains open.
- A headless interface test applies each tool to a box, reopens the saved model
  and checks its volume against a known value.

## Source feature queries

- The topology MCP tool accepts an optional feature id. It reads the cached
  source shape in part coordinates. Without a feature id it reads the final
  placed shape in world coordinates. Each response states the coordinate space.
- Queries do not rebuild or save the model. They check the current recipe before
  reading the cache, so a removed history cannot expose an old feature.
- Tests cover a placed part, changed parameters, file reopen, missing features,
  a removed history and the actual stdio protocol.

## Curved examples and tangent sections

- Added an ogive shell, a ring with four blind pockets and a tapered, twisted
  wing loft in examples/curved-parts. A Rust client builds the editable recipes
  through stdio MCP and records the calls. STEP, STL and images are included.
- The client shares its protocol code with the bin example. The integration
  test checks ring volume, wing volume scaling, shell volume range, exchange
  output and section caps in both directions.
- The nose exposed an open section boundary near its inner tip. Equal section
  segments were kept once instead of cancelled in pairs. A smaller test with a
  plane tangent to a retained box edge reproduced the failure. Pair cancellation
  fixes both cases and preserves the hollow and oblique section tests.
- Workspace format, Clippy and all-target tests passed. The example client ran
  successfully and both exterior and section images were inspected without a
  visible window. These examples do not complete all design acceptance cases.

## Schema and modelling guide

- Added the planned `faber schema` command. It prints the project schema from
  the model types and needs no open project. Extra arguments are refused.
- Added a CLI test for the schema, geometry operations, refusal of extra
  arguments and absence of file writes.
- Replaced the early inline guide with docs/modelling-guide.md. CLI and MCP use
  the same text. It explains batch edits, section previews, revision checks,
  source topology queries and the current feature set.
- Format, workspace Clippy and all-target offline tests passed.

## Slot sketches

- Added a parametric slot loop with centre, total length, width and rotation.
  The Rust adapter expands it to two lines and two exact arcs. Equal length
  and width produce a circle. No C++ change was needed.
- Slots work as outer boundaries and sketch holes. Tests check a rotated
  extrusion, known volume and bounds, two circular ends, parameter changes,
  file reopen and refusal of a length smaller than the width.
- The model type change is in faber-model for a separate future commit. No
  commit was made.
- The viewer sketch toolbar now creates slots from two bounding corners.
  Length and width accept parameter expressions. The preview shows round ends.
  The GUI test draws, saves, reopens and changes the length parameter, then
  checks the resulting volume. The initial slot axis follows the sketch X axis.

## Named render views

- MCP render and candidate render accept iso, front, back, left, right, top and
  bottom. The default remains iso. These views use perspective projection.
- Camera presets live in faber-scene. Viewer Top and Front use the same presets.
  Exact vertical views use a nonparallel up vector, so their matrices remain
  finite. Tests check all six directions and their centre rays.
- The stdio test requests a front candidate section, then top and front images
  of the saved model. It checks the returned view names and distinct PNG data.

## Part render targets

- Render accepts an optional part id and fits the camera to that part only.
  It borrows the existing body without copying its mesh. Candidate renders
  select from candidate bodies, including parts that have not been saved.
- The stdio test compares a selected-part image with an image of the same part
  alone. It also checks a candidate-only part, unchanged source and refusal of
  an unknown target before an output file is created.

## Exact minimum distance

- Added a distance port, a small native adapter call and Project::measure_distance.
  The MCP measure tool accepts two part ids and returns millimetres, closest
  points, coordinate space and revision. It queries placed shapes without a
  rebuild or save.
- The adapter uses single-thread distance evaluation under the existing kernel
  lock and checks returned values. The native exception boundary is retained.
- Tests cover a known diagonal gap, reversed arguments, contact, overlap,
  containment, missing ids, unchanged source and rotated sphere surfaces.
  The stdio test also checks the measured gap between two placed boxes.
- Zero distance does not report penetration depth. Exact face/edge measures,
  material density and inertia remain open requirements.

## Solid offset

- Added a parametric offset feature through the kernel port. Positive distance
  expands a solid with round joins; negative distance contracts it. Zero and
  results without a valid positive-volume solid are refused.
- Added Offset solid to the viewer. It uses the same history worker and feature
  editor as the other modelling tools. The name is distinct from section offset.
- Tests check the exact expanded box volume and bounds, inward offset, collapse
  refusal, unchanged files on failure, reopen and the GUI result volume.
- The model variant stays in faber-model for its own future commit. No commit
  was made. Selected-face offset and thicken remain separate requirements.

## Face draft

- Added a draft feature with a source, face selector, direction, neutral-plane
  origin and normal, and angle in degrees. It uses the kernel port and the
  existing native exception boundary. Invalid angles and vectors are refused.
- Added the Draft form to the GUI. Its default selects one face with normal +X
  and applies 5 degrees above the XY plane. Fields remain editable expressions.
- Tests check positive and negative angles on two selected faces, a moved
  neutral plane, invalid and collapsing angles, rollback, reopen and GUI volume.
- The GUI test waits for the long form layout to settle before it clicks Apply.
  The model variant remains separate for a future model-only commit.
- The full GUI test caught the parameter button moving below the visible panel
  after more tools were added. Part tools now wrap across rows, leaving more
  room for features and parameters. The original GUI flow passes again.

## MCP timing and GPU reuse

- Added examples/benchmark_mcp.rs. It measures real stdio operations on a
  temporary copy of the bin, verifies model results and writes raw samples.
- The initial run identified repeated GPU setup on each headless render.
  Headless saves now reuse one device and pipeline per process. A mutex protects
  the shared camera uniform through draw and readback.
- The concurrent render regression test checks that top/front camera images
  match their sequential references. Existing part-target pixel checks also pass.
- On this small debug-build fixture, median 640 x 480 section render time changed
  from 374.27 ms to 23.58 ms. First-render setup remains about 0.54 seconds.
  See [method and samples](../benchmarks/README.md). These figures are not a
  viewer frame-time or assembly performance result.

## Surface thickening

- Added thicken for sketch faces and for one selected face of a solid. The
  signed thickness follows or opposes the face normal. Source references and
  dimensions remain editable. The result becomes the part output.
- Added face extraction and thickening ports. The native adapter orients the
  closed result before it is measured; the initial planar result had negative
  signed volume despite valid topology. Zero thickness is refused.
- Tests cover sketch holes, both directions, bounds, rollback, reopen and a
  selected cylindrical face with known inner and outer radii. The GUI test
  checks the Thicken face action and resulting volume.
- Model changes remain separate for a future commit. No commit was made.

## Parametric holes

- Added one hole feature for plain, counterbored and countersunk holes. The entry
  and cut direction use placement; diameter, depth and head dimensions accept
  expressions. The counterbore and countersink forms are mutually exclusive.
- Core builds the cutter from existing cylinder/cone ports and applies one cut.
  No native adapter change was needed. A cut outside the source is refused.
- Tests check all three known removed volumes, parameter changes, invalid head
  dimensions, rollback, a missed cut and file reopen. Standard fastener sizes
  remain separate work.

## Hole creation in the viewer

- Added Hole to the part tools. The form offers plain, counterbore and
  countersink types, with editable entry position, rotation and dimensions.
- A type change preserves bore expressions and placement edits. Head dimensions
  start from the defaults for the selected type. Plain removes the head.
- Headless GUI tests create all three types, switch back to plain, retain
  expressions and verify the saved solid volume after reopening the file.
- Format, workspace Clippy and all-target tests pass. Logs are in
  `.cargo/hole-gui-clippy.txt` and `.cargo/hole-gui-tests.txt`. No window was opened.

## Plane split

- Added split with a source, parametric plane origin and normal, and a choice
  of both sides, positive side or negative side. Both sides remain separate
  solids within the same feature result. No new part records are made.
- Added a kernel port and a native plane splitter. Tangent and outside planes
  are refused. The adapter uses a non-destructive, single-threaded operation.
- Added Split body to the viewer with side controls and editable plane fields.
- Tests check volume, bounds, solid count, the shared cut face, oblique cuts,
  reversed normals, parameter changes, failure rollback and file reopen.
  A headless GUI test changes the plane, selects a side and checks saved volume.
- Format, workspace Clippy and all-target tests pass. Logs are in
  `.cargo/split-clippy.txt` and `.cargo/split-tests.txt`. No window was opened.
  Model type changes remain separate for a future commit. No commit was made.

## Edges between face sets

- Added between to edge selectors. Its two face selectors are resolved again
  at each build. An edge must belong to two distinct faces, one in each set.
  Existing direction, centre, length and radius filters still apply.
- Added an adjacency port and native topology query. The query returns unique
  edge-face pairs and does not depend on display mesh triangles.
- Tests check fillet and chamfer volumes, source dimension changes, multiple
  face matches, expressions inside nested selectors, disjoint faces, the same
  face on both sides, wrong counts, rollback and reopen.
- Format, workspace Clippy and all-target tests pass in sequence. Logs are in
  `.cargo/between-faces-clippy.txt` and `.cargo/between-faces-tests.txt`.
- This is adjacency selection, not operation provenance. Created-by, role and
  sketch-origin filters remain open. No commit was made.

## Face origins through transforms and booleans

- Added optional face source links to the kernel port. The native adapter records
  unchanged and modified faces for transforms, union, difference and intersection.
  Initial faces get their feature id. Core retains source feature names through
  the recorded links and caches them with the shape.
- Face selectors accept created_by. Topology queries report creator names or
  null when history is unavailable. Final part placement preserves known history.
  A selector that requires unavailable history is refused rather than guessed.
- Tests track a cylindrical hole wall through placement and cutting, change the
  source height, check union and intersection after part placement, reopen files
  and refuse incorrect creators. An unsupported offset-history test checks the
  explicit error and unchanged source file.
- This is the first history path. Roles, sketch-item origins and history for
  other operations remain open. Extrude and revolve are covered below. Dress-up
  history still needs work.
- Format, workspace Clippy and all-target tests pass with sequential tests.
  Logs: `.cargo/face-history-clippy.txt` and `.cargo/face-history-tests.txt`.
  A repeat bin timing run is stored in `docs/benchmarks/hex-bin-debug-face-history.json`.
  Model type changes remain separate for a future commit. No commit was made.

## Interpolated spline segments

- Added spline segments to 2D sketch paths and 3D curve paths. The curve passes
  through each supplied point. Coordinates accept expressions. Point counts,
  coordinate limits and consecutive equal points are checked before construction.
- Tests check a single spatial edge through the supplied points, sketch
  extrusion, path sweep, parameter changes, refused edits and file reopen.
- A spline sweep exposed an integration error in the reported volume. Shapes
  with spline or Bezier edges or surfaces now use adaptive Gauss integration.
  Analytic shapes retain the existing calculation. Gauss-Kronrod integration
  caused a severe slowdown in the wing fixture and is not used.
- The full suite passes, including ellipse volumes and the curved examples.
  MCP volume assertions use a tolerance for floating-point rounding.
  Logs: `.cargo/spline-clippy.txt` and `.cargo/spline-tests.txt`.
- A new bin benchmark records the current debug build. This is not a spline
  performance measurement. See `docs/benchmarks/hex-bin-debug-spline.json`.
- Periodic splines and specified end tangents remain open. Existing feature
  forms can edit spline point coordinates.

## Spline drawing in the viewer

- Added Spline to the sketch tools. Click at least three interpolation points.
  A straight edge joins the final point to the start to close the profile.
  Remove point and Clear sketch use the same controls as other sketch tools.
- The preview uses the same profile geometry as the saved solid. One background
  worker builds the face and its display mesh. Boundary lines are cached until
  points change. Results for old points are not shown as the current preview.
- The headless GUI test draws points, removes the last point, waits for the
  current preview and saves a solid. It checks the known parabolic profile
  volume after reopening the project.
- Format, workspace Clippy and all-target tests pass. Logs are in
  `.cargo/spline-gui-clippy.txt` and `.cargo/spline-gui-tests.txt`.
  No window was opened and no commit was made.

## Helix paths

- Added a helix curve feature with radius, signed pitch, turns and placement.
  It is a sweep path, not a solid. Turn count and total height are bounded.
- The adapter makes the cylindrical curve and builds its 3D representation to
  a 0.0000001 mm approximation tolerance. Other layers use the kernel port.
- Tests sweep a circular section in both pitch directions, check path length
  and solid volume, change turn count, refuse invalid dimensions and reopen.
- Full format, Clippy and workspace tests passed. GUI helix creation remains open.

## Named reference planes

- Added plane reference features and sketches attached by plane id. The local
  sketch placement is applied before the plane placement. Both accept expressions.
- Feature cache entries distinguish reference planes from shapes. Changes to a
  plane invalidate dependent sketches and solids. Plane values never reach a
  kernel operation that expects a shape.
- Tests check parameter changes, rotated planes with local sketch offsets,
  missing, forward and wrong-type references, invalid coordinates, refused plane
  output, unchanged files on failure and file reopen.
- Format, workspace Clippy and all-target tests pass. Logs are in
  `.cargo/reference-plane-clippy.txt` and `.cargo/reference-plane-tests.txt`.
- Reference axes, points, face-derived planes and GUI plane creation remain open.
  Model changes remain separate for a future commit. No commit was made.

## Reference axes, points and viewer forms

- Added axis and point reference features. Revolve around an axis and place at
  a point use typed dependencies. Parameter changes rebuild the dependent solids.
  Placement translates source coordinates by the point position; it does not
  align the solid centre.
- Tests check the ring volume when its axis moves, translated bounds, invalid
  directions and coordinates, wrong or missing references, refused reference
  outputs, file rollback and reopen.
- Added a References menu in the viewer. Plane, axis and point forms add a
  reference to the feature list and preserve the current solid output. GUI tests
  verify all three saved records and unchanged volume. Reference pickers for
  dependent operations and construction geometry display remain open.
- Three separate buttons pushed the parameter control below the visible area.
  The menu fixes this; the existing parameter and short-window tests pass.
- A parallel GUI test run was stopped after slow progress. The complete suite
  then passed with `--test-threads=1`. Use sequential GUI tests to reduce load.
  Format and workspace Clippy also pass. Logs: `.cargo/references-clippy.txt`
  and `.cargo/references-tests.txt`. No window was opened and no commit was made.

## Planes from faces

- Added plane_on_face with a source, face selector and signed offset. Exactly
  one planar face is required. Its area centre and outward normal define the
  plane. Local X follows projected part X, or part Y near parallel normals.
- Source changes invalidate the plane and its dependent sketches. This uses
  existing face queries and Rust frame calculations; no native code was added.
- Added Plane on face to the viewer References menu. The form edits the selector
  and offset and preserves the current solid output. Direct face picking remains
  separate work.
- Tests check top, bottom, side and oblique planes, source size changes, local
  sketch offsets, volume, bounds, multiple matches, curved faces, unchanged files
  after errors and reopen. The GUI test verifies the saved reference and output.
- Format, workspace Clippy and all-target tests pass, with tests run in sequence.
  Logs: `.cargo/face-plane-clippy.txt` and `.cargo/face-plane-tests.txt`.
  Model type changes remain separate for a future commit. No commit was made.
