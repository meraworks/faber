use serde_json::{Value, json};

pub fn operations() -> Vec<Value> {
    vec![
        json!({"tool":"set_param","name":"span","expression":"150"}),
        json!({"tool":"put_part","id":"nose","part":{
            "name":"Ogive nose","colour":[170,190,205],
            "geometry":{"op":"history","output":"wall","features":[
                {"id":"profile","geometry":{"op":"sketch","placement":{"rotation":[90,0,0]},"profile":{"outer":{"kind":"path","start":[0,0],"segments":[
                    {"kind":"line","end":[20,0]},
                    {"kind":"arc","via":["sqrt(9100)-80",30],"end":[0,60]},
                    {"kind":"line","end":[0,0]}
                ]}}}},
                {"id":"outer","geometry":{"op":"revolve_sketch","source":"profile","origin":[0,0,0],"axis":[0,0,1],"angle":360}},
                {"id":"wall","geometry":{"op":"shell","source":"outer","faces":{"near_centre":[0,0,0],"tolerance":0.000001,"expect":1},"thickness":2}}
            ]}
        }}),
        json!({"tool":"put_part","id":"ring","part":{
            "name":"Pocketed ring","colour":[190,155,100],"placement":{"translation":[100,0,0]},
            "geometry":{"op":"history","output":"pockets","features":[
                {"id":"profile","geometry":{"op":"sketch","placement":{"rotation":[90,0,0]},"profile":{"outer":{"kind":"rectangle","origin":[30,0],"size":[10,10]}}}},
                {"id":"ring","geometry":{"op":"revolve_sketch","source":"profile","origin":[0,0,0],"axis":[0,0,1],"angle":360}},
                {"id":"drill","geometry":{"op":"cylinder","radius":2,"height":6}},
                {"id":"raised","geometry":{"op":"transform","source":"drill","placement":{"translation":[35,0,5]}}},
                {"id":"tools","geometry":{"op":"circular_pattern","source":"raised","count":4,"origin":[0,0,0],"axis":[0,0,1],"angle":90}},
                {"id":"pockets","geometry":{"op":"difference","left":"ring","right":"tools"}}
            ]}
        }}),
        json!({"tool":"put_part","id":"wing","part":{
            "name":"Tapered twisted wing","colour":[105,155,170],"placement":{"translation":[180,0,0]},
            "note":"Two symmetric Bezier sections. This example has no aerodynamic validation.",
            "geometry":{"op":"history","output":"loft","features":[
                {"id":"root","geometry":{"op":"sketch","profile":{"outer":airfoil(100.,8.)}}},
                {"id":"tip","geometry":{"op":"sketch","placement":{"translation":[10,0,"span"],"rotation":[0,0,-5]},"profile":{"outer":airfoil(60.,4.8)}}},
                {"id":"loft","geometry":{"op":"loft_sketches","sources":["root","tip"],"ruled":true}}
            ]}
        }}),
    ]
}

fn airfoil(chord: f64, control_height: f64) -> Value {
    json!({"kind":"path","start":[0,0],"segments":[
        {"kind":"bezier","control1":[0,control_height],"control2":[chord/2.,control_height],"end":[chord,0]},
        {"kind":"bezier","control1":[chord/2.,-control_height],"control2":[0,-control_height],"end":[0,0]}
    ]})
}
