# Fusion feature gap

Date: 2026-11-18. Fusion is the behavioural reference. This note compares the
installed Fusion command set with Faber. It is static evidence from shipped
files. Fusion was not run.

## Viewer controls

Chunk 17 adds canvas selection, a view cube, command search and keyboard
control of edit windows. Chunk 18 adds face and edge selection, feature picks,
a marking menu and press pull for one planar face. Press pull records the
change in the part history. An arbitrary offset of curved faces is still open.

## Materials and shared geometry

Chunk 01 adds material assignment and a mass read with the effective density.
Triangle indices link a placed mesh to its solid faces. The ray queries and
vector writers supply geometry for picking, drawings and analysis.

## Static stress

The viewer and MCP can solve a linear elastic study on one part. The result
gives displacement, von Mises stress, fixture reactions and a safety factor.
The cell size controls the approximation at curved surfaces. Contact between
parts, large displacement and plastic deformation are outside this study.

## Print preparation

Faber checks bed fit, wall thickness, support and build orientation against a
printer profile. The viewer shows thin faces and can apply a chosen placement.
MCP also checks cylindrical fits and tolerance stacks. A 3MF assembly keeps
part colours and placements for the slicer. The support volume is an estimate.

## Plastic features

Part histories support ribs, webs, bosses, snap hooks, lips and grooves.
The snap check estimates the strain from the deflection of a rectangular
beam. The material limit, when supplied, applies to a short deflection.

## Mesh parts

Faber inspects, repairs, reduces and cuts imported triangle meshes. A closed
mesh can become an exact solid with editable features. Coplanar faces can
merge during conversion. A curved mesh surface keeps its flat facets.

## Drawings

A drawing stores references to the design. A change to a part updates its
views and dimensions. The sheet can show hidden edges, section hatching, a
detail, a parts list and a title block. Mesh parts supply their projected
outline. The viewer shows the sheet and writes SVG, PDF, DXF or PNG.

## Assembly structure

Chunk 07 adds a component tree, linked part copies and a bill of materials.
The viewer can isolate a component. Each joint coordinate has its own
control. Named configurations set parameters and suppress features.
A linked part keeps its last imported geometry when its source file is missing.

## Method and evidence

Target: deployment `39e8a3d5e919c70b05ef3ebfe9c6bfc8d9ee001e` under
`%LOCALAPPDATA%\Autodesk\webdeploy\production`.

- `Fusion/UI/FusionUI/Resources/Toolbar/TabToolbars.xml` gives the panels and
  command ids of the Design workspace tabs (Solid, Surface, Sheet Metal).
- `Fusion/UI/FusionUI/Resources/MarkingMenus/MarkingMenus.xml` gives the
  right-click marking menu and the context menu of `FusionSolidEnvironment`.
- `CPP/include/Fusion/**.h` (886 headers) gives the API object model: feature
  inputs, construction geometry, joints, analyses and export options.
- REA static inspection of `NaFusion10.dll` and `NaFusionUI10.dll` adds the
  Move command evidence. See the REA section below when it is recorded.

The parser is `scratchpad/fusion_ui.py` of the 2026-11-18 session. Command ids
are names, not proof of behaviour. Behaviour claims below come from the API
headers or from Autodesk documentation.

## Right-click menu in Fusion

The radial menu of the solid environment holds: Repeat, Press Pull, Redo, Hole,
a Sketch sub-menu, **Move/Copy** (`FusionMoveCommand`), Undo and Delete.
The context menu adds, for a selected body or component: Move/Copy, Align,
Create Components from Bodies, Edit Feature, Suppress, Rename, Copy, Cut, Paste,
Paste New, Delete, Show/Hide, Isolate, Opacity, Selectable, Find in Browser,
Find in Timeline, Look At, Physical Material, Appearance, Properties, Save as
STL/OBJ and Ground.

## Move command

`MoveFeatureInput.h` declares six move definitions:

| Fusion definition | Meaning |
| --- | --- |
| Free move | A transform from the triad (gizmo) drag |
| Translate XYZ | Distances along X, Y and Z |
| Rotate | An angle about an axis |
| Point to point | Move a point of the body to another point |
| Point to position | Move a point to coordinates |
| Translate along entity | A distance along an edge or axis |

The move applies to bodies or to components. The toolbar also has `AlignCmd`
(align geometry of one object to another).

## Comparison

Status: **have** = Faber has it, **part** = a reduced form, **gap** = absent.

### Create

