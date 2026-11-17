# Faber implementation plan

Source: `2026-11-17-faber-design.md`.

## Rules

- Write a failing test before each new behaviour. Check the failure, add
  the code, then run the test again.
- Use Rust. Keep the required C++ connection in the kernel adapter.
- Run one build at a time, with at most six jobs.
- Tests must not use the network or open a window.
- Do not commit without a request from the owner.
- Keep model type changes separate from other changes for later commits.

## 1. Geometry ports and adapter

Create the workspace, opaque shape handles, mesh types and geometry ports.
Add the kernel adapter and its exception boundary. Test boxes, cylinders,
booleans, bounds, volume, tessellation and exchange round trips against
known values. Compare static and dynamic linking before distribution.

## 2. Project and build

Add model schemas, sorted maps, expressions and parameter checks. Test
degrees, precedence, cycles, unknown names and non-finite results. Add
feature builds, cache keys, atomic writes, locks, rollback and CLI commands.
Test that a failed build leaves every source file unchanged.

## 3. Render and MCP

Add shared scenes, cameras, headless images, labels and pixel selection.
Connect core operations to CLI and MCP. Test reports and protocol errors
through stdio. Render test parts without a window.

## 4. References and features

Add history selectors, queries, dress-up operations and patterns. Test
selectors after parameter changes and refuse incorrect result counts.

## 5. Curves

Add revolve, loft, sweep and splines. Test known volumes and bounds. Build
the nose, ring and wing examples from the design.

## 6. Purchased parts and assemblies

Deferred by the owner on 2026-11-17. See [deferred work](../TODO.md).
Current work focuses on AI-first CAD modelling. The tasks below remain for
a later stage.

Add import records, hashes, instances, placements, joints and poses. Use
local downloaded fixtures. Test that tools cannot change purchased models.

## 7. Checks and interference

Use `https://github.com/roku-on-it/clashrs` as the starting point for mesh
queries and pose scans. Its code uses paired BVH traversal in a local frame,
triangle intersection and distance queries, and parallel pair tests.
Keep geometry construction behind the kernel port.

Before integration, add tests for full containment without surface contact,
edge crossings, touching faces and invalid query input. Penetration depth
from vertices is approximate. Use a local worker pool with a fixed limit;
do not change the process-wide pool. Repeat the recorded benchmark with
the original fixture before stating a speed improvement.

Add mesh queries, check registration, print checks and assembly checks.
Assembly-specific checks and pose benchmarks are deferred with phase 6.
Test touching, separated and intersecting solids. Test strict rollback.
Measure the 60-part, 45-pose target with a fixed local fixture.

## 8. Output

Add exchange adapters, print orientation and plates. Test exported units,
names, colours and geometry. Refuse export with errors unless forced.

## 9. Viewer and section analysis

Use egui and wgpu with the shared scene. Add a clear part tree, properties,
checks, notes and view controls. Cache meshes and redraw on changes.
Keep file loading and geometry work off the interface thread.

Section analysis must support base planes and an arbitrary plane, offset
in millimetres, direction reversal and enable/disable controls. Fill cut
faces and preserve holes. Apply the same section to headless renders and
the viewer. Selection must ignore clipped geometry. A section must not
change source geometry or exported solids.

Test a hollow solid, an oblique cut, a plane outside the model and reversed
direction. Compare headless images. Use `faber-viewer shot` to inspect the
interface without opening a window. Measure frame time and update latency.

## 10. Standard parts

Add standard tables, hole sizes and tools. State the standard and size in
each record. Keep these distinct from downloaded purchased models.

## 11. Release checks

Stop before this phase. The owner set this boundary on 2026-11-17. Complete
the active work in phases 1 to 10, with phase 6 still deferred, then stop.
Normal change validation continues during development; phase 11 is not started.

Run format, lint and workspace tests in sequence. Test the example parts,
offline installation, licences and command help. Record performance on the
development PC. Release only on request.
