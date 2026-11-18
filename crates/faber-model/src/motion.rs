//! Rigid motion from the assembled position.

use crate::Placement;
use glam::{DQuat, DVec3, EulerRot};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
mod coordinates;
pub use coordinates::{Pose, PoseValue};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MotionStudy {
    pub joints: BTreeMap<String, Joint>,
    /// Keyed by the driven joint.
    pub gears: BTreeMap<String, Gear>,
    pub linkages: BTreeMap<String, FourBar>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Joint {
    pub kind: JointKind,
    #[serde(default)]
    pub parts: Vec<String>,
    #[serde(default)]
    pub parent: Option<String>,
    pub origin: [f64; 3],
    pub axis: [f64; 3],
    #[serde(default)]
    pub limits: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_axis: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits2: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits3: Option<[f64; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JointKind {
    Revolute,
    Slider,
    Rigid,
    Cylindrical,
    PinSlot,
    Planar,
    Ball,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Gear {
    pub driver: String,
    /// Driven angle divided by driver angle, with sign.
    pub ratio: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FourBar {
    pub driver: String,
    pub follower: String,
    /// Pin centres at the assembled position.
    pub driver_pin: [f64; 3],
    pub follower_pin: [f64; 3],
    pub rod_parts: Vec<String>,
    /// Ball stud axes at the driver and follower pins in the assembled position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket_axes: Option<[[f64; 3]; 2]>,
}

#[derive(Clone, Copy, Default)]
struct Frame {
    rotation: DQuat,
    offset: DVec3,
}

impl Frame {
    fn about(axis: DVec3, angle: f64, origin: DVec3) -> Self {
        let rotation = DQuat::from_axis_angle(axis, angle);
        Self {
            rotation,
            offset: origin - rotation * origin,
        }
    }

    fn point(self, point: DVec3) -> DVec3 {
        self.rotation * point + self.offset
    }

    fn compose(self, local: Self) -> Self {
        Self {
            rotation: self.rotation * local.rotation,
            offset: self.point(local.offset),
        }
    }

    fn placement(self) -> Result<Placement, String> {
        if !self.offset.is_finite() || !self.rotation.is_finite() {
            return Err("The motion exceeds the numeric range".into());
        }
        let (z, y, x) = self.rotation.to_euler(EulerRot::ZYX);
        Ok(Placement {
            translation: self.offset.into(),
            rotation: [x, y, z].map(f64::to_degrees),
        })
    }
}

impl MotionStudy {
    /// Get the world displacement of each moving part. Omitted inputs are zero.
    pub fn solve(
        &self,
        inputs: &BTreeMap<String, f64>,
    ) -> Result<BTreeMap<String, Placement>, String> {
        self.solve_pose(
            &inputs
                .iter()
                .map(|(id, &v)| (id.clone(), PoseValue::Number(v)))
                .collect(),
        )
    }
    pub fn solve_pose(&self, inputs: &Pose) -> Result<BTreeMap<String, Placement>, String> {
        let mut followers = BTreeMap::new();
        for (id, link) in &self.linkages {
            let driver = self.scalar_joint(&link.driver)?;
            let follower = self.scalar_joint(&link.follower)?;
            if driver.parent != follower.parent
                || followers.insert(&link.follower, link).is_some()
                || self.gears.contains_key(&link.follower)
            {
                return Err(format!("Check the follower and parent of linkage {id}"));
            }
        }
        for (id, gear) in &self.gears {
            self.scalar_joint(id)?;
            self.scalar_joint(&gear.driver)?;
            if !gear.ratio.is_finite() || gear.ratio == 0.0 {
                return Err(format!("Invalid gear ratio for {id}"));
            }
        }
        for id in inputs.keys() {
            if !self.joints.contains_key(id)
                || followers.contains_key(id)
                || self.gears.contains_key(id)
            {
                return Err(format!("Joint {id} cannot take a motion input"));
            }
        }
        let mut angles = BTreeMap::<&String, f64>::new();
        let mut frames = BTreeMap::<&String, Frame>::new();
        let mut parts = BTreeMap::new();
        let mut pending: BTreeSet<_> = self.joints.keys().collect();
        while !pending.is_empty() {
            let count = pending.len();
            for id in pending.clone() {
                let joint = &self.joints[id];
                let parent = match &joint.parent {
                    Some(key) => match frames.get(key) {
                        Some(frame) => *frame,
                        None => continue,
                    },
                    None => Frame::default(),
                };
                let mut rod = None;
                let driven;
                let values = if let Some(gear) = self.gears.get(id) {
                    let Some(value) = angles.get(&gear.driver) else {
                        continue;
                    };
                    driven = [value * gear.ratio];
                    &driven[..]
                } else if let Some(link) = followers.get(id) {
                    let Some(angle) = angles.get(&link.driver) else {
                        continue;
                    };
                    let (angle, frame) = link.close(&self.joints[&link.driver], joint, *angle)?;
                    rod = Some((&link.rod_parts, parent.compose(frame)));
                    driven = [angle];
                    &driven[..]
                } else {
                    inputs
                        .get(id)
                        .map_or(&[0.; 3][..joint.kind.count()], PoseValue::as_slice)
                };
                let local = joint.displacement(id, values)?;
                let frame = parent.compose(local);
                for (ids, transform) in Some((&joint.parts, frame)).into_iter().chain(rod) {
                    let placement = transform.placement()?;
                    for part in ids {
                        if parts.insert(part.clone(), placement.clone()).is_some() {
                            return Err(format!("Part {part} has more than one motion"));
                        }
                    }
                }
                frames.insert(id, frame);
                if let [value] = values {
                    angles.insert(id, *value);
                }
                pending.remove(id);
            }
            if pending.len() == count {
                return Err(format!(
                    "Missing or cyclic joint dependency for {}",
                    pending.first().unwrap()
                ));
            }
        }
        Ok(parts)
    }

    fn scalar_joint(&self, id: &str) -> Result<&Joint, String> {
        self.joints
            .get(id)
            .filter(|joint| joint.kind.count() == 1)
            .ok_or_else(|| format!("Expected a revolute or slider joint for {id}"))
    }
}

impl Joint {
    fn direction(&self) -> Result<DVec3, String> {
        DVec3::from(self.axis)
            .try_normalize()
            .ok_or_else(|| "Invalid joint axis".into())
    }
}

impl FourBar {
    fn close(&self, driver: &Joint, follower: &Joint, angle: f64) -> Result<(f64, Frame), String> {
        let axis = follower.direction()?;
        let a = DVec3::from(self.driver_pin);
        let b = DVec3::from(self.follower_pin);
        let pivot = DVec3::from(follower.origin);
        let driver_frame = driver.displacement(&self.driver, &[angle])?;
        let moved = driver_frame.point(a);
        let rod = b - a;
        if rod.length() <= 1e-12 {
            return Err("The linkage has coincident pin centres".into());
        }
        let (follower_value, follower_frame) = if follower.kind == JointKind::Slider {
            let gap = b - moved;
            let along = axis.dot(gap);
            let squared = rod.length_squared() - (gap - axis * along).length_squared();
            if squared < -1e-10 * rod.length_squared() {
                return Err("The linkage cannot reach the specified position".into());
            }
            let branch = if axis.dot(rod) < 0. { -1. } else { 1. };
            let travel = -along + branch * squared.max(0.).sqrt();
            (
                travel,
                Frame {
                    offset: axis * travel,
                    ..Frame::default()
                },
            )
        } else {
            let planar = |v: DVec3| v - axis * axis.dot(v);
            let radius = planar(b - pivot);
            let centres = planar(moved - pivot);
            let distance = centres.length();
            let radius_length = radius.length();
            if radius_length <= 1e-12 || rod.length() <= 1e-12 || distance <= 1e-12 {
                return Err("The linkage has coincident pin centres".into());
            }
            let axial_gap = axis.dot(b - moved);
            let cosine = (distance.powi(2) + radius_length.powi(2) + axial_gap.powi(2)
                - rod.length_squared())
                / (2.0 * distance * radius_length);
            if !(-1.0000000001..=1.0000000001).contains(&cosine) {
                return Err("The linkage cannot reach the specified position".into());
            }
            let branch = if axis.dot(planar(a - pivot).cross(radius)) < 0.0 {
                -1.0
            } else {
                1.0
            };
            let next = DQuat::from_axis_angle(axis, branch * cosine.clamp(-1.0, 1.0).acos())
                * (centres * (radius_length / distance));
            let signed_angle =
                |from: DVec3, to: DVec3| axis.dot(from.cross(to)).atan2(from.dot(to));
            let follower_angle = signed_angle(radius, next);
            let follower_rotation = DQuat::from_axis_angle(axis, follower_angle);
            (
                follower_angle.to_degrees(),
                Frame {
                    rotation: follower_rotation,
                    offset: pivot - follower_rotation * pivot,
                },
            )
        };
        let next_rod = follower_frame.point(b) - moved;
        let direction = next_rod.normalize();
        let initial = rod.normalize();
        let cross = initial.cross(direction);
        let base = if let Some(normal) = cross.try_normalize() {
            DQuat::from_axis_angle(normal, cross.length().atan2(initial.dot(direction)))
        } else {
            DQuat::from_rotation_arc(initial, direction)
        };
        let rotation = match self.socket_axes {
            Some(axes) => {
                let axes = axes.map(DVec3::from);
                let mut normals = [DVec3::ZERO; 2];
                for (normal, stud) in normals.iter_mut().zip(axes) {
                    *normal = (stud - initial * initial.dot(stud))
                        .try_normalize()
                        .ok_or("Invalid socket axis")?;
                }
                let studs = [
                    driver_frame.rotation * axes[0].normalize(),
                    follower_frame.rotation * axes[1].normalize(),
                ];
                let score = |roll: f64| {
                    let rotation = DQuat::from_axis_angle(direction, roll.to_radians()) * base;
                    normals
                        .iter()
                        .zip(studs)
                        .map(|(n, stud)| (rotation * *n).dot(stud).abs())
                        .fold(1.0, f64::min)
                };
                // Get the roll that gives the smallest maximum socket tilt.
                let best = |centre: f64, step: f64, count: i32| {
                    (-count..=count)
                        .map(|i| centre + f64::from(i) * step)
                        .max_by(|a, b| score(*a).total_cmp(&score(*b)))
                        .unwrap()
                };
                let coarse = best(0.0, 0.5, 90);
                let roll = best(coarse, 0.01, 50);
                DQuat::from_axis_angle(direction, roll.to_radians()) * base
            }
            None => base,
        };
        if angle == 0.0 {
            return Ok((0.0, Frame::default()));
        }
        Ok((
            follower_value,
            Frame {
                rotation,
                offset: moved - rotation * a,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pivot(part: &str, x: f64) -> Joint {
        Joint {
            kind: JointKind::Revolute,
            parts: vec![part.into()],
            parent: None,
            origin: [x, 0., 0.],
            axis: [0., 0., 2.],
            limits: None,
            second_axis: None,
            limits2: None,
            limits3: None,
        }
    }

    fn point(placement: &Placement, p: DVec3) -> DVec3 {
        let [x, y, z] = placement.rotation.map(f64::to_radians);
        DQuat::from_euler(EulerRot::ZYX, z, y, x) * p + DVec3::from(placement.translation)
    }

    #[test]
    fn gears_and_sliders_follow_the_parent() {
        let mut study = MotionStudy::default();
        study.joints.insert("gear".into(), pivot("wheel", 0.));
        study.joints.insert("input".into(), pivot("shaft", 1.));
        study.gears.insert(
            "gear".into(),
            Gear {
                driver: "input".into(),
                ratio: -2.0,
            },
        );
        let mut slide = pivot("slide", 1.);
        slide.parent = Some("input".into());
        slide.kind = JointKind::Slider;
        slide.axis = [1., 0., 0.];
        slide.limits = Some([0., 2.]);
        study.joints.insert("slide".into(), slide);
        let mut inputs = BTreeMap::from([("input".into(), 90.), ("slide".into(), 2.)]);
        let result = study.solve(&inputs).unwrap();
        assert!(point(&result["slide"], DVec3::X).distance(DVec3::new(1., 2., 0.)) < 1e-12);
        assert!(point(&result["wheel"], DVec3::X).distance(-DVec3::X) < 1e-12);
        inputs.insert("slide".into(), 3.);
        assert!(study.solve(&inputs).is_err());
    }

    #[test]
    fn both_rod_pins_stay_on_the_cranks() {
        let mut study = MotionStudy::default();
        study.joints.insert("a".into(), pivot("crank", 0.));
        study.joints.insert("b".into(), pivot("rocker", 4.));
        let a = DVec3::Y;
        let b = DVec3::new(4., 1., 0.);
        study.linkages.insert(
            "bar".into(),
            FourBar {
                driver: "a".into(),
                follower: "b".into(),
                driver_pin: a.into(),
                follower_pin: b.into(),
                rod_parts: vec!["rod".into()],
                socket_axes: None,
            },
        );
        for angle in [-45., 0., 30.] {
            let result = study.solve(&BTreeMap::from([("a".into(), angle)])).unwrap();
            assert!(point(&result["crank"], a).distance(point(&result["rod"], a)) < 1e-12);
            assert!(point(&result["rocker"], b).distance(point(&result["rod"], b)) < 1e-12);
        }
        study.joints.get_mut("a").unwrap().parent = Some("b".into());
        assert!(study.solve(&BTreeMap::new()).is_err());
    }

    #[test]
    fn ball_sockets_allow_skew_crank_axes() {
        let mut driver = pivot("input", 0.);
        driver.axis = [1., 1., 0.];
        let mut follower = pivot("output", 4.);
        follower.axis = [0., 1., 0.];
        let a = DVec3::Z;
        let b = DVec3::new(4., 0., 1.);
        let link = FourBar {
            driver: "driver".into(),
            follower: "follower".into(),
            driver_pin: a.into(),
            follower_pin: b.into(),
            rod_parts: vec!["rod".into()],
            socket_axes: Some([[0., 1., 0.]; 2]),
        };
        for angle in [-12., 12.] {
            let (solved, rod) = link.close(&driver, &follower, angle).unwrap();
            let start = Frame::about(driver.direction().unwrap(), angle.to_radians(), DVec3::ZERO);
            let end = Frame::about(DVec3::Y, solved.to_radians(), follower.origin.into());
            assert!(rod.point(a).distance(start.point(a)) < 1e-12);
            assert!(rod.point(b).distance(end.point(b)) < 1e-12);
        }
    }
}