| Fusion | Faber | Status |
| --- | --- | --- |
| Sketch, Extrude, Revolve, Sweep, Loft | sketch, extrude, revolve, sweep, loft | have |
| Box, Cylinder, Sphere, Torus | box, cylinder, sphere, torus, cone | have |
| Coil | helix + sweep | part |
| Pipe | sweep with a circle | part; gap as one feature |
| Hole, Thread | hole, thread (modelled) | have |
| Rib, Web | none | gap |
| Emboss (text and profiles on faces) | raise or engrave a closed sketch on one flat or cylindrical face | part |
| Rectangular, circular pattern, mirror | linear, circular pattern, mirror | have |
| Pattern on path | none | gap |
| Thicken | thicken | have |

### Modify

| Fusion | Faber | Status |
| --- | --- | --- |
| Move/Copy (free, XYZ, rotate, point to point) | numeric placement form; `Transform` feature | part |
| Align | none | gap |
| Press Pull, Offset Faces | planar press pull, signed offsets of selected faces and whole-solid offsets | part |
| Fillet (constant) | fillet | have |
| Fillet (variable, chord, rule, full round) | none | gap |
| Chamfer (equal) | chamfer | have |
| Chamfer (two distances, distance and angle) | none | gap |
| Shell, Draft, Combine, Split Body | shell, draft, booleans, split | have |
| Split Face, Replace Face, Remove Faces | split with a plane or sketch, remove selected faces and heal the solid. Replace Face remains open. | part |
| Scale | none | gap |
| Delete, Copy, Paste | remove part; no copy | part |
| Physical material, appearance | colour; density in a query only | part |

### Construct

| Fusion | Faber | Status |
| --- | --- | --- |
| Offset plane, plane on face | plane, plane on face | have |
| Plane at angle, tangent, midplane, three points, two edges, along path | none | gap |
| Axis through cylinder, two planes, two points, edge, normal to face | axis by point and direction | part |
| Point at vertex, edge-plane, three planes, circle centre | point by coordinates | part |

### Assemble

| Fusion | Faber | Status |
| --- | --- | --- |
| Joint (revolute, slider) | revolute, slider joints | have |
| Joint (rigid, cylindrical, pin-slot, planar, ball) | all five joint kinds | have |
| As-built joint, joint limits, drive joints | as-built study, limits, poses | have |
| Motion link | gear ratio links | have |
| Rigid group, ground, contact sets | rigid joints and declared contacts | part |
| Components and occurrences | nested components, local placements and part instances | have |

### Inspect

| Fusion | Faber | Status |
| --- | --- | --- |
| Measure, interference, section analysis | distance, interference, sections | have |
| Centre of mass, physical properties | `mass_properties` (MCP) | part; no viewer panel |
| Draft analysis | none | gap; overhang check for printing |
| Curvature comb, zebra, curvature map, minimum radius | none | gap |
| Accessibility analysis | none | gap |

### Sketch

| Fusion | Faber | Status |
| --- | --- | --- |
| Lines, arcs, circles, rectangles, slots, polygons, splines | yes, as typed loops | have |
| Geometric constraints and a solver | dimensions in loops only | gap |
| Text | none | gap |
| Offset, trim, extend, sketch fillet | none | gap |

### Timeline and design

| Fusion | Faber | Status |
| --- | --- | --- |
| Timeline, edit feature, reorder, rollback | ordered features, edit, move | have |
| Suppress feature | none | gap |
| Configurations | named parameter values and feature suppression | have |
| Named views | render views | part |

## Work order

The selection follows the product goal: an AI and a person design printable
parts fast and with few errors.

1. Move/Copy in the viewer: a triad with drag arrows and rotation rings,
   Translate XYZ and Rotate modes, live preview, copy option, one undo step.
2. `move_part` through MCP: relative translate, rotate about a pivot, point to
   point and copy. Align of part bounds.
3. Scale, Pipe and Pattern on path features.
4. Overhang (draft) analysis for a build direction, in the viewer and MCP.
   Physical properties in the viewer.
5. Later: suppress, variable fillet, two-distance chamfer, remove faces,
   construction variants, emboss and text, sketch constraints, components.

## REA evidence: Move and Align

Status: REA analysis did not run. This section has no Evidence IDs. The exported bundle holds 0 records.

### REA result

- `open_binary` opened `Applications\Fusion\NaFusionUI10.dll` as PE, x86_64, SHA-256 534ce29842097a052766db2fa6cd5f4b345d8f383ce313fb95b22b28b5a83d38. Evidence ID: none.
- No analysis provider runs for this file. Ghidra is not configured. Kuna does not admit PE. IDA is not configured. Hopper does not run on Windows.
- Therefore REA gives no string search, xref or pseudocode. Task steps 3 and the procedure review are not done.
- To enable analysis, set `GHIDRA_INSTALL_DIR` to an extracted Ghidra 12.1.x directory. Then repeat the REA steps.

