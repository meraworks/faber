use super::{Frame, Joint, JointKind};
use glam::{DQuat, DVec3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Pose = BTreeMap<String, PoseValue>;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PoseValue {
    Number(f64),
    Values(Vec<f64>),
}
impl PoseValue {
    pub fn as_slice(&self) -> &[f64] {
        match self {
            Self::Number(n) => std::slice::from_ref(n),
            Self::Values(v) => v,
        }
    }
    pub fn from_values(v: Vec<f64>) -> Self {
        if let [n] = v.as_slice() {
            Self::Number(*n)
        } else {
            Self::Values(v)
        }
    }
}
impl JointKind {
    pub fn count(self) -> usize {
        match self {
            Self::Rigid => 0,
            Self::Revolute | Self::Slider => 1,
            Self::Cylindrical | Self::PinSlot => 2,
            Self::Planar | Self::Ball => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Rigid => "rigid",
            Self::Revolute => "revolute",
            Self::Slider => "slider",
            Self::Cylindrical => "cylindrical",
            Self::PinSlot => "pin_slot",
            Self::Planar => "planar",
            Self::Ball => "ball",
        }
    }
    pub fn angular(self, i: usize) -> bool {
        match self {
            Self::Rigid | Self::Slider => false,
            Self::Cylindrical | Self::PinSlot => i == 0,
            Self::Planar => i == 2,
            Self::Ball | Self::Revolute => true,
        }
    }
}
impl Joint {
    pub fn range(&self, i: usize) -> Option<[f64; 2]> {
        match i {
            0 => self.limits,
            1 => self.limits2,
            2 => self.limits3,
            _ => None,
        }
    }
    pub fn slide_axis(&self) -> Result<[f64; 3], String> {
        let n = self.direction()?;
        let x = if let Some(s) = self.second_axis {
            let s = DVec3::from(s)
                .try_normalize()
                .ok_or("Invalid second joint axis")?;
            if s.dot(n).abs() > 1e-8 {
                return Err("The second joint axis must be perpendicular to the first axis".into());
            }
            s
        } else {
            let reference = if n.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
            (reference - n * n.dot(reference)).normalize()
        };
        Ok(x.into())
    }
    pub(super) fn displacement(&self, id: &str, v: &[f64]) -> Result<Frame, String> {
        let count = self.kind.count();
        if v.len() != count {
            return Err(format!(
                "Joint {id} is {}. A pose needs {count} values. It has {}.",
                self.kind.name(),
                v.len()
            ));
        }
        let axis = self.direction()?;
        let origin = DVec3::from(self.origin);
        if !axis.is_finite() || !origin.is_finite() {
            return Err(format!("Invalid axis or origin for {id}"));
        }
        if v.iter().any(|a| !a.is_finite()) {
            return Err(format!("Motion values must be finite for {id}"));
        }
        if self.second_axis.is_some()
            && !matches!(self.kind, JointKind::Planar | JointKind::PinSlot)
        {
            return Err(format!("Joint {id} has no second axis"));
        }
        for (i, limits) in [self.limits, self.limits2, self.limits3]
            .into_iter()
            .enumerate()
        {
            if let Some([lo, hi]) = limits {
                if i >= count || !lo.is_finite() || !hi.is_finite() || lo > hi {
                    return Err(format!("Invalid motion limits for {id}"));
                }
                if self.kind == JointKind::Ball && i == 0 {
                    if lo < 0. || hi > 180. {
                        return Err(format!(
                            "The cone limits for {id} must be in 0 to 180 degrees"
                        ));
                    }
                } else if !(lo..=hi).contains(&v[i]) {
                    return Err(format!("Motion value outside the limits for {id}"));
                }
            }
        }
        let (rotation, translation) = match self.kind {
            JointKind::Rigid => return Ok(Frame::default()),
            JointKind::Revolute => return Ok(Frame::about(axis, v[0].to_radians(), origin)),
            JointKind::Slider => (DQuat::IDENTITY, axis * v[0]),
            JointKind::Cylindrical => {
                (DQuat::from_axis_angle(axis, v[0].to_radians()), axis * v[1])
            }
            JointKind::PinSlot => (
                DQuat::from_axis_angle(axis, v[0].to_radians()),
                DVec3::from(self.slide_axis()?) * v[1],
            ),
            JointKind::Planar => {
                let x = DVec3::from(self.slide_axis()?);
                (
                    DQuat::from_axis_angle(axis, v[2].to_radians()),
                    x * v[0] + axis.cross(x) * v[1],
                )
            }
            JointKind::Ball => {
                let r = DQuat::from_scaled_axis(DVec3::new(
                    v[0].to_radians(),
                    v[1].to_radians(),
                    v[2].to_radians(),
                ));
                if let Some([lo, hi]) = self.limits {
                    let moved = r * axis;
                    let tilt = axis
                        .cross(moved)
                        .length()
                        .atan2(axis.dot(moved))
                        .to_degrees();
                    if tilt < lo - 1e-9 || tilt > hi + 1e-9 {
                        return Err(format!(
                            "Joint {id} cone angle {tilt:.6} degrees is outside the limit {lo} to {hi} degrees."
                        ));
                    }
                }
                (r, DVec3::ZERO)
            }
        };
        Ok(Frame {
            rotation,
            offset: translation + (origin - rotation * origin),
        })
    }
}
