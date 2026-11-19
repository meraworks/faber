local f = import 'faber.libsonnet';
local rule = { thickness: 'stock', bend_radius: 2, k_factor: 0.44 };
local hole(at) = {kind: 'circle', centre: at, radius: 2.5};
local base = {
  outer: {kind: 'rectangle', origin: [0, 0], size: [50, 40]},
  holes: [hole([15, 10]), hole([15, 30])],
};
local lip = f.flange('mount', 'base', 2, 30, holes=[hole([20, 18])]);
{
  name: 'Sheet bracket',
  params: {stock: '2'},
  parts: {bracket: f.part('Mounting bracket', [f.sheet('fold', rule, base, [lip])],
                          'fold', colour=[142, 167, 181], material='steel-s235')},
}
