# Fusion parity roadmap

Status: written 2026-11-18. Not started. Run it after the plan
`docs/specs/2026-11-18-review-speed-modular-plan.md` is complete and pushed.

This file is the index and the specification for sixteen plans. Each plan is one
chunk. Each chunk gives working, tested software on its own. Run one chunk at a
time with `superpowers:subagent-driven-development`.

Evidence for the gaps: `docs/specs/2026-11-18-fusion-feature-gap.md` (Design
workspace, from the Fusion files). The other workspaces come from general
knowledge of Fusion. They are not verified against the Fusion files.

## Product goal

Faber is an alternative to Fusion for a person and an AI that design printable
and machinable parts. Each capability has a number that an AI can read, a
picture that a person can read, and a statement of its limits.

## Chunks

| No | Plan file | Result | Needs |
| --- | --- | --- | --- |
| 01 | `01-foundations.md` | Materials, face meshes, ray queries, vector documents | none |
| 02 | `02-static-stress.md` | Static stress study with a colour map | 01 |
| 03 | `03-print-preparation.md` | Print check, orientation, fit, 3MF | 01 |
| 04 | `04-plastic-features.md` | Rib, web, boss, snap hook, lip, groove | none |
| 05 | `05-mesh-tools.md` | Mesh report, repair, reduce, cut, mesh to solid | none |
| 06 | `06-drawings.md` | 2D drawings with views and dimensions | 01 |
| 07 | `07-design-structure.md` | Components, instances, BOM, joints, configurations, links | none |
| 08 | `08-face-operations.md` | Remove, move and split faces; emboss | 04 |
| 09 | `09-surface-analysis.md` | Curvature, zebra, draft, access, minimum radius | 01 |
| 10 | `10-sketch-constraints.md` | Constraint solver and sketch edits | 04 |
| 11 | `11-simulation-studies.md` | Modal, thermal, thermal stress, shape optimisation | 02 |
| 12 | `12-sheet-metal.md` | Sheet metal part and flat pattern | 01 |
| 13 | `13-surface-modelling.md` | Patch, surface features, trim, stitch | none |
| 14 | `14-machining.md` | 3-axis toolpaths, verification, G-code | 01, 05 |
| 15 | `15-render-and-animation.md` | Studio render, path tracer, turntable, exploded view | 01 |
| 16 | `16-subdivision-forms.md` | Subdivision cage modelling | 05 |

Recommended order: 01, 02, 03, 04, 05, 06, 07, then the others in number
order. Chunks with no common "Needs" entry can run one after the other in any
order. Do not run two chunks at the same time: the PC cannot build twice.

Fusion workspace to chunk: Simulation = 02 and 11. Generative Design = the
shape optimisation of 11. Manufacture = 03 (additive) and 14 (machining and
2D cutting). Drawing = 06. Sheet Metal = 12. Surface = 13. Form = 16. Mesh =
05. Render and Animation = 15. Electronics = Lamina, linked through 07.

## Shared rules (bind every chunk)

1. Read `AGENTS.md` first. All text is ASD-STE100 with British spelling. No AI
   attribution. Commit message style as in `AGENTS.md`.
2. A `faber-model` change goes in its own commit. Each new project file has a
   `schema` integer. Keyed collections are sorted maps.
3. `faber-core` and the domain crates never name a kernel, a file format or a
   vendor. A kernel operation is a new method of `faber_ports::Kernel` with a
   default body that returns "not supported". The implementation is in
   `crates/faber-kernel-occt` (`bridge.cpp` and `src/lib.rs`). A file format is
   a port in `faber-ports` with its writer in `crates/faber-files`.
4. A new capability with no kernel call is a new pure Rust crate
   (`faber-sim`, `faber-print`, `faber-meshtools`, `faber-drawing`,
   `faber-sketch`, `faber-sheet`, `faber-cam`, `faber-form`). `faber-wiring`
   joins it to the project, as it does for `faber-clash`.
5. No new C or C++ library. OCCT is the only native dependency.
6. Rust crates in the local cargo cache need no download: `rayon`, `rstar`,
   `spade`, `i_overlay`, `geo`, `kurbo`, `quick-xml`, `zip`, `image`, `png`,
   `tiny-skia`, `skrifa`, `earcutr`, `glam`. Use these first. If a chunk needs
   another crate, the coordinator runs `cargo fetch` once before the chunk and
   records the crate and its licence in the progress file.
7. Cargo: `--offline -j 4`, one cargo process at a time, tests with
   `-- --test-threads=1`. Tests never use the network. Never open a window: use
   `egui_kittest` and `faber render`.
