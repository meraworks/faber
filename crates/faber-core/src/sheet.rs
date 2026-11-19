//! Build sheet walls and round bends in part coordinates.
use faber_model::{Flange, SheetRule, SketchLoop, SketchProfile};
use faber_ports::{Combine, Kernel, Result, Shape};
use faber_sheet::{Bend, Wall};
use glam::{DAffine3, DMat3, DVec3};

fn wall_solid(kernel: &dyn Kernel, wall: &Wall, thickness: f64) -> Result<Shape> {
    let placement = crate::components::placement(wall.frame);
    let rise = (wall.frame.matrix3.z_axis * thickness).to_array();
    let face = kernel.sketch(&wall.profile, &placement)?;
    let mut body = kernel.extrude_shape(&face, rise)?;
    for relief in &wall.reliefs {
        let cutter = kernel.sketch(
            &SketchProfile {
                outer: relief.clone(),
                holes: vec![],
            },
            &placement,
        )?;
        body = kernel.combine(
            &body,
            &kernel.extrude_shape(&cutter, rise)?,
            Combine::Difference,
        )?;
    }
    Ok(body)
}

fn bend_solid(kernel: &dyn Kernel, bend: &Bend) -> Result<Shape> {
    let [a, b, _, d] = bend.section.map(DVec3::from);
    let x = (b - a).try_normalize().ok_or("The bend has no width.")?;
    let z = x
        .cross(d - a)
        .try_normalize()
        .ok_or("The bend has no area.")?;
    let y = z.cross(x);
    let points = bend
        .section
        .map(|p| {
            let delta = DVec3::from(p) - a;
            [delta.dot(x), delta.dot(y)]
        })
        .to_vec();
    let plane = DAffine3::from_mat3_translation(DMat3::from_cols(x, y, z), a);
    let section = kernel.sketch(
        &SketchProfile {
            outer: SketchLoop::Polygon {
                points,
                corner_radius: None,
            },
            holes: vec![],
        },
        &crate::components::placement(plane),
    )?;
    let [start, end] = bend.line.map(DVec3::from);
    let axis = (end - start)
        .try_normalize()
        .ok_or("The bend axis has no length.")?
        * bend.angle_deg.signum();
    kernel.revolve_shape(
        &section,
        start.to_array(),
        axis.to_array(),
        bend.angle_deg.abs(),
    )
}

pub(crate) fn build(
    kernel: &dyn Kernel,
    rule: &SheetRule,
    profile: &SketchProfile,
    flanges: &[Flange],
) -> Result<Shape> {
    let folded = faber_sheet::fold(rule, profile, flanges)?;
    let (base, walls) = folded
        .walls
        .split_first()
        .ok_or("The sheet needs a base wall.")?;
    let mut solid = wall_solid(kernel, base, rule.thickness)?;
    let bends = folded
        .bends
        .iter()
        .map(|b| (b.flange.as_str(), b))
        .collect::<std::collections::BTreeMap<_, _>>();
    for wall in walls {
        if let Some(bend) = bends.get(wall.id.as_str())
            && bend.angle_deg != 0.
        {
            let sector = bend_solid(kernel, bend).map_err(|e| format!("Bend {}: {e}", wall.id))?;
            solid = kernel.combine(&solid, &sector, Combine::Union)?;
        }
        solid = kernel.combine(
            &solid,
            &wall_solid(kernel, wall, rule.thickness)?,
            Combine::Union,
        )?;
    }
    let properties = kernel.properties(&solid)?;
    if !properties.valid || properties.solids != 1 {
        return Err("The walls and bends must make one valid sheet solid.".into());
    }
    Ok(solid)
}
