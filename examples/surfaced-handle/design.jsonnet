local f = import 'faber.libsonnet';
local ring(id, r, z) = f.feature(id, 'sketch', {
  profile: {outer: {kind: 'circle', centre: [0,0], radius: r}},
  placement: {translation: [0,0,z], rotation: [0,0,0]},
});
local cap(id, rim) = f.patch(id, [{kind: 'curve', feature: rim}]);
{
  name: 'Surfaced handle',
  params: {wall: '1.5'},
  parts: {grip: f.part('Hollow handle', [
    ring('foot', 10, 0), ring('waist', 13, 30), ring('crown', 16, 40),
    f.surface_loft('lower_skin', ['foot','waist'], ruled=true),
    f.surface_loft('upper_skin', ['waist','crown'], ruled=true),
    cap('bottom', 'foot'), cap('top', 'crown'),
    f.stitch('closed', ['lower_skin','upper_skin','bottom','top'], as_solid=true),
    f.feature('hollow', 'shell', {source: 'closed', thickness: 'wall',
      faces: {normal: [0,0,-1], tolerance: 0.001, expect: 1}}),
  ], 'hollow', colour=[112,163,178])},
}
