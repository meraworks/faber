local f = import 'faber.libsonnet';

{
  local span = std.parseInt(self.params.span),
  local height = std.parseInt(self.params.height),
  name: 'Desk riser',
  params: { span: '100', height: '35' },
  configurations: {
    compact: f.configuration({span: '100', height: '35'}, note='Standard height'),
    wide: f.configuration({span: '160', height: '50'}, note='Wide and tall'),
  },
  components: {
    riser: f.component('Desk riser'),
    top: f.component('Top plate', 'riser', translation=[0, 0, height]),
    left: f.component('Left support', 'riser'),
    right: f.component('Right support', 'riser', translation=[span-10, 0, 0]),
  },
  parts: {
    deck: f.part('Deck', [f.box('plate', [span, 44, 5])], 'plate',
                 colour=[58, 136, 145], material='steel-s235', component='top'),
    leg: f.part('Support', [f.box('bar', [10, 44, height])], 'bar',
                colour=[120, 131, 139], material='steel-s235', component='left'),
    other_leg: f.instance('Support', 'leg', component='right',
                          colour=[120, 131, 139], material='steel-s235'),
  },
}
