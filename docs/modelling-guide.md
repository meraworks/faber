# Faber modelling guide

Faber builds editable solids through stdio MCP. The client starts `faber mcp`.
Call `schema` to read the shape of a recipe. Call `guide` with a topic to read one section. `faber schema` prints
the project JSON schema. The separate viewer reads the same files.

## Quick start

Follow these six steps for each change.

1. Call `open` with an absolute project path. Set `create: true` only to create a new project. The reply gives the resolved absolute `path`.
2. Call `status`, `params` or `part_info` to read the current state.
3. Write the edits as one `batch` array named `operations`. Each item is flat.
4. Send the same `operations` array to `preview_batch`. Check the candidate. Nothing is saved.
5. Send the same `operations` array to `batch`. Set `expected_revision` to `base_revision`. Faber saves the change.
6. Call `render` to check the saved model. It writes a PNG and returns its path. Set `inline: true` to also get the image. If the result is wrong, call `undo`.

### Worked example: box with a fillet

This example opens a project, sets four parameters, and adds a box with rounded vertical edges. It then renders the box. Replace the path with your own absolute path.

Step 1. Call `open`:

```json
{"path":"C:\\work\\block","create":true,"name":"block"}
```

Step 2. Call `batch`. The fillet rounds the four vertical edges:

```json
{"operations":[
  {"tool":"set_param","name":"w","expression":"40"},
  {"tool":"set_param","name":"d","expression":"20"},
  {"tool":"set_param","name":"h","expression":"30"},
  {"tool":"set_param","name":"r","expression":"3"},
  {"tool":"put_part","id":"block","part":{"name":"Block","colour":[120,140,160],"geometry":{"op":"history","features":[
    {"id":"body","geometry":{"op":"box","size":["w","d","h"]}},
    {"id":"rounded","geometry":{"op":"fillet","source":"body","edges":{"parallel":[0,0,1],"tolerance":0.000001,"expect":4},"radius":"r"}}
  ],"output":"rounded"}}}
]}
```

Step 3. Call `render`:

```json
{"path":"C:\\work\\block.png","view":"iso"}
```

This example saves at step 2. To inspect first, send the same `operations` to `preview_batch`.

### Flat batch operation form

Each item in `operations` is one flat object. The `tool` field names the edit. The other fields are the arguments of that edit. Do not wrap the arguments in an `args` field.

Correct:

```json
{"tool":"set_param","name":"w","expression":"40"}
```

Wrong:

```json
{"tool":"set_param","args":{"name":"w","expression":"40"}}
```

Faber refuses the wrong form with `unknown field args`. These tools can be batch items:

- `set_param`
- `set_part_info`
- `put_part`
- `remove_part`
- `add_feature`
- `edit_feature`
- `delete_feature`
- `move_feature`
- `suppress_feature`
- `move_part`
- `align_parts`
- `set_contact`
- `set_section`
- `remove_section`
- `set_motion`

## Project files

A project folder holds `faber.json` (name, parameters, revision), one
`parts/<id>.json` for each part, and `placement.json`. It also holds
`assembly.json` and `analysis.json` when they have content.

A part file has no placement. Edit a part file as text when the design needs it.
A syntax error names the file, the line and the column.

A project with one `faber.json` still opens. Run `faber migrate <dir>` to write
the split layout.

## Schema tool

Call `schema` with a `kind` before you write a recipe. The kinds are `part`, `feature`, `geometry`, `sketch_loop`, `selector`, `section` and `edit`.

Call `schema` with `kind: geometry` and no `op` to list every op. Each line names the op and its fields. A `?` marks an optional field.

Add `op` to read the full schema of one op with its definitions. An unknown op name gives the nearest names.

## Verify the result

Call `measure` to read the distance between two parts. Call `part_info` to read the bounds and volume of a part.

Compare these values with your own calculation. Then call `render` to check the shape.

## Common mistakes

Each line names a mistake and the correct form.

- Wrapping batch arguments in `args`. Use flat fields, such as `{"tool":"set_param","name":"w","expression":"40"}`.
- Passing a list to chamfer or fillet `edges`. Use one object, such as `"edges":{"parallel":[0,0,1],"tolerance":0.000001,"expect":4}`.
- Guessing a sketch plane rotation. Use the table in Sketch planes.
- Guessing a hole direction. Use the table in Hole axes. For example, `[180,0,0]` cuts down and `[0,90,0]` cuts toward +X.
- Assuming a box is centred. A box starts at its origin corner. To centre a 40 x 20 x 30 box, set part `placement.translation` to `[-20,-10,-15]`.
- Using one tolerance unit for every check. Use millimetres for centre, box, length and radius. Use square millimetres for area.
- Writing a fixed number where a parameter must change. Use an expression, such as `"tolerance":"tol"`.
- Setting an edge selector on the final shape after an earlier fillet or chamfer. Call `topology` with `feature` set to the source feature id.
- Building a helix section with the wrong rotation. Use rotation `[atan(p/(2*pi*r))-90,0,0]` and translation `[r,0,0]`. See Thread example.
- Opening a relative path such as `/tmp/x`. Use an absolute path such as `"C:\\work\\part"`. Faber refuses other paths and replies `Use an absolute path, for example C:\work\part.`

## Batch and preview details

Put related changes in one batch. For example, put shared parameters and their dependent part in one batch. Faber builds the final state once and saves one undo step. A batch accepts 1 to 2000 operations.

A later operation sees the result of an earlier one. A `move_part` or `align_parts` after an `edit_feature` uses the new bounds. The changing tools reply with `changed`, the parts whose recipe, solid or placement changed, and `part_count`. A removed part is not listed, and `part_count` falls. Set `verbose` to `true` to get every part.

To inspect a change first, send the same operations to `preview_batch`. The result has `saved: false`, `base_revision` and a candidate report. The optional `render` accepts `path`, `width`, `height` and `section`. It writes and returns a PNG of the candidate. It does not change the project or the undo list.

To save that candidate, call `batch` with the same operations and `expected_revision` set to `base_revision`. Faber refuses a stale revision. Faber builds the model again on save. Preview does not reserve or keep a candidate.

Use `undo` or `redo` to move through saved changes. Use `checkpoint` to name a saved state. Use `restore` to return to that state.

Faber refuses malformed input and failed builds. A failed edit does not save the design. Set `strict: true` to refuse a change that adds a check error.

## Coordinates and parameters

Lengths use millimetres. Angles use degrees. Z is up. The axes are right-handed.

Number fields accept numbers or expression strings. Use `set_param` with a name and an expression. You can also put `set_param` in a batch. `params` returns expressions and calculated values. Trigonometric functions use degrees.

Part placement has `translation: [x,y,z]` and `rotation: [x,y,z]`. Faber applies rotations in this order: X, then Y, then Z. Features use part coordinates. The placement applies after them. Part and feature ids use letters, digits, underscores or hyphens.

### Expressions

Use an expression in any number field. Names are case-sensitive. A parameter cannot use the name `pi`. Faber accepts numbers such as `1e-3` and `.5`.

Operators, from highest to lowest priority:

| Operator | Meaning |
|---|---|
| `( )` | Brackets. |
| `^` | Power. Right to left: `2^3^2` is 512. |
| `-` `+` | Sign. `-2^2` is -4. |
| `*` `/` | Multiply, divide. |
| `+` `-` | Add, subtract. |
| `<` `<=` `>` `>=` `==` `!=` | Compare. The result is 1 if true, else 0. Faber refuses a chain such as `1 < x < 3`. Use brackets. |

Constant:

| Constant | Value |
|---|---|
| `pi` | 3.141592653589793 |

Functions:

| Function | Arguments | Result and units |
|---|---|---|
| `sin(a)` `cos(a)` `tan(a)` | 1 | Sine, cosine, tangent. `a` is in degrees. |
| `asin(x)` `acos(x)` | 1 | Inverse sine, inverse cosine. `x` is from -1 to 1. Result in degrees. |
| `atan(x)` | 1 | Inverse tangent. Result in degrees. |
| `atan2(y, x)` | 2 | Angle of the point (x, y) from the X axis. Result in degrees, from -180 to 180. `atan2(0, 0)` is 0. |
| `sqrt(x)` | 1 | Square root. `x` is not negative. |
| `abs(x)` | 1 | Absolute value. |
| `round(x)` | 1 | Nearest whole number. Halves round away from zero. |
| `min(a, b)` `max(a, b)` | 2 | Smaller value, larger value. |
| `if(c, a, b)` | 3 | `a` if `c` is not 0, else `b`. Faber does not calculate the other branch. |

Faber refuses a result that is not a finite number. Faber refuses division by 0.

## Frames and rotations

Use this section to place sketches, planes, holes and primitives. All rotations apply X, then Y, then Z.