8. A change to `bridge.cpp` builds the kernel crate again. Say so before the
   build starts.
9. A change that cannot be built is refused and leaves the files unchanged.
   Purchased parts never change: a tool that changes geometry refuses a
   purchased part.
10. Units: millimetre, newton, megapascal, degree, gram, degree Celsius, watt.
    Z is up. An internal solver can use other units; a reply never does.
11. Each analysis reply has `method`, `resolution` and `limits`. `limits` is a
    list of plain sentences that say what the result does not include.
12. Each capability has all of these parts before its chunk is complete:
    - a function in a domain crate with tests that use known exact results;
    - MCP tools with small replies, `expected_revision` on each tool that
      changes data, and descriptions that keep the limits of
      `tests/context_size.rs`;
    - a panel or an overlay in the viewer, with a headless test;
    - one section in `docs/modelling-guide.md` with a unique `##` heading;
    - helpers in `crates/faber-build/src/faber.libsonnet` for each new recipe
      operation;
    - one example project in `examples/`;
    - a line in `docs/specs/faber-progress.md` and the status in the gap note.
13. A new recipe operation is a variant of `faber_model::Geometry` with
    `snake_case` name. The `schema` tool then lists it with no more work. Add
    the operation to the feature list of the viewer
    (`crates/faber-viewer/src/app/feature_tools.rs`, `feature_form.rs`), as
    the `pipe` feature does.
14. A long calculation takes a `resolution` argument and refuses a size above
    its limit with an error that names the size and the limit. The viewer runs
    it on a worker thread.
15. Results that depend only on the revision use the read cache of the review
    plan (Task 4).

## Decisions (change them here before a chunk starts)

| No | Decision | Reason | Cost |
| --- | --- | --- | --- |
| D1 | Stress and thermal studies use a regular voxel grid with the finite cell method and hexahedral elements. No tetrahedral mesher. | No native mesher dependency; every solid and every repaired mesh works; one element matrix for the whole grid makes the solver fast. | Stress at a curved surface is less exact than with a fitted mesh. The reply says so. |
| D2 | Faber does not slice. It checks the part, selects the orientation and exports 3MF for a slicer. | A slicer is a product of its own. | No G-code for printers. |
| D3 | Machining is 3-axis milling, drilling and 2D cutting. | These cover hobby machines. | No turning, no 4 or 5 axes, no adaptive clearing. |
| D4 | Drawings are made through MCP and shown in the viewer. The viewer does not edit a drawing. | The AI makes the drawing; the person reads it. | No drag of a dimension by hand. |
| D5 | A form (chunk 16) becomes a faceted solid. No smooth NURBS conversion. | A correct conversion at irregular vertices is a research subject. | Curved form faces are many small flat faces. |
| D6 | Material values in the library are typical values. The implementer records a public source for each row. | Wrong values give wrong safety factors. | Slow first task. |
| D7 | Printed polymers get one strength factor for the layer direction. | A full orthotropic model needs test data. | The reply states the factor and its origin. |
| D8 | A study has one part. Join parts with `union` before a study. | Contact between bodies is a large subject. | No bolted or sliding contact. |
| D9 | The sketch solver is a new feature kind. The typed loops stay. | An AI writes numbers directly; a person drags. | Two sketch forms exist. |

## New project files

| File | Content | Chunk |
| --- | --- | --- |
| `faber.json` | adds `materials` (project materials) and `configurations` | 01, 07 |
| `parts/<id>.json` | adds `material`, `component` | 01, 07 |
| `assembly.json` | adds `components` | 07 |
| `analysis.json` | adds `studies`, `views` | 02, 15 |
| `manufacture.json` | `printer`, `machining` | 03, 14 |
| `drawings/<id>.json` | one drawing | 06 |

`crates/faber-files/src/layout.rs` reads and writes each new file. A project
that does not have a file reads as the default. The fingerprint includes each
new file.

## How to run one chunk

1. Check that the "Needs" chunks are complete (`docs/specs/faber-progress.md`).
2. Read the plan file and the decisions above. Change a decision first if the
   owner wants a different one.
3. Make a branch `parity-<no>-<name>`.
4. Run `superpowers:subagent-driven-development` on the plan file. Implementers
   use Haiku. Task reviewers use Sonnet. A task that changes saved data or a
   solver gets an Opus reviewer. The final review uses the most capable model.
5. Close: `cargo fmt --all`, workspace Clippy with `-D warnings`, all tests,
   the release build, the installer, the progress note, then merge and push.
