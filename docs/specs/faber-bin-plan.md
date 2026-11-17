# Faber bin implementation plan

## Goal

Build a hexagonal bin through Faber's stdio MCP tools. Show the result in
the native viewer after headless checks. Use the Yoink project as the
reference for size and layout. Do not open windows during development.

## Decisions

- Use the existing crate for interference scans.
- Add a Rust geometry adapter for closed polygon extrusions with holes.
  These operations are sufficient for flat bin panels, rails and rings.
  Keep the geometry port open for the curved-solid adapter in the main plan.
- Keep model, ports, core, adapters and wiring separate, as in Lamina.
- Use the Lamina stdio MCP pattern with rmcp. Tools call core operations.
- Use egui and wgpu for the viewer and the headless image path.
- Store editable part recipes, not only triangle output. Use millimetres,
  degrees and right-handed coordinates with Z up.
- Keep all new code in Rust. Do not change the Yoink reference files.
- No commits: the repository has no first commit and the owner has not
  asked for one. Work in the current folder to preserve its untracked files.

## Tasks and acceptance checks

1. Model and geometry ports. Add polygon extrusion and placement types.
   Test a 10 x 20 x 30 box at 6000 mm3, a hollow profile, closed mesh
   edges, positive winding and malformed profiles. Build a Rust adapter.
2. Project transactions. Add a store port and a file adapter. Create,
   open, add, replace and remove parts. Build before saving. Test rollback,
   duplicate names, unknown schemas and a second writer.
3. Stdio MCP and output. Add open, status, put_part, remove_part, render,
   export and guide. Exercise initialise, tool listing and calls through
   a real subprocess. Export closed meshes and report bounds and volume.
4. Shared scene and section. Add orbit camera, view presets, clipping and
   filled section faces. Test cuts through a hollow solid, an oblique plane,
   reversed direction and a plane outside the solid. Preserve source meshes.
5. Native viewer. Add a part tree, visibility, selection, properties,
   orbit/pan/zoom, section controls and file updates. Run the actual UI
   through a headless harness and inspect its images.
6. Bin. Use a Rust MCP client to send the part recipes to the running Faber
   server. Keep the transcript. Export parts, render exterior and section,
   and inspect bounds, closed meshes and intended contacts. Match the
   confirmed reference scope and use real downloaded purchased models if
   the robot base is included.
7. Final checks. Run format, Clippy and workspace tests, one build at a
   time with six jobs at most. Inspect exported files and UI images. Open
   the finished viewer only at the end, as requested by the owner.

## Review focus

- A failed operation must not change project files.
- A hole must stay open in the model and in a filled section face.
- Coordinates, units and normal directions must agree across adapters.
- The MCP transcript must prove that tools built the delivered bin.
- UI images must come from the actual viewer, not a separate mock-up.

## Progress

The owner confirmed a plain hexagonal bin only. No robot base, purchased
parts, bag frame or other mechanism. The bin is one solid with an open top,
a 2.4 mm wall and a 4 mm floor. It is 300 mm across opposite corners and
340 mm high. These sizes are builder choices from the reference dimensions.

Model, geometry, transactions and stdio MCP tests pass. Section tests cover
holes and oblique planes. The viewer and shared GPU renderer are in progress.

The owner clarified that the target remains a general CAD program. This
plan is the first working path only. Continue `faber-plan.md` after this
path; do not treat one completed example as completion of the CAD program.
