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
  part(name, features, output, colour=[150, 150, 150], material=null)::
    {
      name: name,
      colour: colour,
      geometry: { op: 'history', features: features, output: output },
    } + (if material == null then {} else { material: material }),

  // A feature with an id, an operation and its fields.
  feature(id, op, fields)::
    { id: id, geometry: { op: op } + fields },

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