### Sketch planes

A `sketch` or `plane` with `placement.rotation` has these local axes. The table applies to a `sketch` and to a `plane` used by `sketch_on_plane`. The local normal is local Z.

| Plane | `rotation` | Local X is | Local Y is | Local Z (normal) is |
|---|---|---|---|---|
| XY | `[0,0,0]` | world +X | world +Y | world +Z |
| XZ | `[90,0,0]` | world +X | world +Z | world -Y |
| YZ | `[90,0,90]` | world +Y | world +Z | world +X |

A rectangle with `origin: [0,0]` starts at the local origin and extends toward +X and +Y. An extrusion vector uses part coordinates. For example, the XZ sketch above extrudes toward -Y with `vector: [0,-20,0]`.

### Hole axes

A `hole` cuts along the local +Z axis of its `placement`. The entry is at the placement origin. Set the placement `translation` to the entry point on the face. Set the `rotation` from this table.

| Cut direction | `rotation` | Example entry face |
|---|---|---|
| `+Z` (up) | `[0,0,0]` | Bottom face at z = 0. |
| `-Z` (down) | `[180,0,0]` | Top face at z = 30. |
| `+X` | `[0,90,0]` | Face at x = 0. |
| `-X` | `[0,-90,0]` | Face at x = 40. |
| `+Y` | `[-90,0,0]` | Face at y = 0. |
| `-Y` | `[90,0,0]` | Face at y = 20. |

Verified example for a hole down from the top of a 40 x 20 x 30 box:

```json
{"id":"h","geometry":{"op":"hole","source":"b","diameter":4,"depth":10,"placement":{"translation":[10,10,30],"rotation":[180,0,0]},"head":null}}
```

### Where primitives start

| Primitive | Parameters | Where the solid starts |
|---|---|---|
| `box` | `size: [x,y,z]` | At the origin corner. It extends to +X, +Y and +Z. |
| `cylinder` | `radius`, `height` | Base centre at the origin. The axis is +Z. It extends from z = 0 to `height`. |
| `cone` | `base_radius`, `top_radius`, `height` | Base centre at the origin, at z = 0. The top is at z = `height`. |
| `sphere` | `radius` | Centre at the origin. |
| `torus` | `major_radius`, `minor_radius` | Centre at the origin. The ring lies in the XY plane. |

Only the box starts at a corner. The other primitives start at their centre. To centre a box, set part `placement.translation` to half of its size with a minus sign, such as `[-20,-10,-15]` for a 40 x 20 x 30 box.

## Recipes

A part has a name, a colour `[R,G,B]` and a geometry. Use `put_part` to add or replace one part. `put_parts` changes several parts together. `part_info` returns the recipe, placement, bounds and volume. The final result must be a valid solid.

Use `set_part_info` to change only the name, colour, group or note of a part. It does not rebuild the solid. Set at least one field.

For ordered operations, use a `history` geometry:

```json
{"op":"history","features":[
  {"id":"base","geometry":{"op":"box","size":[10,20,30]}}
],"output":"base"}
```

A reference names an earlier feature id. Intermediate sketches and curves can be faces or wires. Use `add_feature`, `edit_feature`, `delete_feature` and `move_feature` to edit the history. These changes keep the source recipes.

Faber supports these operations:

- box
- cylinder
- cone
- sphere
- torus
- extrude
- pocket
- sketch
- extrude_sketch
- revolve
- revolve_sketch
- loft
- loft_sketches
- sweep
- sweep_sketch
- curve
- transform
- union
- difference
- intersection
- split
- shell
- offset
- draft
- thicken
- hole
- fillet
- chamfer
- linear_pattern
- circular_pattern
- path_pattern
- mirror
- scale
- pipe

Read the argument schema for the fields of each operation. The pattern count includes the source. If the angle is 360 or -360 degrees, the copies divide the full circle equally. For example, count 6 and angle 360 give copies every 60 degrees. For any other angle, copy n is at n times the angle from the source.

`plane` defines a named reference feature with a `placement`. A plane is not a solid. It cannot be the part output. The `sketch_on_plane` operation takes a previous plane id, a `profile` and an optional local `placement`. Faber applies the local placement first, then the plane placement. A plane change rebuilds its dependent sketches. Extrusion vectors still use part coordinates.

`plane_on_face` takes `source`, a `face` selector and a signed `offset`. Select exactly one planar face. The origin is the area centre of that face, plus `offset` along its outward normal. Local Z follows that normal. Local X is the part X direction, projected onto the plane. For nearly parallel normals, Faber uses part Y instead. Local Y completes the right-handed frame. Faber refuses a curved face. The viewer References menu provides Plane on face with the same fields. The reference updates when the source changes. `sketch_on_plane` can use the reference.

`axis` defines an `origin` and a non-zero `direction` in part coordinates. `revolve_around` takes `source`, the axis feature id as `axis`, and `angle` in degrees. A change to the axis rebuilds the result.

`point` defines a `position` in part coordinates. `place_at_point` takes `source` and the point feature id as `point`. It translates source coordinates by the point position, with no rotation. It maps the source coordinate origin to the point. It does not align the solid centre. Axis and point references cannot be part outputs. Faber refuses missing, forward and wrong-type references. The viewer References menu creates planes, axes and points in the selected part. These actions keep the current solid output. Edit reference values in the feature list. The recipe tools provide the specialised reference operations. The viewer does not yet have reference pickers for them.

### Construction references

These operations make a plane, an axis or a point from other references. Each one is a feature in the history. Each keeps the current solid as the part output. Faber refuses missing, forward and wrong-type inputs. A change to a source or a parameter rebuilds the reference and its dependants.

`plane_at_angle` takes `axis`, an axis feature id, `from` and `angle` in degrees. `from` is `"xy"`, `"xz"`, `"yz"` or `{"plane": "<plane id>"}`. The axis must lie in the reference plane. The new plane contains the axis. Its normal turns by `angle` about the axis. The base normals are +Z for XY, +Y for XZ and +X for YZ. An angle of 30 about the X axis from XY gives the normal (0, -0.5, 0.866).

`midplane` takes `first` and `second`, two plane feature ids. The planes must be parallel. The result lies halfway between them and keeps the frame of `first`. For planar faces, first create two `plane_on_face` features.

`plane_through_points` takes `points`, a list of three entries. An entry is a point feature id or `[x, y, z]`. The points must be distinct. They must not lie on one line. Local X lies along the first edge.

`axis_from_cylinder` takes `source` and `face`. The face selector must match exactly one cylinder face. Faber does not support cone faces.

`axis_from_planes` takes `first` and `second`. The planes must not be parallel. The axis is their line of intersection. Its origin is the point on that line nearest to the world origin.

`point_at_centre` takes `source` and `edge`. The edge selector must match exactly one full circle. Faber does not support arcs or spherical faces.

```json
{"id":"tilt","geometry":{"op":"plane_at_angle","axis":"x_axis","from":"xy","angle":30}}
{"id":"mid","geometry":{"op":"midplane","first":"top","second":"bottom"}}
{"id":"tri","geometry":{"op":"plane_through_points","points":["p1","p2",[0,10,5]]}}
```

The viewer References menu creates these references with default values. If a default is refused, the viewer shows the error and saves nothing.

`helix` creates a curve path from `radius`, `pitch`, `turns` and an optional `placement`. The path starts at `[radius,0,0]`. It turns about local +Z. Pitch is the signed rise per turn. Turns must be greater than zero and at most 100. The total height must not exceed 1000000 mm. Use the helix as the path of `sweep_sketch`. The section must be normal to the starting tangent `[0,2*pi*radius,pitch]`. See Thread example. The adapter builds the 3D curve to a 0.0000001 mm approximation tolerance.

`split` takes `source`, a plane `origin` and `normal` in part coordinates, and `keep`: `both` (default), `positive` or `negative`. Positive follows the normal. The plane must cut through the source. Faber refuses a tangent or outside plane. Both sides stay separate solids within one feature result. They do not become separate part records. The viewer provides Split body and side controls.

`offset` takes `source` and `distance`. A positive distance expands a solid with round joins. A negative distance contracts it. Faber refuses zero. The result must stay a valid solid. This operation changes the whole solid. It does not move one selected face. The viewer provides it as Offset solid.

`scale` takes `source`, a `factor` and `about`, a point in part coordinates. A number scales all axes by the same factor. An array `[x,y,z]` gives one factor for each axis. Each factor must be a finite number greater than zero. Faber refuses zero, negative and non-finite factors. Faces keep their `created_by` origin. The result must be a valid solid.

`pipe` sweeps a round section along a path. Set `diameter` in millimetres. Set `path` to a `curve` or `helix` feature id. Set it to an inline curve with the fields of `curve` if no feature is needed. The circle is normal to the path start. Set `wall` to make a tube. The wall must be more than zero and less than the radius.

