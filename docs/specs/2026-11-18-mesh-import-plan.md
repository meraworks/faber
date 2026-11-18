# Mesh import plan

Goal: import STL, 3MF, OBJ, PLY and OFF through MCP and the viewer. Keep STEP input.

Design: keep the source bytes in the existing asset store. Add an optional mesh
read port to AssetStore. FileAssets selects the reader from the stored content.
The core applies part placement and builds a mesh body without an exact shape.
Do not change model types. Do not convert mesh faces to exact faces.

Units: use millimetres for files without units. Apply 3MF units, build items and
component transforms. Preserve geometry; materials and textures are not imported.
Refuse unsupported required extensions and malformed data. Exact shape commands
continue to refuse mesh bodies. Keep reading and import work off the UI thread.

Tests use the public asset port and Project import operation. Check known geometry,
placement, source removal, undo, redo and refusal without a saved change.

- [x] Add a failing mesh import and reopen test; implement the port and STL reader.
- [x] Add reader tests and support OBJ, OFF and ASCII/binary PLY.
- [x] Add 3MF package tests and support units, components and build transforms.
- [x] Update MCP, viewer labels and documentation.
- [x] Run format, lint and workspace tests with at most six jobs.
- [x] Import the original 8x8 plate through MCP and inspect it in the viewer.

No commit or release is part of this task.
