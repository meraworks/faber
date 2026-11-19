//! Build a form in the local coordinates of its part.
use faber_form::{Cage, subdivide};
use faber_geometry::bvh::Bvh;
use faber_model::{CageData, SymmetryPlane};
use faber_ports::{Kernel, Mesh, Result, Shape};

pub fn mesh(data: &CageData, levels: Option<u32>, symmetry: &[SymmetryPlane]) -> Result<Mesh> {
    let cage = Cage::from_data(data)?;
    faber_form::edit::check_symmetry(&cage, symmetry)?;
    subdivide(&cage, levels.unwrap_or(3))
}

pub(crate) fn solid(
    kernel: &dyn Kernel,
    data: &CageData,
    levels: Option<u32>,
    as_solid: bool,
    symmetry: &[SymmetryPlane],
) -> Result<Shape> {
    if !as_solid {
        return Err("Set as_solid to use a form in a solid history.".into());
    }
    let cage = Cage::from_data(data)?;
    faber_form::edit::check_symmetry(&cage, symmetry)?;
    let surface = faber_form::subdivide_topology(&cage, levels.unwrap_or(3))?;
    if Bvh::new(&surface)?.intersects_itself() {
        return Err("The form intersects itself.".into());
    }
    kernel.solid_from_mesh(&surface, true)
}
