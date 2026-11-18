// A plate with a bore. Each call gives one part, with the id `bracket`.
local faber = import 'faber.libsonnet';

{
  // Sizes are in millimetres. The bore is at the centre of the plate.
  bracket(width, depth, thickness, bore)::
    {
      parts: {
        bracket: faber.part('Bracket', [
          faber.box('plate', [width, depth, thickness]),
          faber.cylinder('bore_tool', bore / 2, thickness * 3),
          faber.transform('bore', 'bore_tool', [width / 2, depth / 2, -thickness]),
          faber.difference('bracket_body', 'plate', 'bore'),
        ], 'bracket_body', [200, 120, 60]),
      },
    },
}
