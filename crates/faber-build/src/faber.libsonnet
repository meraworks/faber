// Helpers for Faber design sources.
//
//   local faber = import 'faber.libsonnet';
//
// A design source evaluates to an object with `parts`, and can have `name`
// and `params`. A fragment is an object with `parts`. A block is a function
// that returns a fragment. `merge` joins fragments.
{
  local faber = self,

  // A part with a feature history. `features` is a list of features. `output`
  // is the id of the feature that gives the solid.
  part(name, features, output, colour=[150, 150, 150], material=null, component=null)::
    {
      name: name,
      colour: colour,
      geometry: { op: 'history', features: features, output: output },
    } + (if material == null then {} else { material: material })
      + (if component == null then {} else { component: component }),

  component(name, parent=null, translation=[0,0,0], rotation=[0,0,0], note='')::
    { name: name, placement: {translation: translation, rotation: rotation}, note: note }
    + (if parent == null then {} else {parent: parent}),

  instance(name, of, component=null, colour=[150,150,150], material=null, note='')::
    { name: name, geometry: {op:'instance', of: of}, colour: colour, note: note }
    + (if component == null then {} else {component: component})
    + (if material == null then {} else {material: material}),

  configuration(params={}, suppressed=[], note='')::
    { params: params, suppressed: suppressed, note: note },

  // A sheet with model references. Positions and offsets are in millimetres.
  drawing(title, views={}, dimensions={}, sheet={size:'a4',orientation:'landscape'},
          projection='third', scale=1, notes=[], title_block={}, parts_list=null)::
    { title: title, views: views, dimensions: dimensions, sheet: sheet,
      projection: projection, scale: scale, notes: notes, title_block: title_block }
    + (if parts_list == null then {} else {parts_list: parts_list}),

  // A static study of one part. A face selector uses part coordinates.
  study(part, fixtures, loads, material=null, resolution_mm=null)::
    { part: part, fixtures: fixtures, loads: loads }
    + (if material == null then {} else { material: material })
    + (if resolution_mm == null then {} else { resolution_mm: resolution_mm }),

  fixed(face):: { kind: 'fixed', face: face },
  force(face, newtons):: { kind: 'force', face: face, newtons: newtons },

  // A feature with an id, an operation and its fields.
  feature(id, op, fields)::
    { id: id, geometry: { op: op } + fields },

  // A sketch with editable dimensions. Its coordinates lie in the named plane.
  solved_sketch(id, plane, points, entities, constraints=[], construction=[])::
    faber.feature(id, 'solved_sketch', {plane: plane, sketch: {
      points: points, entities: entities, constraints: constraints, construction: construction,
    }}),

  offset_sketch(id, sketch, distance)::
    faber.feature(id, 'offset_sketch', {sketch: sketch, distance: distance}),

  // A box at the origin with the corner at the origin. `size` is [x, y, z].
  box(id, size)::
    faber.feature(id, 'box', { size: size }),

  // A cylinder on the Z axis. Its base is at the origin.
  cylinder(id, radius, height)::
    faber.feature(id, 'cylinder', { radius: radius, height: height }),

  // A copy of a feature, moved and turned. `rotation` is in degrees.
  transform(id, source, translation, rotation=[0, 0, 0])::
    faber.feature(id, 'transform', { source: source, placement: { translation: translation, rotation: rotation } }),

  // The union of two features.
  union(id, left, right)::
    faber.feature(id, 'union', { left: left, right: right }),

  // The first feature with the second feature removed.
  difference(id, left, right)::
    faber.feature(id, 'difference', { left: left, right: right }),

  // `count` copies of a feature, each `step` apart. `step` is [x, y, z].
  linear_pattern(id, source, count, step)::
    faber.feature(id, 'linear_pattern', { source: source, count: count, step: step }),

  // `count` copies of a feature around an axis through `origin`. Angles are in degrees.
  circular_pattern(id, source, count, origin, axis, angle)::
    faber.feature(id, 'circular_pattern', { source: source, count: count, origin: origin, axis: axis, angle: angle }),

  // Wall lines are pairs of points in the reference plane.
  rib(id, body, plane, lines, thickness, side='symmetric', draft_deg=null)::
    faber.feature(id, 'rib', { body: body, plane: plane, lines: lines, thickness: thickness, side: side }
      + (if draft_deg == null then {} else { draft_deg: draft_deg })),

  web(id, body, plane, lines, thickness, draft_deg=null)::
    faber.feature(id, 'web', { body: body, plane: plane, lines: lines, thickness: thickness }
      + (if draft_deg == null then {} else { draft_deg: draft_deg })),

  // The boss and hook start at a point in the reference plane.
  boss(id, body, plane, at, diameter, height, hole_diameter=null, hole_depth=null,
       draft_deg=null, ribs=null, fillet=null)::
    faber.feature(id, 'boss', { body: body, plane: plane, at: at, diameter: diameter, height: height }
      + { [kv[0]]: kv[1] for kv in [
          ['hole_diameter', hole_diameter], ['hole_depth', hole_depth],
          ['draft_deg', draft_deg], ['ribs', ribs], ['fillet', fillet],
        ] if kv[1] != null }),

  snap_hook(id, body, plane, at, angle_deg, length, width, thickness, undercut, hook_length,
            tip_thickness=null, lead_deg=null, return_deg=null, root_fillet=null)::
    faber.feature(id, 'snap_hook', { body: body, plane: plane, at: at, angle_deg: angle_deg,
        length: length, width: width, thickness: thickness, undercut: undercut, hook_length: hook_length }
      + { [kv[0]]: kv[1] for kv in [
          ['tip_thickness', tip_thickness], ['lead_deg', lead_deg],
          ['return_deg', return_deg], ['root_fillet', root_fillet],
        ] if kv[1] != null }),

  // The loop is outer or inner. The side is inside or outside that loop.
  lip(id, body, face, loop, side, width, height)::
    faber.feature(id, 'lip', { body: body, face: face, loop: loop, side: side, width: width, height: height }),

  groove(id, body, face, loop, side, width, depth, clearance)::
    faber.feature(id, 'groove', { body: body, face: face, loop: loop, side: side, width: width, depth: depth, clearance: clearance }),

  remove_faces(id, body, faces)::
    faber.feature(id, 'remove_faces', { body: body, faces: faces }),

  move_faces(id, body, faces, distance)::
    faber.feature(id, 'move_faces', { body: body, faces: faces, distance: distance }),

  split_face(id, body, face, tool)::
    faber.feature(id, 'split_face', { body: body, face: face, tool: tool }),

  emboss(id, body, face, sketch, depth, mode='raise', at=null)::
    faber.feature(id, 'emboss', { body: body, face: face, sketch: sketch, depth: depth, mode: mode }
      + (if at == null then {} else { at: at })),

  // Join fragments. Each part id must be unique. The later fragment keeps
  // the other fields. Refuses a part id that two fragments have.
  merge(fragments)::
    local ids = std.flattenArrays([std.objectFields(std.get(f, 'parts', {})) for f in fragments]);
    local dup = std.set([i for i in ids if std.count(ids, i) > 1]);
    assert std.length(dup) == 0 : 'more than one fragment has these parts: ' + std.join(', ', dup);
    std.foldl(function(acc, f) acc + f + { parts: std.get(acc, 'parts', {}) + std.get(f, 'parts', {}) }, fragments, {}),

  // Put the text in front of each part id of a fragment. Use it for a block
  // that appears more than once.
  prefix(fragment, text)::
    fragment + { parts: { [text + id]: fragment.parts[id] for id in std.objectFields(std.get(fragment, 'parts', {})) } },
}
