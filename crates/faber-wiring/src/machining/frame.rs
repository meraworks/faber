use faber_cam::StockBox;
use faber_model::machining::{Origin, Setup, Stock};
use faber_ports::{FaceInfo, Mesh, Result};
use glam::{DAffine3, DMat3, DQuat, DVec3};

pub struct Frame {
    pub local_from_machine: DAffine3,
    pub stock: StockBox,
    pub part_top: f64,
}
impl Frame {
    pub fn new(mesh: &mut Mesh, setup: &Setup) -> Result<Self> {
        let z = DVec3::from(setup.up)
            .try_normalize()
            .ok_or("Set the up direction.")?;
        // Rotate the working plane through the shortest angle to the setup direction.
        let axis = DVec3::Z.cross(z);
        let sine = axis.length();
        let angle = sine.atan2(z.z);
        let turn = DQuat::from_axis_angle(axis.try_normalize().unwrap_or(DVec3::X), angle);
        let y = z.cross(turn * DVec3::X).normalize();
        let axes = DMat3::from_cols(y.cross(z), y, z);
        let inverse = axes.transpose();
        for normal in &mut mesh.normals {
            *normal = (inverse * DVec3::from(*normal)).into();
        }
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = -lo;
        for p in &mut mesh.vertices {
            let v = inverse * DVec3::from(*p);
            lo = lo.min(v);
            hi = hi.max(v);
            *p = v.into();
        }
        if !lo.is_finite() || !hi.is_finite() {
            return Err("The part has no surface.".into());
        }
        let centre = (hi + lo) * 0.5;
        let (min, max) = match setup.stock {
            Stock::Offset { sides, top, bottom } => (
                lo - DVec3::new(sides, sides, bottom),
                hi + DVec3::new(sides, sides, top),
            ),
            Stock::Box { size } => {
                let size = DVec3::from(size);
                if (size + 1e-7).cmplt(hi - lo).any() {
                    return Err("The stock box must contain the part.".into());
                }
                let min = DVec3::new(centre.x - size.x / 2., centre.y - size.y / 2., lo.z);
                (min, min + size)
            }
        };
        let origin = match setup.origin {
            Origin::StockTopCentre => DVec3::new(centre.x, centre.y, max.z),
            Origin::StockTopCorner => DVec3::new(min.x, min.y, max.z),
            Origin::PartBottomCorner => lo,
        };
        for p in &mut mesh.vertices {
            *p = (DVec3::from(*p) - origin).into();
        }
        Ok(Self {
            local_from_machine: DAffine3::from_mat3_translation(axes, axes * origin),
            stock: StockBox {
                min: (min - origin).into(),
                max: (max - origin).into(),
            },
            part_top: hi.z - origin.z,
        })
    }
    pub fn transform_faces(&self, faces: &mut [FaceInfo]) {
        let inverse = self.local_from_machine.inverse();
        let point = |p: [f64; 3]| inverse.transform_point3(p.into()).into();
        let vector = |p: [f64; 3]| inverse.transform_vector3(p.into()).into();
        for f in faces {
            f.centre = point(f.centre);
            f.normal = f.normal.map(vector);
            if let Some(c) = &mut f.cylinder {
                c.start = point(c.start);
                c.axis = vector(c.axis);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_change_in_setup_direction_keeps_the_toolpaths_on_the_same_side() {
        let frame = |x: f64| {
            let mut mesh = Mesh {
                vertices: vec![[0.; 3], [10., 20., 5.]],
                ..Default::default()
            };
            let setup = Setup {
                part: "plate".into(),
                stock: Stock::Offset {
                    sides: 1.,
                    top: 1.,
                    bottom: 1.,
                },
                origin: Origin::StockTopCorner,
                up: [x, 0., (1. - x * x).sqrt()],
            };
            Frame::new(&mut mesh, &setup)
                .unwrap()
                .local_from_machine
                .matrix3
                .x_axis
        };
        assert!(frame(0.98999).dot(frame(0.99001)) > 0.99999);
    }

    #[test]
    fn side_setup_transforms_points_normals_and_the_drill_axis_together() {
        let mut mesh = Mesh {
            vertices: vec![[1., 2., 3.], [11., 22., 33.]],
            normals: vec![[0., 1., 0.]; 2],
            triangles: vec![],
        };
        let setup = Setup {
            part: "bracket".into(),
            stock: Stock::Offset {
                sides: 1.,
                top: 2.,
                bottom: 3.,
            },
            origin: Origin::StockTopCorner,
            up: [0., 2., 0.],
        };
        let frame = Frame::new(&mut mesh, &setup).unwrap();
        assert_eq!(frame.stock.min, [0., 0., -25.]);
        assert_eq!(frame.stock.max, [12., 32., 0.]);
        assert_eq!(mesh.vertices, [[1., 31., -22.], [11., 1., -2.]]);
        assert_eq!(mesh.normals, [[0., 0., 1.]; 2]);
        assert_eq!(
            frame
                .local_from_machine
                .transform_point3(mesh.vertices[1].into()),
            DVec3::new(11., 22., 33.)
        );
        let mut faces = [FaceInfo {
            cylinder: Some(faber_ports::CylinderFace {
                start: [1., 2., 3.],
                axis: [0., 1., 0.],
                radius: 2.,
                length: 8.,
                internal: true,
            }),
            index: 1,
            created_by: None,
            area: 32.,
            centre: [11., 22., 33.],
            normal: None,
        }];
        frame.transform_faces(&mut faces);
        let cylinder = faces[0].cylinder.as_ref().unwrap();
        assert_eq!(cylinder.start, mesh.vertices[0]);
        assert_eq!(cylinder.axis, [0., 0., 1.]);
        assert_eq!(faces[0].centre, mesh.vertices[1]);
    }
    #[test]
    fn fixed_stock_smaller_than_the_part_refuses_the_setup() {
        let mut mesh = Mesh {
            vertices: vec![[0.; 3], [10., 20., 5.]],
            ..Default::default()
        };
        let setup = Setup {
            part: "plate".into(),
            stock: Stock::Box {
                size: [10., 19.9, 5.],
            },
            origin: Origin::PartBottomCorner,
            up: [0., 0., 1.],
        };
        assert!(
            Frame::new(&mut mesh, &setup)
                .err()
                .unwrap()
                .contains("contain")
        );
    }
}