`path_pattern` copies `source` along `path`. `count` includes the source and must be an integer from 1 to 1000. Copy 0 stays at the path start. Without `spacing`, the copies span the whole path. With `spacing`, copy n is at n times the spacing. Faber refuses a spacing that runs past the path end. Set `orient` to true to turn each copy to the path tangent. Each copy moves with the path from the start. On a closed path, use `spacing` so that the last copy does not overlap the first.

`draft` takes `source`, a `faces` selector, `direction`, `origin`, `normal` and `angle`. Origin and normal define the neutral plane in part coordinates. A positive angle removes material on the direction side of that plane. A negative angle adds it. The angle is in degrees. It must be non-zero and strictly between -90 and 90. The viewer provides a Draft form with the same fields.

`thicken` takes `source` and signed `thickness`. Use a sketch face as the source. Or set `face` to a selector that finds exactly one face of a solid. A positive thickness follows the face normal. A negative thickness uses the opposite side. The result is a new solid from that face. It becomes the part output. Holes stay open. The viewer provides Thicken face, initially selecting the top face.

`hole` takes `source`, bore `diameter`, total `depth` and a `placement`. The entry is at the placement origin. The cut follows the local +Z axis. The bore has a flat end. Omit `head` for a plain hole. For a wider cylindrical entry, use `head: {kind: counterbore, diameter, depth}`. For a conical entry, use `head: {kind: countersink, diameter, angle}`. The angle is the included cone angle in degrees. The head must fit within the total hole depth and be wider than the bore. The viewer Hole form offers all three types. When you change the type, the bore expressions and placement stay. The head dimensions start from the selected defaults. Faber refuses a cut that removes no material. Use the table in Hole axes to aim a hole. These holes are geometric holes. They are not standard fastener or insert records.

Sketch loops include rectangles, rounded rectangles, circles, slots, ellipses, regular polygons, polygons
and paths with lines, arcs, Bezier or spline segments. An outer loop can contain holes.
Polygon point lists omit a repeated final point. A pocket leaves a closed floor;
its `floor` field sets the remaining base thickness.

A spline segment uses `kind: spline` and `points`. It starts at the previous
segment endpoint, or the path start. Supply 2 to 4096 further points, including
the endpoint. Each coordinate accepts an expression. Use 2D points in a sketch
and 3D points in a curve path. The curve passes through the points; they are not
control poles. Consecutive equal points are refused. Use a further segment to
close an open spline sketch. Optional `tangents` set the start and end directions.

Spline segments can set `tangents` to two vectors: start and end, in path order.
Use two coordinates for a sketch and three for a spatial curve. Each vector must
be non-zero and finite, with components within 1000000. Coordinates can use
parameter expressions. The kernel scales vector lengths for interpolation;
the vectors specify directions, not derivative magnitudes. Omit `tangents` for
unconstrained ends.

Set `periodic: true` for a closed spline in a sketch or spatial curve. Use one
spline segment for the whole path. Supply the start and at least two further
points; do not repeat the start or add a closing line. The join is smooth.
Periodic splines cannot set end tangents. The default is `periodic: false`.
In the viewer Spline tool, enable **Closed spline** to draw a periodic profile.
The preview updates when this option changes. Disable it for a straight closing
edge between the last and first points.

A slot uses `kind: slot`, `centre: [x,y]`, `length`, `width` and `rotation`.
Length includes both round ends and must be at least the width. Zero rotation
puts the long axis along X. Equal length and width produce a circle. A slot can
be an outer boundary or a hole. Its ends remain exact circular arcs.

A rounded rectangle uses `kind: rounded_rectangle`, `centre: [x,y]`, `width`,
`height`, `radius` and `rotation`. Width and height give the sharp size. Zero
rotation puts the width along X. The radius must be less than half the shorter
side. A rounded rectangle can be an outer boundary or a hole.

A `polygon` or `regular_polygon` loop can set `corner_radius`. It rounds every
corner with that radius. Zero or absent keeps sharp corners. Each edge must hold
two corner tangent lengths. For a regular polygon, the limit is the apothem:
`radius` times `cos(180 / sides)`, with the angle in degrees.

Each rounded corner is an exact arc tangent to both edges. A radius that does not
fit is refused. The refusal names the loop and the limit in millimetres. Every
new field accepts an expression, such as `"radius": "r"`.


## Edge and face selection

Call `topology` with part `id` and source `feature` to inspect cached edges and faces in part coordinates. Without `feature`, it reports the placed final shape in world coordinates. Each result states its coordinate space and revision. Do not reuse indices for a different shape or revision.

Fillet and chamfer select edges by direction, centre, length or radius. Shell selects faces by normal, centre or area. Set `tolerance` and `expect`. Faber then refuses an ambiguous selection. The adapter reports a normal only for a flat face. Use centre or area for other faces. Query the source feature, not the final placed shape, when you set a selector.

### Fillet and chamfer variants

A chamfer can have two distances. Set `distance2`. The first distance is `distance`.

A chamfer can have a distance and an angle. Set `angle_deg` in degrees. Do not set `distance2` with `angle_deg`.

Set `face` to choose the face where `distance` is measured. The face must touch each selected edge. Without `face`, Faber uses the first face that the kernel reports for each edge.

A fillet can have a variable radius. Set `radius_end`. The radius changes in a straight line from `radius` at the start of the edge to `radius_end` at the end.

Faber refuses a zero or negative distance, angle or radius. Faber also refuses an angle of 90 degrees or more.

Two distances, with a bevel of 2 mm and 4 mm on the first face of each edge:

```json
{"id":"bevel","geometry":{"op":"chamfer","source":"body","edges":{"parallel":[0,0,1],"tolerance":0.000001,"expect":1},"distance":2,"distance2":4}}
```

A distance and an angle, with 3 mm measured on the face that has normal -Y, at 45 degrees:

```json
{"id":"bevel","geometry":{"op":"chamfer","source":"body","edges":{"parallel":[0,0,1],"tolerance":0.000001,"expect":1},"distance":3,"angle_deg":45,"face":{"normal":[0,-1,0],"tolerance":0.000001,"expect":1}}}
```

A variable fillet from 1 mm to 3 mm:

```json
{"id":"round","geometry":{"op":"fillet","source":"body","edges":{"parallel":[0,0,1],"tolerance":0.000001,"expect":1},"radius":1,"radius_end":3}}
```

Old files stay valid. A new key is written only when you set it.

The `tolerance` is a distance in millimetres. Faber uses it for centre, box, length and radius checks. For an area check, it is an area difference in square millimetres. A normal check uses an exact direction. It does not use `tolerance`. Number fields such as `tolerance`, `in_box`, `length` and `radius` accept expressions. Use a whole number for `expect`.

An edge selector can also use `between: [face_selector_a, face_selector_b]`. Each face selector has its own tolerance and expected count. The edge must belong to two distinct matching faces, one from each set. Set the outer `expect` to the required edge count. Other edge filters apply as well. For the edge shared by the top and right faces of a box, use:

```json
{"between":[{"normal":[0,0,1],"tolerance":0.000001,"expect":1},
            {"normal":[1,0,0],"tolerance":0.000001,"expect":1}],
 "tolerance":0.000001,"expect":1}
```

This uses exact face-edge adjacency.

A face selector can set `created_by` to a feature id. The filter combines with normal, centre and area filters. Initial faces carry their creating feature id. Transforms preserve it. Boolean union, difference and intersection use recorded unchanged and modified face links to carry it forward. Faces with no recorded ancestor receive the current feature id. Topology replies include `created_by` as a list of source feature names, or null when history is unavailable.

Extrude, revolve, loft and sweep assign new faces to the solid feature. They do not assign these faces to the source sketch. Sketch-item origins remain unavailable.

Fillet and chamfer preserve the origins of unchanged and modified faces. New rounded and bevelled faces carry the fillet or chamfer feature id. Draft preserves source feature ids for faces whose angle or boundary changes.

Shell preserves recorded source faces. New inner faces carry the shell feature id.
Shell thickness must be positive. The shell grows inward from the outer faces.
For other wall thicknesses, add `thickness_overrides`: a list of rules, each with
`faces` (a face selector) and `thickness` (a positive number). Each rule must match
at least one face. A face can be removed or get one rule, not both. For example,
a tray with 2.4 mm walls and a 4 mm floor uses
`"thickness_overrides":[{"faces":{"normal":[0,0,-1],"expect":1,"tolerance":0.000001},"thickness":4}]`.
Shell before you fillet or chamfer the edges that touch the removed faces.
A fillet radius smaller than the thickness can also fail.

