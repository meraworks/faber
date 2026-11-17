# Exact solid kernel plan

Use the main design and `kernel-windows-research.md`. The native polygon
path remains useful, but meshes must not replace exact solids in the CAD
model. Use the official OCCT 7.9.3 shared SDK behind the geometry port.

1. Add opaque owned shape values and measured properties to `faber-ports`.
   Add primitive, placement, boolean, tessellation and shape exchange
   methods. Keep product and format names out of domain types and ports.
2. Add `faber-kernel-occt`. A thin C ABI catches every C++ exception and
   checks operation status. Rust owns handles through a safe wrapper.
   Serialise calls and shape destruction. Never expose C++ types to Rust.
3. Test a 10 x 20 x 30 box at 6000 mm3. Move a second box by 5 mm on X;
   test union at 9000, difference at 3000 and intersection at 3000 mm3.
   Test invalid dimensions and a failed operation without process exit.
4. Test exact extrusion with holes and blind pockets. Build the same bin
   through the new adapter. Compare its volume to the formula. Export and
   read an exact shape; compare volume and bounds. Keep the meshes for views.
5. Add fillet, chamfer, shell, revolve, loft and sweep through the port.
   Test known values and failed operations. Add topology and operation
   history for selectors; do not store face indices as persistent references.
6. Connect the kernel to the project feature history and parameter engine.
   Test a parameter change that rebuilds downstream features, cached
   unchanged features, failed-build rollback and exact exchange through MCP.
7. Extend the GUI tree with the feature list and parameters. Keep the
   renderer on cached meshes. Repeat headless interaction and section tests.

No commit or release is authorised. No visible window before the requested
final demonstration. Tests use local files only. Build jobs stay at six or
fewer, with one build at a time.
