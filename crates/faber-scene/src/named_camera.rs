use super::*;
use faber_model::view::Camera as SavedCamera;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lens {
    pub fov_deg: f32,
    pub ortho_height: Option<f32>,
    pub roll_deg: f32,
}
impl Default for Lens {
    fn default() -> Self {
        Self {
            fov_deg: 38.,
            ortho_height: None,
            roll_deg: 0.,
        }
    }
}

impl Camera {
    /// Keep pointer travel and handle size consistent across projection settings.
    pub fn interaction_scale(self) -> f32 {
        if self.lens.ortho_height.is_none() && self.lens.fov_deg == 38. {
            return self.distance;
        }
        let default = (19_f32.to_radians()).tan();
        self.lens.ortho_height.map_or_else(
            || self.distance * (self.lens.fov_deg.to_radians() * 0.5).tan() / default,
            |h| h / (2. * default),
        )
    }
    pub fn from_named(view: &SavedCamera) -> Result<Self> {
        view.validate()?;
        let target = DVec3::from(view.target);
        let delta = DVec3::from(view.eye) - target;
        let out = delta
            .try_normalize()
            .ok_or("The camera eye and target must differ.")?;
        let up = DVec3::from(view.up);
        let up = (up - out * up.dot(out))
            .try_normalize()
            .ok_or("The camera up must differ from its sight line.")?;
        let mut camera = Self {
            target: target.as_vec3(),
            distance: delta.length() as f32,
            yaw: out.y.atan2(out.x).to_degrees() as f32,
            pitch: out.z.atan2(out.x.hypot(out.y)).to_degrees() as f32,
            lens: Lens {
                fov_deg: view.fov_deg.unwrap_or(38.) as f32,
                ortho_height: view.ortho_height.map(|n| n as f32),
                roll_deg: 0.,
            },
        };
        if !camera.target.is_finite()
            || !camera.distance.is_finite()
            || camera.distance <= 0.
            || camera.eye() == camera.target
            || view.fov_deg.is_some() == view.ortho_height.is_some()
            || !(0.0..180.).contains(&camera.lens.fov_deg)
            || camera.lens.fov_deg <= 0.
            || camera
                .lens
                .ortho_height
                .is_some_and(|h| !h.is_finite() || h <= 0.)
        {
            return Err(
                "The camera needs a finite eye, target and positive projection size.".into(),
            );
        }
        let (_, reference) = camera.screen_axes();
        let up = up.as_vec3();
        camera.lens.roll_deg = out
            .as_vec3()
            .dot(reference.cross(up))
            .atan2(reference.dot(up))
            .to_degrees();
        Ok(camera)
    }

    pub fn to_named(self) -> SavedCamera {
        SavedCamera {
            eye: self.eye().as_dvec3().into(),
            target: self.target.as_dvec3().into(),
            up: self.screen_axes().1.as_dvec3().into(),
            fov_deg: self
                .lens
                .ortho_height
                .is_none()
                .then_some(f64::from(self.lens.fov_deg)),
            ortho_height: self.lens.ortho_height.map(f64::from),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_rolled_orthographic_view_preserves_its_rays_and_target_when_zoomed() {
        let named = SavedCamera {
            eye: [10., 0., 0.],
            target: [0.; 3],
            up: [0., 1., 0.],
            fov_deg: None,
            ortho_height: Some(20.),
        };
        let mut c = Camera::from_named(&named).unwrap();
        assert!(c.screen_axes().1.distance(Vec3::Y) < 1e-6);
        let (a, ray) = c.ray(1., [0., 0.]);
        let (b, edge) = c.ray(1., [0., 1.]);
        assert!(ray.distance(edge) < 1e-6 && ((b - a).y - 10.).abs() < 1e-4);
        let anchor = b + edge * 10.;
        c.zoom_about(anchor.as_vec3(), 0.5);
        assert_eq!(c.lens.ortho_height, Some(10.));
        let [x, y, _] = c.matrix(1.).project_point3(anchor.as_vec3()).to_array();
        assert!(x.abs() < 1e-4 && (y - 1.).abs() < 1e-4);
        assert_eq!(c.to_named().fov_deg, None);
    }
}