Offset preserves source face ids through its recorded face links. New round transitions carry the offset feature id.

Thicken preserves the original face id. New side and offset faces carry the thicken feature id. This also applies when thickness is negative.

Split preserves source ids on retained faces and assigns the cut face to the split feature. This applies when one or both sides are kept.

Not all operations keep face history yet. Faber refuses a `created_by` selector on such a source. It does not guess from position or face number. Face roles and sketch-item origins are not yet available.

Sketch holes can contain a `grid` with `item`, `count: [columns, rows]` and
`step: [[column_dx, column_dy], [row_dx, row_dy]]`. The item is a sketch loop.
Steps and item dimensions accept expressions. The first copy uses the item's
original position. Counts include that copy. Steps can define an oblique grid.
The expanded sketch can have at most 256 holes and 16 nested array levels.
Zero counts, zero repeated steps and arrays used as the outer boundary are refused.
Grid creation is available in recipes; a dedicated GUI control remains open.

### Region and perimeter filters

A selector sees the shape after all earlier features. An earlier chamfer or fillet changes edge lengths and centres.

Set `in_box` to keep only items whose centre lies inside a box. The box has two corners, `[[xmin, ymin, zmin], [xmax, ymax, zmax]]`. Each value can be an expression. The `tolerance` widens the box on every side. A face selector accepts `in_box` in the same way.

```json
{"in_box":[[-0.5,-0.5,29.5],[10.5,20.5,30.5]],"tolerance":0.000001,"expect":4}
```

This selects the four top edges of a 10 x 20 x 30 box.

Set `of_face` on an edge selector to a face selector. The result is every edge that bounds a matched face. The adjacency is exact, as with `between`. Use this to chamfer or fillet the whole outline of one face. The inner face selector has its own `tolerance` and `expect`.

```json
{"of_face":{"normal":[0,0,1],"tolerance":0.000001,"expect":1},"tolerance":0.000001,"expect":4}
```

### Topology filters

Set `kind` to `edges` or `faces` to return one list. Set `edge` or `face` to keep only the items that pass a selector. Each list holds at most 50 items. Set `limit` to change the cap. There is no offset; to read more items, raise `limit`. The reply always has `total_edges` and `total_faces`, the counts before the cap. A list that `kind` leaves out has a null total. The `expect` value of a filter is not checked.

```json
{"id":"base","kind":"faces","face":{"normal":[0,0,1],"tolerance":0.000001,"expect":1}}
```

### Face split by an earlier feature

An earlier feature can split one face into several faces. Select the piece that you need with `in_box`. Its centre must lie inside the box. Read the faces with `topology` after the split. Example:

```json
{"id":"base","kind":"faces","face":{"normal":[0,0,1],"in_box":[[0,0,9.5],[10,20,10.5]],"tolerance":0.000001,"expect":1}}
```

### Count errors

When the count differs from `expect`, the error lists the matched items, up to 5. If there are too few, it also lists up to 3 closest rejected items. Each rejected item names the filter that rejected it. Use these lines to correct the selector.

## Text and emboss

A sketch loop of kind `text` makes one line of text. Faber makes the glyph outlines from a built-in font. Each glyph contour becomes a sketch loop. A letter counter, such as the hole of "O", becomes a hole.

Fields of a text loop:

- `text`: one line of 1 to 256 characters. A line break is refused.
- `height`: the cap height in mm. Expressions are accepted.
- `at`: the `[x, y]` start of the baseline in sketch coordinates. `align` moves the text only along x. `left` starts at `at[0]`. `centre` centres on `at[0]`. `right` ends at `at[0]`.
- `align`: `left` (default), `centre` or `right`.
- `angle_deg`: the turn about `at` in degrees. Positive is counter-clockwise. Absent means 0.
- `font`: `ubuntu-light` (default) or `hack`. Faber refuses other names.

Text can only be the outer boundary of a sketch. A text sketch cannot have extra holes. Faber flattens each curve to line segments. The tolerance is 0.002 mm for each mm of height, so it is 0.02 mm at 10 mm height. A letter with separate parts, such as "i", gives several regions. Faber fuses the solids of the regions. A text sketch can have at most 256 outlines.

Faber refuses text with no visible outline, such as a space only. Faber refuses a non-positive height.

For raised or engraved text on a face, use the existing sketch route. Use `plane_on_face` for the face, `sketch_on_plane` for the letters and `extrude_sketch` for the depth. A positive vector adds material. A negative vector removes material when the result is a `difference`. Extrusion vectors use part coordinates. Faber has no separate emboss feature.

Raised text on the top face of a 40 x 20 x 5 mm box:

```json
{"name":"Tag","colour":[100,140,170],"geometry":{"op":"history","output":"tag","features":[
  {"id":"base","geometry":{"op":"box","size":[40,20,5]}},
  {"id":"top","geometry":{"op":"plane_on_face","source":"base","face":{"normal":[0,0,1],"expect":1,"tolerance":0.000001},"offset":0}},
  {"id":"letters","geometry":{"op":"sketch_on_plane","plane":"top","profile":{"outer":{"kind":"text","text":"LAB-01","height":6,"at":[0,-3],"align":"centre"}}}},
  {"id":"raised","geometry":{"op":"extrude_sketch","source":"letters","vector":[0,0,1]}},
  {"id":"tag","geometry":{"op":"union","left":"base","right":"raised"}}
]}}
```

Engraved text, 1 mm deep, uses the same features with two changes. The vector is `[0,0,-1]`. The last feature is a `difference`:

```json
{"name":"Tag","colour":[100,140,170],"geometry":{"op":"history","output":"tag","features":[
  {"id":"base","geometry":{"op":"box","size":[40,20,5]}},
  {"id":"top","geometry":{"op":"plane_on_face","source":"base","face":{"normal":[0,0,1],"expect":1,"tolerance":0.000001},"offset":0}},
  {"id":"letters","geometry":{"op":"sketch_on_plane","plane":"top","profile":{"outer":{"kind":"text","text":"LAB-01","height":6,"at":[0,-3],"align":"centre"}}}},
  {"id":"cut","geometry":{"op":"extrude_sketch","source":"letters","vector":[0,0,-1]}},
  {"id":"tag","geometry":{"op":"difference","left":"base","right":"cut"}}
]}}
```

The origin of the face plane is the area centre of the face. The text `at` values are relative to that origin.

Fonts and licences. Faber compiles the fonts into the program. No font file is read at run time.

- `ubuntu-light` is Ubuntu Light. It comes from the `epaint_default_fonts` 0.37.0 crate. Its licence is the Ubuntu Font Licence 1.0.
- `hack` is Hack Regular. It comes from the same crate. Its licence is the MIT License. The crate also keeps the Bitstream Vera and DejaVu notices.

## Thread example: helix and sweep_sketch

Use `metric_fastener` with `{"size":"M3"}` to read coarse pitch, tap drill,
clearance and head dimensions. Supported sizes are M1.6, M2, M2.5, M3, M4,
M5, M6, M8, M10, M12, M16 and M20. Missing head data is returned as null.
This query does not create a purchased part or a thread. The viewer Hole form
has metric clearance and tap-drill presets. Clearance presets use medium fit;
the dimensions remain editable. A tap-drill hole has no modelled thread.

This example builds a helical ridge with a triangular section. It uses a helix path and a `sweep_sketch`. The section must be normal to the start of the helix.

The rules are:

1. The helix starts at `[r,0,0]` and turns about +Z. Positive pitch rises counter-clockwise when seen from +Z.
2. The helix angle is `atan(p/(2*pi*r))`. The start tangent is `[0,2*pi*r,p]`.
3. The section sketch has translation `[r,0,0]`, the path start. Its rotation about X is the helix angle minus 90 degrees. This turns the section normal onto the start tangent.
4. `sweep_sketch` sweeps the section along the helix.

For `r` = 10, `p` = 5 and `n` = 3, the helix angle is 4.56 degrees. The section rotation is -85.44 degrees. This JSON built one solid of 552.7 mm³. Replace the path with your own absolute path:

```json
{"path":"C:\\work\\thread","create":true,"name":"thread"}
```

```json
{"operations":[
  {"tool":"set_param","name":"r","expression":"10"},
  {"tool":"set_param","name":"p","expression":"5"},
  {"tool":"set_param","name":"n","expression":"3"},
  {"tool":"put_part","id":"thread","part":{"name":"Thread","colour":[120,140,160],"geometry":{"op":"history","features":[
    {"id":"path","geometry":{"op":"helix","radius":"r","pitch":"p","turns":"n"}},
    {"id":"section","geometry":{"op":"sketch","placement":{"translation":["r",0,0],"rotation":["atan(p/(2*pi*r))-90",0,0]},"profile":{"outer":{"kind":"regular_polygon","centre":[0,0],"radius":1.5,"sides":3,"rotation":0}}}},
    {"id":"thread_solid","geometry":{"op":"sweep_sketch","source":"section","path":"path"}}
  ],"output":"thread_solid"}}}
]}
```

