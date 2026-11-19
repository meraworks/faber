local f = import 'faber.libsonnet';

{
  name: 'Label bottle',
  params: { relief: '1.5', letter_height: '10' },
  parts: {
    bottle: f.part('Label bottle', [
      f.feature('section', 'sketch', {
        placement: { translation: [0, 0, 0], rotation: [90, 0, 0] },
        profile: {
          outer: { kind: 'polygon', points: [
            [0, 0], [27, 0], [27, 62], [12, 72], [12, 84],
            [10, 84], [10, 72], [25, 61], [25, 2], [0, 2],
          ] },
          holes: [],
        },
      }),
      f.feature('bottle_wall', 'revolve_sketch', {
        source: 'section', origin: [0, 0, 0], axis: [0, 0, 1], angle: 360,
      }),
      f.feature('lettering', 'sketch', {
        profile: {
          outer: { kind: 'text', text: 'FABER', height: 'letter_height', at: [0, 0], align: 'centre' },
          holes: [],
        },
        placement: { translation: [0, 0, 0], rotation: [0, 0, 0] },
      }),
      f.emboss('raised_label', 'bottle_wall', {cylinder_radius: 27, internal: false, tolerance: 0.000001, expect: 1},
               'lettering', 'relief', at=[-24, 29]),
    ], 'raised_label', colour=[207, 156, 91]),
  },
}
