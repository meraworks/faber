struct Uniform { matrix:mat4x4<f32>, eye:vec4<f32> }
@group(0) @binding(0) var<uniform> camera:Uniform;
struct Input { @location(0) position:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) colour:vec3<f32>, @location(3) kind:f32 }
struct Output { @builtin(position) clip:vec4<f32>, @location(0) world:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) colour:vec3<f32>, @location(3) kind:f32 }
@vertex fn vs_main(v:Input) -> Output {
    var o:Output;
    o.clip=camera.matrix*vec4(v.position,1.0);o.world=v.position;o.normal=v.normal;o.colour=v.colour;o.kind=v.kind;
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
        let shadow=1.0-0.45*exp(-dot(v.world.xy,v.world.xy)/22000.0);
        return vec4(v.colour*shadow+vec3(0.016)*line*fade,1.0);
    }
    var n=normalize(v.normal);
    if !front {n=-n;}
    let light=normalize(vec3(-0.4,-0.7,1.0));
    let fill=normalize(vec3(0.8,0.3,0.4));
    let eye=normalize(camera.eye.xyz-v.world);
    let half_vector=normalize(light+eye);
    let diffuse=0.34+0.62*max(dot(n,light),0.0)+0.22*max(dot(n,fill),0.0);
    let specular=0.12*pow(max(dot(n,half_vector),0.0),45.0);
    var colour=v.colour*diffuse+vec3(specular);
    if v.kind>1.5 {
        // Cut face: a light 45 degree hatch over a dark base in the body colour.
        let edge=abs(fract(diagonal)-0.5);
        let line=1.0-smoothstep(0.25-aa,0.25+aa,edge);
        colour=v.colour*mix(0.25,1.0,line);
    }
    return vec4(colour,1.0);
}
