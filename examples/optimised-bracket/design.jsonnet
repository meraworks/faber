local f = import 'faber.libsonnet';
local end(x) = { normal: [x, 0, 0], tolerance: 0.001, expect: 1 };
{
  name: 'Optimised shelf bracket',
  parts: {
    'bracket-shape': f.saved_mesh('Optimised bracket', '04ef4e4e08f8c8fe8f3329f068d49629ff07561f2778c753cfefc826ccfac469', colour=[214, 152, 67], material='steel-s235'),
    bracket: f.part('Bracket stock', [f.box('stock', [48, 16, 12])], 'stock',
                    material='steel-s235', colour=[72, 132, 156]),
  },
  studies: {
    reduce_stock: f.shape_study(
      'bracket', [f.fixed(end(-1))], [f.force(end(1), [0, 0, -40])],
      target_volume=0.55, min_member_mm=4, resolution_mm=2
    ),
  },
}
