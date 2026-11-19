local f = import 'faber.libsonnet';
local line(a,b) = {kind:'line',a:a,b:b};
local distance(axis,a,b,value) = {kind:'distance_'+axis,a:a,b:b,value:value};
local profile = f.solved_sketch('outline','base_plane',
  {a:[0,0],b:[60,0],c:[60,36],d:[0,36],left:[10,18],right:[50,18]},
  {bottom:line('a','b'),side:line('b','c'),top:line('c','d'),end:line('d','a'),
   left_hole:{kind:'circle',centre:'left',radius:3},right_hole:{kind:'circle',centre:'right',radius:3}},
  [
    {kind:'fix',point:'a'},
    {kind:'horizontal',line:'bottom'},{kind:'vertical',line:'side'},
    {kind:'horizontal',line:'top'},{kind:'vertical',line:'end'},
    distance('x','a','b','plate_width'),distance('y','a','d','plate_height'),
    distance('x','a','left','hole_margin'),distance('y','a','left','plate_height / 2'),
    distance('x','a','right','plate_width - hole_margin'),distance('y','a','right','plate_height / 2'),
    {kind:'radius',entity:'left_hole',value:'hole_radius'},
    {kind:'equal',a:'left_hole',b:'right_hole'},
  ]);
{
  name:'Constrained mounting plate',
  params:{plate_width:'60',plate_height:'36',hole_radius:'3',hole_margin:'10',base_height:'4',inset:'2'},
  parts:{
    plate:f.part('Stepped mounting plate',[
      f.feature('base_plane','plane',{placement:{translation:[0,0,0],rotation:[0,0,0]}}),
      profile,
      f.feature('base','extrude_sketch',{source:'outline',vector:[0,0,'base_height']}),
      f.offset_sketch('shoulder_outline','outline','-inset'),
      f.feature('shoulder','extrude_sketch',{source:'shoulder_outline',vector:[0,0,'base_height + 2']}),
      f.union('plate','base','shoulder'),
    ],'plate',colour=[76,143,157]),
  },
}
