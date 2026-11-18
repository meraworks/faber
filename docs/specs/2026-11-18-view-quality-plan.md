# View quality and modelling checks

The owner requested smooth curved surfaces, a light scene, clear bottom views,
threaded parts and slicer checks. Do not connect to or send jobs to a printer.

- [x] Test smooth cylinder normals and sharp box edges. Use CAD surface normals;
  use angle-weighted corner normals with a 35 degree limit for mesh input.
- [x] Reduce curved-surface chord error and angular steps. Test small and large
  cylinders. Keep geometry work outside the UI thread.
- [x] Use a light neutral floor and background, with fill light below the model.
  Test top and bottom views. Inspect the reported bin artefacts.
- [x] Check existing curve, fillet, loft, sweep and thread tools against examples.
  Add missing basic thread controls if the existing tools cannot meet the task.
- [x] Build and reopen complex examples through MCP. Export and inspect them in
  the installed slicer, without a printer connection or a print action.
- [x] Run the final checks and record evidence and remaining limits.

Mesh import remains part of the active work. Keep downloaded models unchanged.
