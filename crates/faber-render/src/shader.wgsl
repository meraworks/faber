struct Uniform { matrix:mat4x4<f32>, eye:vec4<f32>, plane:vec4<f32>, selected:vec4<u32>, hovered:vec4<u32>, section_style:vec4<f32>, stress_ramp:array<vec4<f32>,4> }
@group(0) @binding(0) var<uniform> camera:Uniform;
struct Input { @location(0) position:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) colour:vec3<f32>, @location(3) kind:f32, @location(4) mx:vec4<f32>,@location(5) my:vec4<f32>,@location(6) mz:vec4<f32>,@location(7) mw:vec4<f32>, @location(8) face_mark:u32 }
struct Output { @builtin(position) clip:vec4<f32>, @location(0) world:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) colour:vec3<f32>, @location(3) kind:f32 }
@vertex fn vs_main(v:Input, @builtin(vertex_index) index:u32) -> Output {
    var o:Output;
    let model=mat4x4<f32>(v.mx,v.my,v.mz,v.mw);
    let world=model*vec4(v.position,1.0);
    o.clip=camera.matrix*world;o.world=world.xyz;o.normal=(model*vec4(v.normal,0.0)).xyz;o.colour=v.colour;o.kind=v.kind;
    if v.kind < -0.5 {return o;}
    // Keep the cut face visible where it meets a coplanar face.
    if v.kind>1.5 {o.clip.z-=0.0000003*o.clip.w;}
    let limits=select(camera.selected.xy,camera.selected.zw,v.kind>1.5);
    let hover=select(camera.hovered.xy,camera.hovered.zw,v.kind>1.5);
    if (index>=hover.x && index<hover.y) || v.face_mark==1u {o.colour=mix(v.colour,vec3(0.55,0.78,0.83),0.35);}
    if (index>=limits.x && index<limits.y) || v.face_mark==2u {o.colour=pow(vec3(126.0,197.0,197.0)/255.0,vec3(2.2));}
    return o;
}
@fragment fn fs_main(v:Output,@builtin(front_facing) front:bool) -> @location(0) vec4<f32> {
    // Screen-space diagonal, one unit per 9 pixels, for the cut hatch.
    let diagonal=(v.clip.x+v.clip.y)*0.70710678/9.0;
    let aa=max(fwidth(diagonal),0.001);
    if v.kind>0.5 && v.kind<1.5 {
        // Hide the floor when the camera is below it.
        if camera.eye.z<v.world.z {discard;}
        let p=v.world.xy/25.0;
        let grid=abs(fract(p-0.5)-0.5)/max(fwidth(p),vec2(0.001));
        let line=1.0-min(min(grid.x,grid.y),1.0);
        let fade=exp(-length(v.world.xy)/750.0);
        let shadow=1.0-0.08*exp(-dot(v.world.xy,v.world.xy)/22000.0);
        return vec4(v.colour*shadow-vec3(0.06)*line*fade,1.0);
    }
    if v.kind<0.5 && dot(camera.plane,vec4(v.world,1.0))>0.00001 {discard;}
    if v.kind < -0.5 {
        let t=clamp(v.colour.x,0.0,1.0)*3.0;
        let i=min(u32(t),2u);
        let srgb=mix(camera.stress_ramp[i].rgb,camera.stress_ramp[i+1u].rgb,t-f32(i));
        let linear=select(srgb/12.92,pow((srgb+0.055)/1.055,vec3(2.4)),srgb>vec3(0.04045));
        return vec4(linear,1.0);
    }
    var n=normalize(v.normal);
    if !front {n=-n;}
    let light=normalize(vec3(-0.4,-0.7,1.0));
    let fill=normalize(vec3(0.8,0.3,0.4));
    let eye=normalize(camera.eye.xyz-v.world);
    let half_vector=normalize(light+eye);
    let lower_fill=normalize(vec3(-0.2,0.4,-1.0));
    let diffuse=0.55+0.48*max(dot(n,light),0.0)+0.2*max(dot(n,fill),0.0)+0.24*max(dot(n,lower_fill),0.0);
    let value=dot(v.colour,vec3(0.2126,0.7152,0.0722));
    let specular=(0.009+0.055*sqrt(value))*pow(max(dot(n,half_vector),0.0),14.0);
    var colour=v.colour*diffuse+vec3(specular);
    if v.kind>1.5 {
        let edge=abs(fract(diagonal)-0.5);
        let line=(1.0-smoothstep(0.065-aa*0.5,0.065+aa*0.5,edge))*camera.section_style.w;
        let component=mix(v.colour,vec3(1.0),0.12+0.12*(1.0-smoothstep(0.02,0.25,max(v.colour.r,max(v.colour.g,v.colour.b)))));
        let face=select(component,camera.section_style.xyz,camera.section_style.x>=0.0);
        let pixels=v.normal/max(fwidth(v.normal),vec3(0.00001));
        let outline=1.0-smoothstep(0.3,1.2,min(pixels.x,min(pixels.y,pixels.z)));
        colour=mix(face,vec3(0.018),max(outline,line*0.94));
    }
    return vec4(colour,1.0);
}
