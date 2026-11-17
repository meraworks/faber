# Implementation audit

Date: 2026-11-17. Sources: the design, faber-plan.md, current source files and
the latest all-target test log at `.cargo/sketch-arrays-tests.txt`.

This is a progress audit, not a completion claim. Passing tests prove the tested
paths. They do not prove every design requirement. No release is authorised.

The owner requested a pause after the sketch grid task for transfer to another
agent. See [handoff](agent-handoff.md). This does not mark any whole phase complete.

## Scope

The owner requires an AI-first CAD application, with fast model creation through
stdio MCP and a shared native GUI. The plain hexagonal bin is an example, not
the product boundary. Purchased parts and assemblies are deferred in
[TODO](../TODO.md). Slicer settings and printer operation are outside the current
task. Do not restore them as current blockers.

The owner now requires work to stop on reaching phase 11. Complete the active
requirements of phases 1 to 10, with phase 6 deferred, then stop before release
checks and packaging. This replaces the earlier finish-at-phase-11 boundary.

## Plan phases

| Phase | Evidence in the current tree | Remaining work |
|---|---|---|
| 1. Geometry | Opaque shapes and Kernel port; adapter and solids tests; face links for transforms and booleans | Full operation provenance; static versus dynamic distribution comparison |
| 2. Project | Model, expression tests, transaction tests, batch tests, undo tests, in-memory feature cache | Persistent shape/mesh cache; planned file split; part-local parameter limits |
| 3. Render and MCP | Actual stdio tests; shared GPU rendering; preview with section image; CLI schema | Labelled geometry, image id maps, full query surface and rich change reports |
| 4. References and features | Geometric and between-face selectors; created-by through transforms, booleans, solid construction, dress-up and split; holes, copies and references; GUI reference creation | Full face history, role and sketch-origin selectors; GUI reference pickers; standard-sized hole tools |
| 5. Curves | Sketches on named and face-derived planes, hole grids, arcs, Bezier, interpolated splines with end tangents, periodic splines, helix, revolve, loft, sweep; curved example client | GUI helix and grid creation; tangent drawing controls; circular sketch arrays |
| 6. Purchased parts and assemblies | Owned-model import and placement only | Deferred by owner; see TODO |
| 7. Checks | Local faber-clash crate, bounded worker pool, containment/contact/crossing tests, strict rollback | General check registry, data-driven rules, exclusions; original fixture benchmark not repeated |
| 8. Output | STL and STEP export, checked export, round-trip tests | Names/colours acceptance, state hash, 3MF/glTF; print preparation is outside current task |
| 9. Viewer | egui/wgpu, background workers, part tree, feature edits, sketches, section, headless GUI tests | Face/edge selection and history, geometry notes, changed-face display, frame/update measurements |
| 10. Standard tools | No faber-std crate or standards table | Standard records and hole tools remain absent; do not mix this with purchased models |
| 11. Release | Format, Clippy and all-target tests pass | Offline package install, licence bundle, performance record, command coverage; release not authorised |

## Design requirements

### Storage and transactions: sections 5.1 to 5.3 and 5.5

- Current project data lives in one `faber.json`. `faber-files` reads that file;
  parts do not yet live in separate files. Undo/checkpoints use `.faber/history`.
- Project data has schema and revision numbers and sorted maps. Source writes
  are atomic. Writer locks, failed builds and external edits have tests.
- Feature caches are in memory. `faber-wiring::read` rebuilds the project; it
  does not load persistent shapes. A new process does not meet the fast-cache
  requirement yet.
- Parameters are shared string expressions. Part-local parameters, notes on
  parameters and min/max limits are not in ProjectData or Part.
- Final part output is one history result, which can contain several solids.
  There are no separately named body records within a part.
- Slots, circles, rectangles, polygons, ellipses and paths exist. Sketches use
  placement transforms or named reference planes. Planar-face references use a
  face selector and rebuild when their source changes.

### References, queries and replies: sections 5.4, 5.6 and 5.7

- The kernel port exposes optional face source links. The adapter records
  unchanged and modified faces for transforms and booleans. Core carries feature
  origins through these links. Other operations can report history unavailable.
- Extrude and revolve assign new faces to their solid feature. Tests check
  selection after height changes, refused sketch origins and file reopen.
- Fillet and chamfer retain source origins for unchanged and modified faces.
  New rounded and bevelled faces carry the current feature id. Other dress-up
  operations still need history support.
- Draft keeps source ids for faces whose angle or boundary changes. Tests cover
  both angle directions and a dependent cap selected by its source id.
- Shell retains recorded source face links. New inner faces receive the shell
  feature id. Tests select the inner floor after wall thickness changes.