Call `render` with `view` set to `iso` to check the shape. The render shows three turns of a triangular ridge.

## Inspection and output

`status` returns part bounds, volume and triangle counts. `interference` checks the current parts with a requested `proximity_mm`. The next three subsections explain its findings, declared contacts and the export refusal.

`measure` accepts part ids `a` and `b`. It returns `distance_mm` and one pair of closest points, `point_a` and `point_b`, in world coordinates. It measures the placed exact shapes within kernel tolerance. It does not rebuild or save them. Zero distance can mean contact or overlap, including containment. It is not a penetration depth. Several equally close point pairs can exist.

`mass_properties` accepts a part id `id` and an optional `density_g_cm3`. It returns `volume_mm3`, `area_mm2` and `centre_of_mass_mm` in world coordinates. If you set `density_g_cm3`, it also returns `mass_g` and `inertia_kg_mm2`. The inertia tensor is about the centre of mass. Faber refuses a density that is zero or below. The tool uses the placed exact shape. It does not save the project.

`render` writes a PNG to `path`. The reply has the path. Set `inline` to `true` to also send the image. Set `view` to one of these values. The default is `iso`.

- `iso`
- `front`
- `back`
- `left`
- `right`
- `top`
- `bottom`

These are perspective views from the selected direction. Front looks from negative Y, right from positive X, and top from positive Z. The same option is available in a preview render. Set `part` to a part id to draw and frame only that part. Omit it to draw the whole project. Preview renders resolve the id in the candidate. A new part can be inspected before you save it. Faber refuses an unknown id. The optional `section` has `normal: [x,y,z]`, `offset` in millimetres and `reverse`. The section fills cut faces and keeps holes. It changes the view, not the source or the exported solid.

The section keeps the material on the side opposite to `normal`. Set `reverse` to `true` to keep the material on the side that `normal` points to. A view along the plane shows the cut face edge-on. To see the cut face, use a view along `normal`. For `normal: [1,0,0]`, use the `right` view.

`export` writes one STL or STEP file per part into `directory`. Set `format` to `stl` or `step`. Faber refuses an export when the check finds a violation. Set `force` to `true` to export anyway. For STL, set `tolerance_mm` from 0.001 to 1.0 mm. The default is 0.01 mm. Each entry of `files` is an object with `path` and, for STL, `triangles`. The reply holds `tolerance_mm` for STL. STEP has no triangle count. The viewer and the library still write STL at 0.001 mm. `import_part` keeps an owned STEP solid in the project and makes a new part. Use the purchased part record for catalogue hardware. Slicer settings are separate.

### Feature details

Call `feature_info` with `part` and `feature` to read one feature in part coordinates. A solid gives `volume_mm3`, `bounds_mm`, `faces`, `edges`, `suppressed` and `depends_on`. A plane gives `origin`, `x`, `y` and `normal`. An axis gives `origin` and `direction`. A point gives `point`. Set `features` to `ids` in `part_info` to read only the feature ids.

### Interference and contact

`interference` returns `status`, which is `PASS` or `FAIL`, and a list for each kind. Each finding has the part ids `a` and `b`, and a `depth_mm` or a `gap_mm`.

Each finding has one of these kinds:

- `interference`: The parts overlap by more than 0.000001 mm. This is a violation.
- `contact`: The parts touch. The depth is at most 0.000001 mm, and the exact common volume is at most 0.000001 mm3. This is not a violation.
- `clearance`: The parts do not touch, and the gap is less than `proximity_mm`. This is a violation.
- `allowed_contact`: A declared pair touches or overlaps. This is not a violation.

Depth is approximate. The exact kernel check runs on pairs that the mesh check reports as contact. It also runs on a declared pair that the mesh check finds deeper than its limit. A surface crossing that the mesh check cannot measure is always a violation.

### Declared contacts

Some parts must touch or press on purpose. Declare such a pair with `set_contact`.

Set `a` and `b` to the two part ids. Set `allowed` to `true` to declare the pair. Set `allowed` to `false` to remove it. Set `note` to a short reason, up to 500 characters.

Set `max_depth_mm` to the deepest allowed overlap, in millimetres. Use zero to allow touch only. The value must be finite and zero or more. The default is 0.6 mm.

A declared pair with touch or an overlap up to `max_depth_mm` gives no violation. If the mesh check finds a deeper overlap, Faber checks the common volume with the exact kernel. If the common volume is empty, the pair is a touch and gives no violation. Otherwise, the pair is a violation.

A surface crossing that the mesh check cannot measure is still a violation.

For example, a lid that seats 0.3 mm into its base gives no violation. If `max_depth_mm` is 0.6, a lid that goes 5 mm into the base is a violation. If `max_depth_mm` is 6, the same lid gives no violation.

A declaration changes only the contacts in `assembly.json`. It also counts as one revision. Use `undo` to remove it. Removing a part also removes its declared contacts.

### Export refusal

`export` runs the same check. If a violation exists, Faber refuses the export and writes no file.

The refusal lists each violation on its own line. Each line has the two part ids, the kind, and the depth or gap in millimetres:

```
Export refused: 1 interference or clearance finding.
box and lid: interference, depth 1.000 mm.
Declare an intended contact with set_contact, or use force: true.
```

A declared pair that is deeper than its limit has its own line:

```
box and lid: interference, depth 5.000 mm, above the declared contact limit of 0.600 mm.
```

To allow a pair, declare it with `set_contact`. To export with violations, set `force` to `true`. A `strict` batch uses the same rules. A declared contact that goes deeper than its limit is a new violation.

## Section analysis

A section analysis is a saved plane. It cuts the views and gives numbers. It never changes a solid.

Save a section with `set_section`. Give an `id` and a `section`. The plane has one of three kinds:

- `plane`: an `origin` and a `normal` in world coordinates, in millimetres.
- `reference`: a `plane` feature of a part (`part`, `feature`), plus an `offset` along its local Z.
- `face`: a planar face of a part (`part`, optional `feature`, `face` selector), plus an `offset` along the outward normal.

Example:

```json
{"tool":"set_section","id":"mid","section":{"plane":{"kind":"plane","origin":[0,0,15],"normal":[0,0,1]}}}
```

Optional fields of a section:

- `parts`: the part ids to cut. An empty list cuts all parts.
- `reverse`: flips the direction of the plane.
- `visible`: set to `false` to hide the section in the viewer. A render that names the section still draws it.
- `note`: up to 500 characters.

Read the exact numbers with `section_info`. Each cut part gives:

- `area_mm2` and `perimeter_mm`: exact. The perimeter includes the holes.
- `bounds_plane_mm`: the 2D bounds in the plane axes that `section_info` returns.
- `regions` and `holes`.
- `min_wall_mm`: approximate. It is the least distance from an outer boundary to a hole boundary. It is absent when no region has a hole.

A plane that misses a part gives zero area. It is not an error.

In the viewer, the plane cuts the listed parts. You can still see and select the other parts. Moving a part updates its cut face. Drag a saved plane and save it to keep the same part list.

Draw a saved section with `render` and set `section` to its id. A render with a section that lists parts draws only those parts. Preview renders take an inline plane only.

Remove a section with `remove_section`. `set_section` and `remove_section` also work in `batch` and `preview_batch`. Each change makes one undo step.

A part removal refuses while a section plane uses that part. Otherwise the part leaves every `parts` list.

## Overhang analysis

An overhang analysis finds faces that need support when you print a part. It never changes a solid.

Each triangle has an angle to the build direction. The default build direction is up, `[0,0,1]`. The default critical angle is 45 degrees. A face that points down within this angle needs support.

Run the `overhang_analysis` tool. Set `part` to one part id. Leave it out to analyse every part. Set `direction` and `critical_angle_deg` to change the rules. The critical angle goes from 0 to 90 degrees.

The result gives:

- `support_area_mm2`: the area that needs support.
- `bed_area_mm2`: the area on the bed. It needs no support.
- `total_area_mm2`: the whole surface area.
- `flattest_overhang_deg`: the smallest angle from the downward direction over the support faces. Zero is a flat underside. It is absent when no face needs support.
- `regions`: up to five largest support regions. Each region gives `part`, `centroid_mm`, `area_mm2` and `triangles`.

Each part has its own bed. The bed faces are the faces at the lowest point of the part. Their corners lie within 0.05 mm of that point, and they point down.

In the viewer, select the OVERHANG check box in the analysis panel. Red faces are flat. Orange faces are near the critical angle. Blue faces are on the bed. The slider sets the critical angle.

