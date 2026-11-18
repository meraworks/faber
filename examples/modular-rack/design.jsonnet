// A rack of three brackets. Each bracket has its own size.
local faber = import 'faber.libsonnet';
local bracket = import 'bracket.libsonnet';

faber.merge([
  { name: 'Modular rack' },
  faber.prefix(bracket.bracket(40, 30, 3, 5), 'left_'),
  faber.prefix(bracket.bracket(60, 30, 3, 5), 'mid_'),
  faber.prefix(bracket.bracket(80, 40, 4, 6), 'right_'),
])
