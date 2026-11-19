local f = import 'faber.libsonnet';
local edge(id, source, centre, radius) =
  f.feature(id, 'fillet', {
    source: source,
    edges: { near_centre: centre, tolerance: 0.001, expect: 1 },
    radius: radius,
  });

{
  name: 'Surface inspection block',
  params: { inside_radius: '1.5', outside_radius: '2' },
  parts: {
    block: f.part('Counterbored block', [
      f.box('blank', [60, 42, 14]),
      edge('front_round', 'blank', [30, 0, 14], 'outside_radius'),
      f.cylinder('drill', 5, 16),
      f.transform('drill_at', 'drill', [36, 21, -1]),
      f.difference('through_bore', 'front_round', 'drill_at'),
      f.cylinder('counterbore', 11, 8),
      f.transform('pocket_at', 'counterbore', [36, 21, 7]),
      f.difference('pocket', 'through_bore', 'pocket_at'),
      f.feature('floor_round', 'fillet', {
        source: 'pocket',
        edges: { radius: 11, in_box: [[24, 9, 6.99], [48, 33, 7.01]], tolerance: 0.001, expect: 1 },
        radius: 'inside_radius',
      }),
    ], 'floor_round', colour=[89, 157, 164]),
    deck: f.part('Access bridge', [
      f.box('post', [12, 3, 8]),
      f.transform('front', 'post', [7, 7, 14]),
      f.transform('back', 'post', [7, 32, 14]),
      f.box('span', [12, 28, 3]),
      f.transform('span_at', 'span', [7, 7, 22]),
      f.union('front_span', 'front', 'span_at'),
      f.union('bridge', 'front_span', 'back'),
    ], 'bridge', colour=[196, 147, 84]),
  },
}