## Model a metric thread

Add a `thread` feature after the feature that makes the shaft or bore. Set `source` to that feature. Use `face.cylinder_radius` and `face.internal` to select one cylindrical face. Set `size` to a coarse metric designation such as `M6`. Set `length` and `offset` in millimetres. Set `left_handed` to `true` for a left-hand thread.

Open `examples/metric-thread-pair` for an M6 shaft and sleeve. Each thread has an axial length of 8 mm. The profile uses nominal dimensions. It has no tolerance class or allowance for a printed fit.

## Drive an assembly

Open `examples/assembly-motion`. Under **Assembly motion**, select **Drag moving parts**. Drag the input arm to turn the follower and the ratio indicator. Drag the carriage along its guide. Use **Reset pose** to return all parts to their assembled positions. The model stores the joint definitions. Dragging changes the view only.

Select a body and use **Add joint to selected part** to set an axis and an origin. Coordinates use the assembled world frame. Open **Edit joints and relations** to set travel limits, attach parts to a common joint, specify a parent joint, enter a signed gear ratio or connect two rotating arms with a rigid rod. Supply the two pin centres at the assembled position.

Call `set_motion` with the `motion` object from the example project. Call `motion_pose` with `{"values":{"input":15}}` to get the world displacement of each moving part. Unspecified drives stay at zero. The solver refuses an unreachable rod position or a value outside the limits.

Call `sweep_motion` with `{"joint":"input","range":[-30,30],"proximity_mm":0}` or use **Check travel** in the viewer. The check excludes pairs with no relative motion. It returns a pair and input value when it finds a violation. It returns an uncertain interval when it cannot establish clearance between checked positions. Mesh contact can require inspection of the original solids.

To capture a pose, run `faber-viewer shot PROJECT --joint input 15 --out pose.png`. Repeat `--joint` to drive more than one independent joint.

## Move and align parts

Call `move_part` with exactly one mode. The call refuses zero or several modes. Set `id` for one part. Set `ids` to a list of part ids to move a group. The group moves by one motion.

- `translate`: move by `[dx, dy, dz]` in mm. The move is relative.
- `rotate`: set `axis` to `"x"`, `"y"`, `"z"` or a vector. Set `angle_deg`. Set `about` to `"centre"`, `"origin"` or a world point. The default is `"centre"`, the centre of the bounding box.
- `point_to_point`: set `from` and `to`. The part moves by the vector between them.
- `position`: move the bounding box centre to this world point.

Example: `{"id":"bracket","rotate":{"axis":"z","angle_deg":90}}` turns the bracket 90 degrees about its centre.

Each move is one undo step. If you want to keep the source part, set `copy_as` to a new id. Set `expected_revision` to refuse a stale change. A placement with a parameter expression is refused. Replace the expression with a number first.

Call `align_parts` to line up a part with a target part. Use `"origin"` as the target to line up with the world origin.

- `axes`: list `"x"`, `"y"` or `"z"`. Only these axes move.
- `mode`: `min`, `max`, `centre`, `min_to_max` or `max_to_min`.
- `gap_mm`: a gap in mm. Use it with `min_to_max` and `max_to_min` only.

To stack a part on top of another, use `min_to_max` on the `z` axis. Example: `{"id":"lid","target":"box","axes":["z"],"mode":"min_to_max","gap_mm":0.5}`. The move uses world bounding boxes and changes translation only.

## Suppress a feature

Call `suppress_feature` with `part` and `id`. The default is `suppressed: true`. Set `suppressed: false` to unsuppress the feature.

A suppressed feature is not built. It stays in the recipe. Faber saves the flag as `"suppressed": true`. An active feature has no flag.

A suppressed feature with an input solid passes that solid on. The input is the `source` of a fillet, chamfer, shell, draft, offset, hole, thread, transform, scale, split or pattern. For a boolean, the input is `left`. Later features and the output use the input solid. Suppressing a fillet or hole therefore removes its effect and keeps the chain. A suppressed chain resolves to its first input.

A creation feature has no input solid, such as a box, cylinder, sketch extrude, sweep or imported part. Suppressing it leaves no solid. A later feature that needs it is refused. The error names that later feature. Set `cascade: true` to suppress the later features too. A boolean with a suppressed `right` operand is refused in the same way. With cascade, the boolean is suppressed and passes on its `left` operand.

Faber refuses to suppress the output when the output has no input solid. Faber refuses to unsuppress a feature whose source has no solid. Unsuppress the source first.

Each suppress change is one undo step. Set `expected_revision` to refuse a stale change.

## Modular design with Jsonnet

A project can have a `design.jsonnet` file. Jsonnet is JSON with variables, functions and `import`. The source generates `faber.json` and the part files. Placement, assembly and analysis stay plain JSON.

The source evaluates to an object with these keys:

- `name`: the project name. Optional.
- `params`: parameter expressions as strings. Optional.
- `parts`: each part id maps to `name`, `colour`, `geometry` and, optionally, `group` and `note`.

If the project has a source, the tools refuse changes to parts, features and parameters. Edit the source, then call `build`. Placement, contact, section and motion tools still work.

Call `build` to write the files. A build that fails writes no file. If an input changed, the MCP server builds the source before the next tool call. The build report lists the written files and the added or removed parts. A removed part loses its placement. An undo of a build regenerates the same recipes from the source. A build before the next tool call raises the revision. An `expected_revision` from before the source edit is refused. Call `status` and read the revision again.

Imports look first in the folder of the importing file. Then they look in `lib/` of the project folder. `import 'faber.libsonnet'` gives these helpers:

- `part(name, features, output, colour)`: a part with a feature history.
- `box`, `cylinder`, `transform`, `union`, `difference`, `linear_pattern` and `circular_pattern`: one feature each.
- `feature(id, op, fields)`: a feature for any other op.
- `merge(fragments)`: joins fragments. A part id can appear in one fragment only.
- `prefix(fragment, text)`: puts text in front of each part id. Use it for a block that appears more than once.

Example: `examples/modular-rack`. The file `lib/bracket.libsonnet` is a block. The design uses it three times:

```jsonnet
local faber = import 'faber.libsonnet';
local bracket = import 'bracket.libsonnet';

faber.merge([
  { name: 'Modular rack' },
  faber.prefix(bracket.bracket(40, 30, 3, 5), 'left_'),
  faber.prefix(bracket.bracket(60, 30, 3, 5), 'mid_'),
  faber.prefix(bracket.bracket(80, 40, 4, 6), 'right_'),
])
```

## Viewer controls

Open `examples/viewer-controls` to try the controls on an angle plate and a pin.
Click a part to select it. Shift-click adds a part. Ctrl-click adds or removes
a part. Drag from left to right to select whole parts inside the rectangle.
Drag from right to left to select parts that touch the rectangle. Right-click
a part for its tools.

| Action | Control |
| --- | --- |
| Pan | Middle drag |
| Choose a marking menu command | Right drag towards the command, then release |
| Orbit | Shift + middle drag or Alt + left drag |
| Zoom at the pointer | Wheel |
| Fit the model | F or double-click the middle button |
| Set a view | Click a face, edge or corner on the view cube |
| Fit the isometric view | Home on the view cube |
| Find a command | S, then enter its name |
| Move the selection | M |
| Show or hide the section | Shift + S |
| Select all visible parts | Ctrl + A |
| Delete the selection | Delete |
| Undo | Ctrl + Z |
| Redo | Ctrl + Y or Ctrl + Shift + Z |

Enter applies the open form. Escape first closes a menu, then stops a pick,
then cancels a tool, then clears the selection. Each press does one action.

## Materials and face meshes

Open `examples/material-samples`. The steel cube is 10 mm on each side. Its
volume is 1 cm³. At 7.85 g/cm³ its mass is 7.85 g. The pin beside the cube
has a curved side and two flat ends.

Use `materials` to find a material or read its properties and source. Assign
its id to a part with `set_part_info`. Read `mass_properties` with the part
id to use that density. A density in the request overrides the material.
A project material takes precedence over a library material with the same id.
For a design source, define project materials before the source uses them.
Use the optional `material` argument of `faber.part` to assign the id.
See [Material data](material-data.md) for the grades and test directions.
Library values are typical. For a printed part, check the material, process
and layer direction before you use a strength value.

The face mesh of a native solid gives a face index for each triangle. `faber_core::face_triangles` gives its points in world coordinates,
including the part placement. Rebuilds can change face indices. Imported
triangle meshes have no exact faces.

Use `faber_geometry::bvh::Bvh` for the first ray hit, all ray hits, the closest
surface point or an inside test. Results refer to the triangles of the input
mesh. The inside test needs a closed mesh. Ray distances use millimetres.