- Offset retains source face ids for inward and outward changes. New round
  transitions receive the offset id.
- Thicken retains the selected source face id and assigns its new faces to the
  thicken feature. Tests cover both thickness directions and dependent features.
- Split retains source ids for the kept faces and names the cut face with the
  split id. Tests cover both sides together and each side alone.
- Loft and sweep from sketches assign their new faces to the solid feature.
  Tests include holes, dependent face selection and section height changes.
- Selectors use direction, centre, area, length, radius and expected count.
  `created_by` filters faces when history is available. `between` filters edges
  with two face selectors and exact adjacency. Role, from and convexity are absent.
- Topology queries return edges and faces of a placed part or cached feature.
  They do not convert a one-build id into a persistent semantic selector.
- MCP uses generic typed recipes for feature operations. This is a permitted
  naming choice, but it does not replace absent operations.
- Bounds, volume, triangle counts and revision are reported. The complete
  before/after change report, violation delta, stored selectors, changed-region
  picture and build timing are absent. Nonstrict edits do not automatically
  return all current check violations.
- Measure now returns minimum distance and closest points between placed parts.
  Face/edge measures, mass/inertia/material, find, describe and image-pick tools
  remain absent. Suppression and named saved views are also absent.
- Schema is available through CLI and tool input schemas. There is no separate
  project-schema MCP tool yet.

### Rendering and GUI: sections 5.8 and 5.13

- The shared section function fills cut faces and keeps holes. Tests cover
  hollow and oblique cuts, reversal, an outside plane and tangent edges.
  Curved examples also test both directions. Original solids stay unchanged.
- The GUI has orbit, pan, zoom and view presets. MCP and preview renders now
  accept iso/front/back/left/right/top/bottom directions and can frame a single
  part. They cannot yet target a feature or add labels, highlight or measurements.
- Viewer selection identifies parts. It does not identify exact faces or edges.
  Part notes are plain fields, not geometry-attached review notes.
- Headless interface tests cover the editing flows. No recorded frame-time
  or file-to-display latency benchmark proves the performance target.
- The Hole form creates plain, counterbored and countersunk holes. Type changes
  preserve bore expressions and placement. Saved volumes have GUI tests.
- The sketch Spline tool has a background geometry preview and a straight
  closing edge or a periodic closed curve. GUI tests check point removal, preview,
  switching to a closed curve and saved geometry.
- The bin MCP benchmark now records status, edit, reopen and headless-render
  timings. GPU reuse reduces repeated image cost on that fixture. See
  [benchmark scope](../benchmarks/README.md); broader targets remain unproved.

### Checks, exchange and adapter: sections 5.9 to 5.12 and 5.14

- MeshChecks uses a fixed 0.1 mm proximity. It compares violation ids for strict
  changes. It is not the planned check registry with severity and rule records.
- Mesh intersection and vertex penetration estimates do not prove exact common
  volume. Unresolved surface crossings are refused instead of treated as contact.
- The original clash benchmark has not been repeated. The assembly pose target
  is deferred; no speed-up claim is supported by the current tests.
- STEP/STL geometry is tested. Full named/coloured exchange acceptance and an
  export project-state hash are not implemented.
- The adapter serialises calls and catches native exceptions. Pure Rust can
  replace port methods later, but the current exact kernel remains native.
- The repository contains the imported clash licence. A complete distribution
  licence bundle and an offline installation test are still required.

## Example acceptance: section 5.16

- The plain hexagonal bin has editable source, a stdio transcript, exact output
  and images in examples/hex-bin. It has no robot hardware.
- Ogive shell, pocketed ring and twisted wing are in examples/curved-parts.
  The integration test checks known ring volume, wing scaling, shell volume
  range, output and section caps. This does not prove aerodynamic performance.
- The desk clamp with the stated volume, bounds and face count is not present.
- The plate with 64 octagon and 49 round holes and its stated volume is not
  present. Do not substitute a different plate and claim that target passed.
- Purchased-servo gimbal acceptance is deferred with assemblies.

## Next work order

1. Named MCP view directions and single-part targets are implemented. Extend
   targets to features when face/edge queries and provenance are available.
2. Add measurements and the missing modelling operations, with known-value
   tests at the project and stdio boundaries.
3. Add kernel provenance and semantic selectors, then exact face/edge selection
   in the GUI. Do not claim persistent references from geometric filters alone.
4. Complete check reports, persistent caching and the remaining active example
   cases. Record representative build, query, frame and update timings.
5. Complete the active exchange requirements of phases 1 to 10. Keep deferred
   work separate. Stop before phase 11; installation packaging and release
   checks are beyond the owner's current stopping boundary.
