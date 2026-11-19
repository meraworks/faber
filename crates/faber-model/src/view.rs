//! A saved camera and its assembly display state.
use crate::motion::Pose;
use glam::DVec3;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    pub up: [f64; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fov_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ortho_height: Option<f64>,
}
impl Camera {
    pub fn validate(&self) -> Result<(), String> {
        let delta = DVec3::from(self.target) - DVec3::from(self.eye);
        let Some((dir, up)) = delta
            .try_normalize()
            .zip(DVec3::from(self.up).try_normalize())
        else {
            return Err("The camera needs a finite eye, target and up direction.".into());
        };
        if dir.cross(up).length_squared() < 1e-20 {
            return Err("The camera up direction is parallel to the line of sight.".into());
        }
        match(self.fov_deg,self.ortho_height){
            (Some(v),None) if v>0.&&v<180.=>Ok(()),
            (None,Some(v)) if v.is_finite()&&v>0.=>Ok(()),
            _=>Err("Choose one camera projection. Set a field of view between 0 and 180 degrees or a positive orthographic height.".into())
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamedView {
    pub camera: Camera,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explode: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<String>,
    #[serde(default, skip_serializing_if = "Pose::is_empty")]
    pub joints: Pose,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}
impl NamedView {
    pub fn validate(&self) -> Result<(), String> {
        self.camera.validate()?;
        if self.explode.is_some_and(|x| !(0. ..=1.).contains(&x)) {
            return Err("Set the explode factor between 0 and 1.".into());
        }
        if !matches!(self.style.as_deref(), None | Some("technical" | "studio")) {
            return Err("Use the technical or studio style.".into());
        }
        if self
            .joints
            .values()
            .flat_map(|v| v.as_slice())
            .any(|v| !v.is_finite())
        {
            return Err("A saved joint coordinate must be finite.".into());
        }
        Ok(())
    }
}