Use `faber_wiring::export_vector` with a vector document and a path that ends
in `.svg` or `.dxf`. Put lines, circles and arcs on named layers. Supply a
sheet size in millimetres. Coordinates start at the lower left. The SVG writer
converts the vertical coordinate to the page direction. A vector document
contains sheet geometry and text. Solid modelling history stays in the project.


## Picking and press pull

Open `examples/press-pull-block`. Set the selection filter to Face. Pick the
top face and press Q. Drag the arrow out of the block, or enter 10 mm in the
distance field. Release the arrow or press Enter to save the change. The block
is now 30 mm high. One Undo restores the original height.

A negative distance cuts into the part. Escape cancels a drag. The preview
needs a planar face on an editable solid. An imported mesh without face data
supports body selection only.

Use the Edge filter to pick an edge for a fillet or chamfer. The inspector
identifies the selected item. Hold Ctrl or Shift to select more items.
The saved feature uses a geometric selector so that it can find the same set
after the part changes size.

Right-click in the view to show the marking menu. Click a command, or hold the
right button, drag towards the command and release. The top command repeats
the last operation. Use the middle button to pan.

For MCP, use `topology` to find a planar face and `press_pull` to move it.
Supply the revision at which you read the face index and a signed distance
in millimetres. The reply gives the new revision and part volume.


## Stress study

Open `examples/shelf-bracket-study` and select the bracket. Open the stress
study panel and choose `shelf_load`. Two M5 screws pass through the
20 mm mounting pads on the back of the flange. The annular end faces of both
pads are fixed. The arm tapers from 34 mm to 10 mm. Its 12 by 10 mm bearing
face carries 100 N downwards at the end of the shelf. Run the study to see stress in
MPa, displacement in mm, the safety factor and the support reaction in N.
The deformed shape has a display scale. It does not change the recipe.

The colour scale shows von Mises stress. Use the five marked points to inspect
the regions with the highest calculated values. A fixed fixture holds all
three translations. A slide fixture holds motion normal to a planar face.
A force is the total across its selected faces in N. Positive pressure acts
inward and uses MPa. The safety factor divides the material yield stress by
the largest calculated stress. For a printed polymer,
give a print direction to also check the normal stress across the layers.

From MCP, use `quick_stress` for one fixed face and one force. To save several
fixtures or loads, use `schema` with kind `study`, then `set_study`. Use
`run_study` to solve it. Set `render.path` to save a colour map. A selector
uses part coordinates. Forces, gravity and print directions use world axes.

This linear elastic model assumes small displacement. It studies one solid
and uses the specified bulk material. It omits screw contact, preload, infill,
creep and failure after yield. The model approximates curved surfaces with
partly filled cells. A local stress at a sharp corner can change with cell
size. Compare results at two resolutions before you use the peak value.

The saved example gave these results in a release build with four solver threads.
Time includes the mesh, solve and report, and depends on the computer.

| Cell size (mm) | Active cells | Iterations | Time (s) | Peak stress (MPa) | Safety factor |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1.0454 | 29,901 | 1,553 | 3.099 | 23.92 | 2.20 |
| 0.5227 | 220,922 | 2,973 | 54.759 | 25.41 | 2.07 |

The peak stress changed by 5.89 % relative to the fine result. The display
samples the stress field across each face. Additional display triangles keep
the original surface and face indices. The display has a limit of 150,000
triangles for this subdivision. Peak values in the report use all active cells.

The first bracket had a sharp shelf junction and two fully fixed bore walls.
Its peak near the upper screw increased from 17.12 MPa to 23.86 MPa when the
cell size decreased from 0.651 mm to 0.326 mm. The difference was 28.24 %
relative to the fine result. A fixed bore wall represents a bonded joint.
The example uses separate mounting pads, blends at the flange and a tapered
shelf arm. Both grids locate its peak in the bend of the arm, about 32 mm
from the rear face of the flange.

A design source can set the `studies` map with `f.study`, `f.fixed` and
`f.force` from `faber.libsonnet`. An explicit map replaces the saved studies.
Omit the field to keep studies that you made in the project.

## Print check

Open `examples/print-check` and select the arch. Open **Print check**, select
the printer and run the check. The centre fin is 0.6 mm wide. Its lower edge
starts 13 mm above the bed. Select a finding to see its location. A wall
below the profile limit appears in the thin-wall overlay.

The report uses the placed parts. It gives the size, layer count, bed contact
and unsupported area. Support volume is a geometric estimate in cubic mm.
The slicer sets the actual support, layers and toolpath. Compare the ranked
orientations. **Apply** turns the selected part about its current bounds
centre and lowers it to the bed. Undo restores its previous placement.

For MCP, use `printers` and `set_printer`. For a custom profile, read `schema`
with `kind: "printer"`. Use `print_check` with part ids
or omit them to check the assembly. Use `orient_for_print` with a part id.
If all candidates exceed the build volume, the reply gives `fits_any: false`,
the part size, bed size and candidate sizes. Use `fit_check` to find mating
cylinders in two placed parts. Clearance is hole diameter minus shaft diameter.
Print checks and orientation searches accept `resolution_mm` from 0.001 to
1.0. This sets the chord error for exact solids. Imported meshes keep their
source triangles.
`tolerance_stack` combines signed dimensions and their positive and negative
allowances. Worst case uses all extreme values. Root-sum-square assumes
independent variation.