### Local byte count (not REA)

A read-only `grep -c` on the DLL bytes gives these counts (UI DLL / core DLL):

- FusionMoveCommand: 1 / 1
- AlignCmd: 2 / 0
- MoveMethodFreeMove, MoveMethodTranslate, MoveMethodRotate, MoveMethodPoint2Point, MoveMethodPoint2Position, MoveMethodGeometry: 1 / 0 each
- CreateCopy: 3 / 0
- CaptureSnapshot, MoveCoordinateSpace, MoveCommandsMoveType: 1 / 0 each
- Pivot: 8 / 1

A count shows that a name occurs in the file. It does not show which code uses the name.

### Observations

Files: `Fusion\Build\Common\Tools\Localization\StringTable\en-US\NaFusionUI10.xml` (UI) and `NaFusion10.xml` (core). Line numbers from `grep -n`. Evidence ID: none.

- Move method labels, UI lines 4247 to 4253: Components, Bodies..., Free Move, Point to Point, Point to Position, Rotate, Translate.
- Move Object at line 4255. Move Type at lines 4218 and 4254. Coordinate Space at line 4228, with values Design (4216) and Component (4215).
- Other Move dialog labels: Direction (4242), Distance (4243), Point (4262), Axis (4268). Create Copy at line 1524. Ground To Parent at line 3479.
- Create Copies is offered for pinned or grounded components. UI lines 1049 and 1050.
- Align labels: Align (133, 140), From (141), To (142), prompts (138, 139), Aligned Axis (145), Align Components (136).
- ALIGN_INVALID_SELECTION, UI line 17: "The selections are invalid to create a parametric Align Feature."
- Core line 3042: "Use Set Pivot to reposition the manipulator." Core line 3041 gives the Move tooltip for components.
- Headers `CPP\include\Fusion\Features\MoveFeature.h` and `MoveFeatureInput.h` declare six move methods: FreeMove (transform), TranslateXYZ (x, y, z, isDesignSpace), TranslateAlongEntity (entity, distance), Rotate (axis, angle), PointToPoint (origin, target), PointToPosition (point, x, y, z, isDesignSpace).
- Six definition headers (`MoveFeature*Definition.h`) match these six methods.
- One search for class names with `Align` under `CPP\include` gave no match.

### Inferences (not proven)

- Move Type offers the six methods. Labels and headers agree. Confidence: high.
- Move Object selects bodies or components. Confidence: high.
- `isDesignSpace` is the Coordinate Space choice, Design or Component. Confidence: medium.
- Pivot sets the origin of Rotate and of the manipulator. Confidence: medium. Only tooltip text supports it.
- Align takes From and To geometry or snap points and makes a parametric feature. Confidence: medium. The Align feature class was not found in the headers.
- No Move snap or increment label was found. This does not prove that none exists, because the search was narrow.
- Preview behaviour was not checked.

## REA Ghidra evidence: Move command

Status: Ghidra analysis failed. There are no Evidence IDs. String, xref and decompile steps did not run.

### Run

- Target: `NaFusionUI10.dll`, 38 128 192 bytes, SHA-256 534ce29842097a052766db2fa6cd5f4b345d8f383ce313fb95b22b28b5a83d38.
- REA 6.7.0, Ghidra 12.1.4, provider `ghidra`, snapshot `.cargo\rea-nafusionui.snapshot`.
- Run 1: stopped at the 330 s startup limit.
- Run 2: `REA_GHIDRA_STARTUP_TIMEOUT_MS=3600000`. Stopped at the 60 min limit.
- Run 2 time: start 15:33:37, end 16:33:42 (+03:00). Run time 60 min 5 s.
- Error code: `provider_timeout`. Ghidra did not import and analyse the file.
- Error record: `.cargo\fusion-move-ghidra-evidence.json`. This file is an error record. It is not Evidence.

### Observations

- None from Ghidra. The observations in the section above stay byte counts and string-table lines. They are not Ghidra results.

### Next steps

- Set `REA_GHIDRA_DISABLED_ANALYZERS` to skip slow analysers. Then run again.
- Or use `NaFusion10.dll`. That file is larger and needs more time.
- Do not start a second Ghidra run while one runs.

### Inferences

- None. No Ghidra output exists.
