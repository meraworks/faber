use super::*;
use faber_wiring::{
    animate::{self, Animation, AnimationOptions},
    studio::{self, Style},
    views::{self, Cut, Picture},
};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveView {
    pub name: String,
    #[schemars(with = "Value")]
    pub view: faber_model::view::NamedView,
    pub expected_revision: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewName {
    pub name: String,
    pub expected_revision: Option<u64>,
}

fn style(value: Option<&str>) -> Result<Option<Style>, String> {
    match value {
        None => Ok(None),
        Some("studio") => Ok(Some(Style::Studio)),
        Some("technical") => Ok(Some(Style::Technical)),
        _ => Err("Use the technical or studio style.".into()),
    }
}
#[derive(serde::Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
enum ViewInput {
    Name(String),
    Preset(faber_scene::View),
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PictureFile {
    pub path: String,
    part: Option<String>,
    view: Option<ViewInput>,
    section: Option<SectionArg>,
    #[serde(default = "image_width")]
    width: u32,
    #[serde(default = "image_height")]
    height: u32,
    #[serde(default)]
    pub inline: bool,
    style: Option<String>,
    quality: Option<String>,
    ground: Option<bool>,
    background: Option<[u8; 3]>,
    exposure: Option<f32>,
    explode: Option<f64>,
    samples: Option<u32>,
    seconds: Option<f64>,
}
impl PictureFile {
    pub fn write(&self, p: &Project) -> Result<Value, String> {
        views::check_picture_path(Path::new(&self.path))?;
        if !(64..=4096).contains(&self.width) || !(64..=4096).contains(&self.height) {
            return Err("Image size must be between 64 and 4096 pixels".into());
        }
        let (named, preset) = match &self.view {
            Some(ViewInput::Name(name)) if p.data.views.contains_key(name) => {
                (Some(name.as_str()), Default::default())
            }
            Some(ViewInput::Name(name)) => (
                None,
                serde_json::from_value(json!(name)).map_err(|_| format!("Unknown view: {name}"))?,
            ),
            Some(ViewInput::Preset(preset)) => (None, *preset),
            None => (None, Default::default()),
        };
        let view = self.view.as_ref().map_or(json!("iso"), |v| json!(v));
        let mut requested = style(self.style.as_deref())?;
        let high = match self.quality.as_deref() {
            Some("high") => true,
            None | Some("standard") | Some("normal") => false,
            _ => return Err("Use standard or high picture quality.".into()),
        };
        if !high && (self.samples.is_some() || self.seconds.is_some()) {
            return Err("Use high quality to set samples or seconds.".into());
        }
        if !high && (self.background.is_some() || self.ground.is_some() || self.exposure.is_some())
        {
            if requested == Some(Style::Technical) {
                return Err("Choose the studio style to change the lighting.".into());
            }
            requested = Some(Style::Studio);
        }
        let mut report = json!({"schema":1,"path":self.path,"width":self.width,"height":self.height,"view":view,"part":self.part,"revision":p.data.revision});
        // Keep the same renderer and cut hatching for the original presets.
        if named.is_none()
            && !high
            && !matches!(requested, Some(Style::Studio))
            && self.explode.is_none()
            && self.ground.is_none()
            && self.background.is_none()
            && self.exposure.is_none()
        {
            let old = Render {
                path: self.path.clone(),
                part: self.part.clone(),
                view: preset,
                section: self.section.clone(),
                width: self.width,
                height: self.height,
                inline: false,
            };
            let (section, bodies) = render_input(p, &old)?;
            faber_wiring::render_view(
                &bodies,
                Path::new(&self.path),
                section,
                self.width,
                self.height,
                preset,
            )?;
        } else {
            let pic = Picture {
                view: named.map(str::to_owned),
                preset,
                part: self.part.clone(),
                cut: self.section.as_ref().map(|s| match s {
                    SectionArg::Saved(id) => Cut::Saved(id.clone()),
                    SectionArg::Plane(s) => Cut::Plane(*s),
                }),
                explode: self.explode,
                joints: Default::default(),
                style: requested,
            };
            let drawn = views::prepare(p, &pic)?;
            let mut opts = studio::Options {
                style: drawn.style,
                high,
                ..Default::default()
            };
            if let Some(v) = self.ground {
                opts.ground = v
            }
            if let Some(v) = self.background {
                opts.background = v
            }
            if let Some(v) = self.exposure {
                opts.exposure = v
            }
            if let Some(v) = self.samples {
                opts.samples = v
            }
            opts.seconds = self.seconds;
            let frame = studio::draw(
                &p.data,
                &drawn.bodies,
                &drawn.camera_at_size(self.width, self.height),
                &opts,
                self.width,
                self.height,
            )?;
            report["warnings"] = json!(drawn.warnings);
            report["samples_done"] = json!(frame.samples_done);
            report["seconds"] = json!(frame.seconds);
            views::save_frame(Path::new(&self.path), frame.rgba, self.width, self.height)?;
        }
        Ok(report)
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnimationFile {
    path: PathBuf,
    kind: String,
    view: Option<String>,
    turns: Option<f64>,
    from: Option<f64>,
    to: Option<f64>,
    joint: Option<String>,
    names: Option<Vec<String>>,
    frames: Option<u32>,
    fps: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    style: Option<String>,
    frames_dir: Option<PathBuf>,
}
impl AnimationFile {
    pub fn write(&self, p: &Project) -> Result<Value, String> {
        let kind = self.kind.as_str();
        if self.turns.is_some() && kind != "turntable"
            || self.names.is_some() && kind != "views"
            || self.joint.is_some() && kind != "motion"
            || (self.from.is_some() || self.to.is_some()) && !matches!(kind, "motion" | "explode")
        {
            return Err("A field does not apply to this animation kind.".into());
        }
        let animation = match kind {
            "turntable" => Animation::Turntable {
                turns: self.turns.unwrap_or(1.),
            },
            "explode" => Animation::Explode {
                from: self.from.unwrap_or(0.),
                to: self.to.unwrap_or(1.),
            },
            "motion" => Animation::Motion {
                joint: self.joint.clone().ok_or("Choose a joint.")?,
                from: self.from.unwrap_or(0.),
                to: self.to.ok_or("Set the final joint coordinate.")?,
            },
            "views" => Animation::Views {
                names: self.names.clone().ok_or("Choose the named views.")?,
            },
            _ => return Err("Use a turntable, explode, motion or views animation.".into()),
        };
        let mut opts = AnimationOptions {
            view: self.view.clone(),
            frames_dir: self.frames_dir.clone(),
            ..Default::default()
        };
        if let Some(v) = self.frames {
            opts.frames = v
        }
        if let Some(v) = self.fps {
            opts.fps = v
        }
        if let Some(v) = self.width {
            opts.width = v
        }
        if let Some(v) = self.height {
            opts.height = v
        }
        let saved = self
            .view
            .as_ref()
            .and_then(|id| p.data.views.get(id))
            .and_then(|v| v.style.as_deref());
        if let Some(v) = style(self.style.as_deref().or(saved))? {
            opts.style = v
        }
        serde_json::to_value(animate::animate(p, &animation, &opts, &self.path)?)
            .map_err(|e| e.to_string())
    }
}
