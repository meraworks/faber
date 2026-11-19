struct Scene {
    vp: mat4x4<f32>, eye: vec4<f32>, settings: vec4<f32>, background: vec4<f32>,
    shadow_frame: vec4<f32>, section: vec4<f32>, cut_style: vec4<f32>,
};
@group(0) @binding(0) var<uniform> scene: Scene;
@group(1) @binding(0) var mask: texture_2d<f32>;
@group(1) @binding(1) var smooth_mask: sampler;
struct Input {
    @location(0) pos: vec3<f32>, @location(1) n: vec3<f32>, @location(2) base:vec3<f32>, @location(3) surface:vec3<f32>,
    @location(4) x:vec4<f32>, @location(5) y:vec4<f32>, @location(6) z:vec4<f32>, @location(7) origin:vec4<f32>, @location(8) flags:vec4<f32>,
};
struct Out {
    @builtin(position) p: vec4<f32>, @location(0) world:vec3<f32>, @location(1) n:vec3<f32>, @location(2) base:vec3<f32>,
    @location(3) surface:vec3<f32>, @location(4) flags:vec2<f32>,
};
const KEY = vec3<f32>(-0.48,-0.58,0.72);
@vertex fn vs(v:Input)->Out {
    let transform = mat4x4<f32>(v.x,v.y,v.z,v.origin);
    var o:Out;
    o.world = (transform*vec4(v.pos,1.)).xyz;
    o.p = scene.vp*vec4(o.world,1.);
    o.n = (transform*vec4(v.n,0.)).xyz;
    o.base = v.base;
    o.surface = v.surface;
    o.flags = v.flags.xy;
    return o;
}
fn mask_uv(p:vec2<f32>)->vec2<f32> {
    return (p-scene.shadow_frame.xy)/scene.shadow_frame.z+vec2(.5);
}
// Project the caster onto the horizontal ground along the key light.
@vertex fn shadow_vs(v:Input)->Out {
    let t = mat4x4<f32>(v.x,v.y,v.z,v.origin);
    let p = (t*vec4(v.pos,1.)).xyz;
    let uv = mask_uv(p.xy-KEY.xy*(p.z-scene.settings.x)/KEY.z);
    var o:Out;
    o.p = vec4(2.*uv.x-1.,1.-2.*uv.y,0.,1.);
    o.world = p;
    o.flags = v.flags.xy;
    return o;
}
fn clipped(v:Out)->bool {
    return v.flags.x>0.5 && dot(vec4(v.world,1.),scene.section)>0.;
}
@fragment fn shadow_fs(v:Out)->@location(0) vec4<f32> {
    if clipped(v) {
        discard;
    }
    return vec4(1.);
}
// A soft box and a strip light reflect in the sky gradient.
fn sky(r:vec3<f32>)->vec3<f32> {
    let gradient = mix(vec3(.27,.30,.35),vec3(.66,.77,.98),clamp(r.z*.5+.5,0.,1.));
    let offset1 = vec2((r.x + .42) * 5.5, (r.z - .58) * 3.4);
    let offset2 = vec2((r.y - .73) * 8., (r.z - .2) * 3.5);
    let strip1 = exp(-dot(offset1, offset1));
    let strip2 = exp(-dot(offset2, offset2));
    return gradient*.68+vec3(4.,3.7,3.3)*strip1+vec3(1.5,1.85,2.3)*strip2;
}
// GGX distribution with correlated Smith visibility and Schlick Fresnel.
fn light(n:vec3<f32>,v:vec3<f32>,l:vec3<f32>,rough:f32,base:vec3<f32>,metal:f32,colour:vec3<f32>)->vec3<f32> {
    let h = (v + l) * inverseSqrt(max(dot(v + l, v + l), 1e-12));
    let nl = max(dot(n,l),0.);
    let nv = max(dot(n,v),0.001);
    let nh = clamp(dot(n,h),0.,1.);
    let vh = clamp(dot(v,h),0.,1.);
    let a = max(rough*rough,.008);
    let a2 = a*a;
    let denom = nh*nh*(a2-1.)+1.;
    let distribution = a2/(3.14159265*max(denom*denom,.00000001));
    let vis = .5/max(nl*sqrt(nv*nv*(1.-a2)+a2)+nv*sqrt(nl*nl*(1.-a2)+a2),.0001);
    let f0 = mix(vec3(.04),base,metal);
    let f = f0+(1.-f0)*pow(1.-vh,5.);
    return colour*nl*((1.-f)*(1.-metal)*base/3.14159265+distribution*vis*f);
}
@fragment fn fs(v:Out,@builtin(front_facing) front:bool)->@location(0) vec4<f32> {
    if clipped(v) {
        discard;
    }
    let n = normalize(select(-v.n,v.n,front));
    let eye = normalize(scene.eye.xyz-scene.eye.w*v.world);
    let rough = clamp(v.surface.x,0.035,1.);
    let metal = v.surface.y;
    let occ = clamp(v.surface.z,.05,1.);
    let nv = clamp(dot(n,eye),0.,1.);
    let reflected = reflect(-eye,n);
    let fresnel = mix(vec3(.04),v.base,metal);
    let env = mix(sky(reflected),vec3(.43,.46,.54),rough*rough);
    let bounce = v.base*(1.-metal)*(.32+.13*n.z)+env*(fresnel+(max(vec3(1.-rough),fresnel)-fresnel)*pow(1.-nv,5.));
    let key = light(n,eye,normalize(KEY),rough,v.base,metal,vec3(3.5,3.28,2.97));
    let fill = light(n,eye,normalize(vec3(.68,-.08,.6)),rough,v.base,metal,vec3(1.2,1.45,1.72));
    let rim = light(n,eye,normalize(vec3(.22,.79,.44)),rough,v.base,metal,vec3(2.0,2.14,2.34));
    var c = (key+fill+rim+bounce)*pow(occ,1.45)*scene.settings.y;
    c = c/(vec3(.8)+c);
    if v.flags.y>.5 {
        c = mix(c,vec3(.015,.62,.75),.22);
    }
    return vec4(c,1.);
}
@vertex fn floor_vs(@builtin(vertex_index) i:u32)->Out {
    let coords = array<vec2<f32>,6>(vec2(-1.,-1.),vec2(1.,-1.),vec2(1.,1.),vec2(-1.,-1.),vec2(1.,1.),vec2(-1.,1.));
    var o:Out;
    o.world = vec3(scene.shadow_frame.xy+coords[i]*scene.shadow_frame.w,scene.settings.x);
    o.p = scene.vp*vec4(o.world,1.);
    return o;
}
@fragment fn floor_fs(v:Out)->@location(0) vec4<f32> {
    let uv = mask_uv(v.world.xy);
    var blocked = 0.;
    // Average the silhouette over a disc to soften the ground shadow.
    for(var i = 0u;i<32u;i++) {
        let fi = f32(i)+.5;
        let r = sqrt(fi/32.)*.012;
        let q = uv+vec2(cos(fi*2.399963),sin(fi*2.399963))*r;
        let ink = textureSampleLevel(mask,smooth_mask,clamp(q,vec2(0.),vec2(1.)),0.).r;
        blocked+= select(0.,ink,all(q>=vec2(0.))&&all(q<=vec2(1.)));
    }
    let fade = exp(-2.*dot(uv-vec2(.5),uv-vec2(.5)));
    let floor = mix(scene.background.xyz,scene.background.xyz*.8+vec3(.16),fade);
    return vec4(floor*(1.-blocked*.67/32.),1.);
}
struct Cap {
    @location(0) pos:vec3<f32>,@location(1) n:vec3<f32>,@location(2) colour:vec3<f32>,@location(3) kind:f32
};
@vertex fn cap_vs(v:Cap)->Out {
    var o:Out;
    o.p = scene.vp*vec4(v.pos,1.);
    o.p.z -= 0.0000003 * o.p.w;
    o.world = v.pos;
    o.n = v.n;
    o.base = v.colour;
    o.surface = vec3(.65,0.,1.);
    return o;
}
@fragment fn cap_fs(v:Out)->@location(0) vec4<f32> {
    let diagonal = (v.p.x+v.p.y)*0.70710678/9.;
    let aa = max(fwidth(diagonal),0.001);
    let line = (1. - smoothstep(.065-aa*.5,.065+aa*.5,abs(fract(diagonal)-.5))) * scene.cut_style.w;
    let component = mix(v.base,vec3(1.),.12+.12*(1.-smoothstep(.02,.25,max(v.base.r,max(v.base.g,v.base.b)))));
    let face = select(component,scene.cut_style.xyz,scene.cut_style.x>=0.);
    let distance = v.n / max(fwidth(v.n),vec3(.00001));
    let outline = 1.-smoothstep(.3,1.2,min(distance.x,min(distance.y,distance.z)));
    return vec4(mix(face,vec3(.018),max(outline,line*.94)),1.);
}
