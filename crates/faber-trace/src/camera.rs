use crate::material::Stream;
use faber_model::view::Camera;
use faber_ports::Result;
use glam::DVec3;
pub(crate) struct Frame {
    eye: DVec3,
    forward: DVec3,
    right: DVec3,
    up: DVec3,
    width: u32,
    height: u32,
    ortho: bool,
}
impl Frame {
    pub(crate) fn new(c: &Camera, w: u32, h: u32) -> Result<Self> {
        c.validate()?;
        let eye = DVec3::from(c.eye);
        let forward = (DVec3::from(c.target) - eye).normalize();
        let right = forward.cross(DVec3::from(c.up)).normalize();
        let up = right.cross(forward);
        let half_height = c
            .ortho_height
            .map_or_else(|| (c.fov_deg.unwrap().to_radians() / 2.).tan(), |v| v / 2.);
        Ok(Self {
            eye,
            forward,
            right: right * half_height * (f64::from(w) / f64::from(h)),
            up: up * half_height,
            width: w,
            height: h,
            ortho: c.ortho_height.is_some(),
        })
    }
    pub(crate) fn ray(&self, i: u32, rng: &mut Stream) -> (DVec3, DVec3) {
        let x = 2. * (f64::from(i % self.width) + rng.next()) / f64::from(self.width) - 1.;
        let y = 1. - 2. * (f64::from(i / self.width) + rng.next()) / f64::from(self.height);
        let v = self.right * x + self.up * y;
        if self.ortho {
            (self.eye + v, self.forward)
        } else {
            (self.eye, (self.forward + v).normalize())
        }
    }
}
