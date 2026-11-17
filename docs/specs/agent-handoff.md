# Faber agent handoff

Date: 2026-11-17. Workspace: `C:\Users\mranderson\projects\faber`.

## Stop instruction

The owner asked this agent to finish the current sketch grid task, stop and
report for another agent. Do not start another task without the owner's resume
instruction. The wider objective remains incomplete: finish active phases 1 to
10, then stop before phase 11. Phase 6 is deferred.

Read these files before continuation:

- `AGENTS.md` and the session instructions.
- `docs/specs/2026-11-17-faber-design.md` for the intended application.
- `docs/specs/faber-plan.md` for phases 1 to 11.
- `docs/specs/implementation-audit.md` for remaining requirements by phase.
- `docs/specs/faber-progress.md` for implementation and test evidence.
- `docs/TODO.md` for deferred purchased parts and assemblies.

## Current state

Faber is an AI-first CAD application with CLI, stdio MCP and an egui/wgpu viewer.
The hexagonal bin is an example, not the product scope. Section analysis exists
in the viewer and headless renderer. The product is not complete. Work reached
phase 5 while earlier phase gaps remained open; do not infer that phases 1 to 4
are complete from that number.

Recent work added face origins through transforms, booleans, extrude, revolve,
fillet, chamfer, draft, shell, offset, thicken, split, loft and sweep. The
`created_by` face selector uses recorded history. Face roles, sketch-item origins
and the full persistent selection design remain incomplete. Planar normals now
use surface derivatives, which fixes reversed normals on revolved caps.

Splines support interpolation, optional end directions and periodic closure in
2D and 3D. The viewer has spline drawing, background previews and a Closed spline
option. Helix paths exist in recipes. Reference planes, axes and points, including
planes from selected faces, exist. GUI reference creation exists; direct face
and edge picking remains open.

## Last task: sketch grids

`SketchLoop::Grid` contains `item`, `count: [columns, rows]` and two 2D `step`
vectors. It can occur in sketch holes. It expands to translated profiles in Rust
before the adapter constructs wires. Steps and item dimensions accept parameter
expressions. The first copy stays at the original item position.

- Model and number mapping: `crates/faber-model/src/lib.rs`.
- Expansion: `crates/faber-kernel-occt/src/sketch_arrays.rs`.
- Adapter integration: `crates/faber-kernel-occt/src/lib.rs`.
- Integration tests: `crates/faber-wiring/tests/sketch_arrays.rs`.
- Usage: `docs/modelling-guide.md`.

The expanded sketch is limited to 256 holes and 16 nested array levels. Zero
counts, zero repeated steps, excessive copies and an array as the outer boundary
are refused. Tests cover circle grids, parameter changes, all current loop and
segment types, oblique steps, known volumes, invalid input, unchanged files on
failure and reopen. Dedicated GUI grid controls and circular arrays are absent.

## Remaining work

Use the audit for the full list. Main gaps include:

- Persistent shape and mesh caches, the planned project file split and part-local
  parameter metadata and limits.
- Face roles, sketch origins, further semantic selectors and conversion of
  temporary topology ids to persistent selectors.
- Rich MCP change reports, labelled images and pixel selection, additional
  queries, suppression, saved views, materials and mass properties.
- GUI helix creation, tangent drawing controls, grid controls, reference pickers,
  exact face/edge selection, construction geometry and measured frame latency.
- The general check registry, rules and exclusions; the original clash benchmark.
- Exchange names and colours, state hashes, 3MF/glTF.
- Phase 10 standard tables and hole tools. These are distinct from purchased parts.
- Required desk-clamp and pegboard reference fixtures and their acceptance values.

Phase 6 purchased parts, joints and assemblies remain deferred. Printer and
slicer settings are outside scope. Do not start phase 11 packaging or release.

## Verification and workspace rules

The final check result is recorded in the sketch grid section of
`faber-progress.md`. Logs use `.cargo/sketch-arrays-*`. Do not infer a pass from
the existence of a log; read the final result and process exit status.

Run one Cargo process at a time, at most six build jobs. Use sequential tests:

```powershell
cargo fmt --all
cargo clippy --offline --workspace --all-targets -- -D warnings
cargo test --offline --workspace --all-targets -- --test-threads=1
```

Tests must not use the network. Do not open windows on the user's screen. Use
headless GUI tests, `faber render` and `faber-viewer shot`. The GUI does geometry
work in background workers. A full source build of the kernel can take about
20 minutes; the installed SDK normally avoids that build.

The worktree is uncommitted and its project files appear as untracked in Git.
No commit, push, tag or release was authorised. Preserve all current files;
`git diff` alone cannot show these changes. Model type changes must be separate
from other changes if commits are later authorised. Keep the application domain
independent of kernel and vendor names; use ports and wiring. Use British English
and short plain text for source comments and documentation.

## Reference research

`photocraft-research.md` records documentation, test-source and forum findings.
The reported 10.5 days is an initial alpha development interval, with 10–20
concurrent agents and estimated 2,000–4,000 agent-hours, not full Photoshop parity.
`fusion-reference-method.md` records the Fusion reference work and its limits.
Do not claim a Fusion runtime comparison or completed reverse engineering from
static metadata inspection. Lamina patterns can be copied when the plan says so;
do not add a path dependency on a Lamina crate.