| Profile | Build volume (mm) | Source |
| --- | --- | --- |
| Generic 220 | 220 × 220 × 250 | Faber practice profile |
| Bambu Lab A1 | 256 × 256 × 256 | [Bambu Lab quick start](https://cdn1.bambulab.com/documentation/quick-start-a75adcb1d5d5e/Quick%20Start%20Guide%20for%20A1.pdf) |
| Bambu Lab A1 mini | 180 × 180 × 180 | [Bambu Lab quick start](https://cdn1.bambulab.com/documentation/quick-start-f507128172bdf/Quick%20start%20guide%20-%20A1%20mini-EN.pdf) |
| Original Prusa MK4 | 250 × 210 × 220 | [Prusa FAQ](https://help.prusa3d.com/article/faq-frequently-asked-questions_1932) |
| Creality Ender-3 | 220 × 220 × 250 | [Creality FAQ](https://www.creality.com/resources/faq) |

The profiles use a 0.4 mm nozzle and 0.2 mm layers. Faber uses practice
limits of two nozzle widths for walls, 45 degrees from vertical for
overhangs and 10 mm for bridges. Its example diametral clearances are
0.00 mm for press, 0.15 mm for snug, 0.25 mm for slide and 0.40 mm for
loose. Print a fit sample for your machine, material and orientation.
The part size, print direction and calibration affect the result.
[Prusa modelling guide](https://help.prusa3d.com/article/modeling-with-3d-printing-in-mind_164135).

Export with **3MF** in the viewer, the MCP `export` tool with `format: "3mf"`,
or `faber export PROJECT DIRECTORY --format 3mf`. Open `assembly.3mf` in the
slicer. The file keeps one object per part, its display colour and its build
placement in millimetres.

## Plastic features

![Snap box with ribs and screw bosses in the base and two hooks on the lid.](images/snap-box.png)

Select an editable part and open **Plastic features**. Rib, Web, Boss and
Snap hook need a reference plane in the part history. To add a plane to a
flat surface, use **References → Plane on face**. In the plastic feature
form, select **Reference plane** and choose the plane by name. Positions and
line ends use the X and Y axes of that plane. Enter lengths in millimetres
and angles in degrees. Dimension fields accept parameter expressions such
as `wall * 2`. **Apply feature** adds one step to the history.

| Feature | Inputs and effect |
| --- | --- |
| Rib | Enter a line that touches the body at both ends. The rib grows in the reference plane to the body. Set thickness, wall side and optional draft. |
| Web | Enter one or more lines. The web grows along the plane normal to the body. Set thickness and optional draft. Use **Add line** for crossing walls. |
| Boss | Set the centre, diameter and height. Optional fields add a bore, draft, radial ribs and a base fillet. The hole diameter must be smaller than the boss diameter. Each rib has a length, thickness and height. The rib count must resolve to a whole number. |
| Snap hook | Set the root, beam length, width and thickness. The beam grows along the plane normal. The head points along plane X at zero degrees. The undercut sets the deflection for the strain estimate. Set head length, tip thickness, lead angle, return angle and root fillet as needed. The default lead and return angles are 30° and 90°. |
| Lip | Pick a planar face with **Faces**. Choose the outer or inner loop and its inside or outside. Width sets the strip across the face. Height sets the projection along the normal. |
| Groove | Pick the mating face and set the loop, side, width and depth. Clearance adds space on both sides and at the bottom. A clearance of 0.2 mm adds 0.4 mm to the groove width. |

For Jsonnet, import `faber.libsonnet`. Each helper returns a feature with an
id. The second argument is the solid feature to change. This example adds a
boss on a plane named `floor` to a tray named `shell`.

```jsonnet
faber.boss('post', 'shell', 'floor', [10, 8], 7, 6,
           hole_diameter=3, hole_depth=5)
```

The other helpers are `rib`, `web`, `snap_hook`, `lip` and `groove`. Wall
lines have the form `[[[x1, y1], [x2, y2]], ...]`. A loop is `'outer'`
or `'inner'`. Its side is `'inside'` or `'outside'`. Select the feature
in the history to edit or suppress it.

Open [Snap box](../examples/snap-box/faber.json) for a 60 × 40 mm tray with
two corner ribs, four bored bosses and a lip. Its lid has a matching groove
and two hooks. Both parts lie flat beside each other for the print check.
The hooks enter the two side windows when the lid is turned over. Use
`feature_info` on a hook or `snap_check` with its dimensions to read the
strain estimate. The constant-section beam estimate uses
`1.5 × thickness × undercut / length²`. It does not model layer adhesion or
stress at the root fillet.

## Mesh parts

Open the [mesh repair example](../examples/mesh-repair/faber.json).
Right-click **Open cube** in the body list and select **Mesh report**.
This 10 mm cube has one open face and four open edges. The report uses
part coordinates. It counts shells, boundary edges, non-manifold edges,
duplicate triangles, triangles with zero area and winding errors. A closed
mesh has a signed volume. Negative volume indicates inward winding.

| Imported cube with four open edges | Repaired cube with a volume of 1000 mm³ |
| --- | --- |
| ![The inner walls are visible through the missing face.](images/mesh-repair-before.png) | ![Two triangles close the top face.](images/mesh-repair-after.png) |

Select **Repair** and **Apply repair**. With the default settings, the tool
welds coincident vertices, removes duplicate and degenerate triangles,
corrects winding and fills the missing face. The repaired cube has a volume
of 1000 mm³. Reduce **Maximum edges per hole** to keep larger openings.
Inspect the remaining defects after repair. A shared non-manifold edge needs
a modelling decision. Purchased parts permit inspection only.

| Tool | Input and result |
| --- | --- |
| Reduce | Select a triangle target, a fraction of the original count, or a maximum error in millimetres. Use a closed mesh. A target above the existing count keeps the same asset and revision. |
| Plane cut | Enter the origin in world millimetres and a non-zero world normal. The normal points towards the upper side. Select one side or both. **Cap cut faces** closes the cut loops. When keeping both, the lower side keeps the original id and the upper side gets a new id. |
| Convert to solid | Convert a closed mesh with up to 200,000 triangles. **Merge coplanar faces** makes one face from adjacent triangles on the same plane. Add a hole, fillet or other solid feature to the new exact body. |

Each edit creates an asset and one undo step when the geometry changes. Use
**Undo** to restore its previous asset. For a one-sided cut or a conversion,
enter a new part id to keep the original mesh beside the result in the tree.

For MCP, call `mesh_report` with `part`. Read the current project revision
from `status`. Pass it as `expected_revision` to `repair_mesh`,
`reduce_mesh`, `cut_mesh` or `mesh_to_solid`. A changing call returns a
log, changed part summaries and the new revision. This cut keeps both sides
of the cube and stores the upper half as `top`.

```json
{"part":"cube","expected_revision":2,"plane":{"origin":[0,0,5],"normal":[0,0,1]},"keep":"both","cap":true,"copy_as":"top"}
```

## Drawings

Use `auto_drawing` to make a sheet from the placed parts. Read the revision
with `status` first. For a project at revision 7, this call saves a
landscape A4 sheet for the part `block`.

```json
{"id":"overview","parts":["block"],"sheet":{"size":"a4","orientation":"landscape"},"title":"Block assembly","expected_revision":7}
```

Use `drawings` to list saved sheets. In the viewer, select the sheet title
under **Drawings**. Drag the sheet to pan. Scroll to zoom about the pointer.
**Fit sheet** restores the full paper view. The sheet updates when a model
change reaches the viewer. **Back to model** returns to the solid.

Drawings are in `drawings/<id>.json`. They store views and references to the
model in millimetres. To set a dimension by hand, add its definition to the
`dimensions` map and save the complete drawing with `set_drawing`. This
entry measures the horizontal distance between two part-local vertices of
a 40 mm block in the front view.

```json
{
  "width": {
    "kind": "linear", "view": "front", "along": "horizontal", "offset": 8,
    "a": {"kind": "vertex", "part": "block", "near": [0, 0, 0]},
    "b": {"kind": "vertex", "part": "block", "near": [40, 0, 0]}
  }
}
```

For the full width of a view, use `"kind": "overall"` with
`"along": "horizontal"`. The value follows the current outline when a part
grows or moves. An automatic diameter group checks each referenced circle.
If a circle disappears or its diameter changes relative to the group, the
sheet marks the dimension with a question mark.


The renderer resolves the references on each model revision. A lost
reference gives a red question mark and an entry in `problems`. Mesh-only
parts give a projected outline. The export reports this in `limits`.
First-angle and third-angle projection place the projected views on
opposite sides of their parent. A parts list groups equal resolved
geometry and shows its quantity, name, material and mass.

To save a sheet from the viewer, select **Browse…** or enter the export
path, then select **Export**. SVG, PDF, DXF and PNG are available. For MCP,
call `export_drawing` with an id and path. For PNG, set `width_px` to the
required pixel width and `inline` to true to receive the image. To export
the [trash bin overview](../examples/trash-bin/drawings/overview.json) on
the command line, run the following command.

```console
faber drawing examples/trash-bin overview overview.pdf
```

Use `schema` with `kind: "drawing"` for the full drawing definition.
`set_drawing`, `remove_drawing` and `auto_drawing` require
`expected_revision`. A Jsonnet source can provide a `drawings` map with
`faber.drawing(title, views, dimensions)`. The build updates the sheets
owned by the source and preserves sheets added separately.

## Components and instances

Use a component to place parts in an assembly. Its parent can be another
component. A part's placement is relative to its component. Moving the
parent moves its children and their parts together.

If `design.jsonnet` defines a component, edit its position, parent or name
in that file and build the project. The same rule applies to a part's
component assignment in the source. Components added separately keep
their saved frames when the source rebuilds or the configuration changes.

The browser shows the component tree. Select the arrow to expand a
component. Drag a part name onto a component to change its parent and
keep its world position. Drop it on **Components** to put it at the
assembly root. Select a component and click **Isolate** to see its
contents. **End isolate** restores the previous visibility.

A linked copy uses the geometry of its source. Select a part, open
**Move/Copy**, select **Create copy** and **Linked copy**, then position
it and click **OK**. Edit the source to update every instance.
Use `make_independent` with a made instance's part id to edit it
separately. A purchased instance keeps its source model.

Open **Parts list** to see item numbers, quantities, materials and group
masses in grams. A dash means the mass or material is unknown. Parts
with equal resolved geometry and material share one row.
`faber bom PROJECT --format csv` prints the list for a spreadsheet.
Use `bom` with a component id in `scope` for one sub-assembly.

## Configurations

A configuration sets named parameters and can suppress features. Select
its name in the toolbar to rebuild the assembly in one undo step.
`use_configuration` takes the configuration name. Values outside that
configuration keep their previous values during a switch.

The [desk riser example](../examples/structured-assembly/design.jsonnet)
has two sizes and two linked supports. The wide configuration changes
the deck span from 100 to 160 mm and the height from 35 to 50 mm.
Its Jsonnet source reads `self.params` for the component positions, so
both supports still meet the deck after a size change.

```jsonnet
local f = import 'faber.libsonnet';
{
  params: { span: '100' },
  configurations: { wide: f.configuration({span: '160'}) },
  components: { base: f.component('Base') },
  parts: {
    rail: f.part('Rail', [f.box('bar', ['span', 10, 6])], 'bar', component='base'),
    spare: f.instance('Spare rail', 'rail'),
  },
}
```

For a separate export, pass `configuration` to `export`. Use
`all_configurations: true` to write one folder for each size.
The saved project keeps its active configuration. The CLI accepts `--configuration NAME` and `--all-configurations`.
Change a configuration from `design.jsonnet` in its source file.

## Linked parts

Export a board from Lamina as a STEP file at a fixed path. Call
`import_part` with the file path, part id and `link: true`.
The saved part records the source path and file hash. A path relative to
the project folder also works when the project moves.

After a board change, export the STEP file to the same location. Call
`links` to see which files changed. Call `refresh_links` to update their
saved assets together. Each part keeps its assembly placement. If a
source file is missing, the project opens with its last saved asset.
