# Open CAD surface research

Date: 2026-11-18. Scope: Open CASCADE 7.8.1 source, current FreeCAD source,
and Faber source. FreeCAD `main` links can change. No source code was copied.
This note does not describe Fusion internals.

## Observed behaviour

### Surface normals

FreeCAD enables `NormalsFromUV` by default. Its view code requests normals for
each CAD face. It keeps a separate node range for each face, reverses triangle
order when required, and applies the face placement to points and normals.
Thus, coincident points on two faces need not share a display normal.
[FreeCAD view source](https://github.com/FreeCAD/FreeCAD/blob/main/src/Mod/Part/Gui/ViewProviderExt.cpp#L1042-L1285)

The helper first uses stored normals. Otherwise, it gets the surface and calls
`GeomLib::NormEstim` at each mesh UV coordinate. If that fails, it averages
adjacent triangle normals within that face. It stores the surface normals before
it reverses the output for a reversed face. A separate helper applies placement.
This is evidence for CAD surface normals, not only triangle smoothing.
[FreeCAD normal source](https://github.com/FreeCAD/FreeCAD/blob/main/src/Mod/Part/App/Tools.cpp#L409-L490)

Open CASCADE 7.8.1 supplies `BRepLib_ToolTriangulatedShape::ComputeNormals`.
It uses UV coordinates and the surface, with a mesh fallback. It returns early
if normals already exist. It computes in the local face frame; it does not
reverse the result for a reversed face. The caller must apply orientation and
placement. Its last fallback can be the positive Z axis, so failure cases need
tests.
[Open CASCADE normal source](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_8_1/src/BRepLib/BRepLib_ToolTriangulatedShape.cxx)

### Mesh accuracy

Open CASCADE uses linear and angular deflection. Linear deflection controls
distance from a curve. Angular deflection controls the turn between curve
segments. Face interiors have separate controls. A shape tolerance can limit
the accuracy that the mesher achieves. Small deflection values can greatly
increase mesh size and time.
[Mesh guide](https://dev.opencascade.org/doc/overview/html/occt_user_guides__mesh.html)

The 7.8.1 parameter structure exposes `DeflectionInterior`, `AngleInterior`,
`MinSize` and `ControlSurfaceDeflection`. It also has a separate switch for
surface checks on all surface types. Relative deflection scales by edge size;
absolute deflection retains the model units.
[Mesh parameters](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_8_1/src/IMeshTools/IMeshTools_Parameters.hxx)

FreeCAD exposes linear deflection, angular deflection and relative mode through
`MeshPart.meshFromShape`. Its standard mesh path clears cached triangulation
before it meshes with the requested values. Export mesh quality can therefore
be set independently of the view properties.
[Mesh API](https://github.com/FreeCAD/FreeCAD/blob/main/src/Mod/MeshPart/App/AppMeshPartPy.cpp#L93-L103),
[mesh construction](https://github.com/FreeCAD/FreeCAD/blob/main/src/Mod/MeshPart/App/Mesher.cpp#L225-L237)

### Threads

FreeCAD distinguishes thread data from a modelled thread. Its Hole tool can
make real thread geometry, at a substantial recompute cost. The project tutorial
also distinguishes a helical thread from rings that only look like a thread.
[Hole documentation](https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/PartDesign_Hole.md),
[thread tutorial](https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/Thread_for_Screw_Tutorial.md)

## Assessment of Faber

The following points are deductions from the inspected code, not claims about
Fusion.

Faber requests 0.03 mm linear deflection in the core. The adapter uses 0.1 radian
angular deflection, absolute mode and serial meshing. It already keeps separate
face vertices and reverses triangle order for reversed faces. The subsequent
implementation now returns surface normals through the mesh port. The adapter
computes these from surface parameters and reverses them for reversed faces.
[Core](../../crates/faber-core/src/lib.rs),
[adapter](../../crates/faber-kernel-occt/src/bridge.cpp)

The scene code uses supplied surface normals when available. For mesh input
without normals, it groups corners by rounded position, then averages normals
within 35 degrees of each triangle normal. It uses corner angles as weights.
[Scene normals](../../crates/faber-scene/src/lib.rs)

The mesh fallback has four limits:

- A real crease below 35 degrees can appear rounded. Position alone does not
  identify which CAD faces meet or whether they are connected.
- A curved face beside a sharp boundary can get a biased mesh normal. Changing
  the diagonal layout can change the shading even when the CAD surface is fixed.
- A position-continuous boundary (C0) need not have a continuous normal. Keep
  normals from each side. A tangent-continuous seam can have matching normals
  without merging the face vertices.
- Better normals do not improve the silhouette or printed shape. Those depend
  on triangle positions. A fixed 0.03 mm error can become visible at high zoom.

For scale, a circular chord has sag `r * (1 - cos(a / 2))`. For a 10 mm radius
and a 0.1 radian segment, the sag is about 0.0125 mm. This is a geometric example,
not a prediction of the exact triangle count from Open CASCADE.

## Recommended changes

1. Return surface normals from the adapter through a vendor-free port type.
   Keep one normal per face node or triangle corner. Preserve mesh smoothing as
   a fallback for mesh-only input. Check normal count, finite values and length.
2. Reverse surface normals when the face is reversed. Apply placement and
   normalise them. Test a cylinder, sphere poles, torus seam, underside, shallow
   crease, fillet boundary and a rotated part. Include a surface with a C0 join.
3. Keep 0.03 mm and 0.1 radian as initial display settings while normals are
   corrected. Measure silhouette error before increasing triangle counts.
   Set explicit interior controls if the installed kernel supports them.
4. Give print export an explicit absolute tolerance in millimetres. Build or
   verify that mesh independently of the view cache. Check closed boundaries,
   consistent winding, zero-area triangles and self-intersections. A smooth
   screenshot is not evidence that a mesh is suitable for a slicer.
5. Use real helical geometry when a printed thread is required. Keep thread
   data separate from display decoration. Do not change a downloaded purchased
   part. Mesh settings cannot create a missing thread.
6. Test underside visibility separately from normals. Grid occlusion, face
   culling, triangle order and light direction are different possible causes.
   Add fill light only after geometry checks pass. Brightness alone does not
   prove that orientation is correct.

## Licences and reuse

The inspected FreeCAD C++ files declare `LGPL-2.1-or-later` in their SPDX headers.
Open CASCADE 7.8.1 declares LGPL 2.1 with its special exception and an alternative
commercial licence. The exception has conditions; it is not a general permission
to copy source into any project.
[FreeCAD file header](https://github.com/FreeCAD/FreeCAD/blob/main/src/Mod/Part/App/Tools.cpp#L1-L20),
[Open CASCADE file header](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_8_1/src/BRepLib/BRepLib_ToolTriangulatedShape.cxx#L1-L11),
[Open CASCADE exception](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_8_1/OCCT_LGPL_EXCEPTION.txt)

Use the installed Open CASCADE API from the adapter. Implement the Faber port
and tests from the requirements above. Keep the source links for review. Check
the exact file licence before any later source reuse.
