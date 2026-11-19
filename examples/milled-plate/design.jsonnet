local f = import 'faber.libsonnet';
local centres = [[7,7],[53,7],[53,33],[7,33]];
local holes = std.flattenArrays([
  local previous = if i == 0 then 'pocket' else 'drilled_' + std.toString(i);
  local n = std.toString(i + 1);
  [f.transform('position_' + n, 'hole_tool', centres[i] + [-1]),
   f.difference('drilled_' + n, previous, 'position_' + n)]
  for i in std.range(0,3)
]);
{
  name: 'Milled plate',
  params: {thickness: '8'},
  machining: f.machining(
    tools={
      end_mill: f.cutting_tool('flat', 1, 6, 18, 2, 400, 100, 6000),
      drill: f.cutting_tool('drill', 2, 5, 18, 2, 250, 90, 6000),
    },
    setups={top: f.machining_setup('plate', {kind:'offset', sides:3, top:1, bottom:0}, 'stock_top_corner')},
    operations=[
      f.machining_operation('face', 'top', 'end_mill', 'face',
        f.machining_heights(0,-1), {stepover:3}, stepdown=1),
      f.machining_operation('pocket', 'top', 'end_mill', 'pocket',
        f.machining_heights(-1,-4), {floor:{z:-4}, stepover:2.5}, stepdown=1.5),
      f.machining_operation('holes', 'top', 'drill', 'drill',
        f.machining_heights(-1,-9), {holes:'auto', peck:2}, stepdown=2),
      f.machining_operation('outside', 'top', 'end_mill', 'contour',
        f.machining_heights(-1,-9), {z:{z:-6}, side:'outside'}, stepdown=2),
    ],
  ),
  parts: {
    plate: f.part('Milled plate', [
      f.box('blank', [60,40,'thickness']),
      f.box('pocket_tool', [24,14,5]),
      f.transform('pocket_position', 'pocket_tool', [18,13,'thickness - 3']),
      f.difference('pocket', 'blank', 'pocket_position'),
      f.cylinder('hole_tool', 2.5, 'thickness + 2'),
    ] + holes, 'drilled_4', colour=[147,164,168]),
  },
}
