use super::*;
use faber_model::{Relief, SheetRule, SketchLoop, SketchProfile};

pub(super) fn value(n: &Scalar) -> String {
    match n {
        Scalar::Expression(s) => s.clone(),
        Scalar::Number(n) => n.to_string(),
    }
}
fn number(s: &str) -> Scalar {
    s.trim()
        .parse()
        .map(Scalar::Number)
        .unwrap_or_else(|_| Scalar::Expression(s.trim().into()))
}
fn optional(s: &str) -> Option<Scalar> {
    (!s.trim().is_empty()).then(|| number(s))
}
fn optional_text(s: &Option<Scalar>) -> String {
    s.as_ref().map(value).unwrap_or_default()
}

pub(super) fn field(ui: &mut egui::Ui, label: &str, s: &mut String) {
    let id = ui.label(label).id;
    ui.add(
        egui::TextEdit::singleline(s)
            .desired_width(ui.available_width())
            .id_salt(label),
    )
    .labelled_by(id);
}

#[derive(Default, PartialEq, Eq)]
enum Cut {
    #[default]
    None,
    Rectangle,
    Round,
}

pub(super) struct RuleForm {
    thickness: String,
    radius: String,
    k: String,
    cut: Cut,
    width: String,
    depth: String,
    diameter: String,
    base_size: [String; 2],
}
impl Default for RuleForm {
    fn default() -> Self {
        Self {
            thickness: "2".into(),
            radius: "2".into(),
            k: "0.44".into(),
            cut: Cut::None,
            width: "2".into(),
            depth: "2".into(),
            diameter: "2".into(),
            base_size: ["50".into(), "40".into()],
        }
    }
}
impl RuleForm {
    pub(super) fn new(rule: &SheetRule<Scalar>) -> Self {
        let mut f = Self {
            thickness: value(&rule.thickness),
            radius: value(&rule.bend_radius),
            k: optional_text(&rule.k_factor),
            ..Default::default()
        };
        match &rule.relief {
            Relief::None => {}
            Relief::Rectangular { width, depth } => {
                f.cut = Cut::Rectangle;
                f.width = value(width);
                f.depth = value(depth);
            }
            Relief::Round { diameter } => {
                f.cut = Cut::Round;
                f.diameter = value(diameter);
            }
        }
        f
    }
    pub(super) fn controls(&mut self, ui: &mut egui::Ui, create: bool) {
        if create {
            field(ui, "Base width (mm)", &mut self.base_size[0]);
            field(ui, "Base depth (mm)", &mut self.base_size[1]);
        }
        field(ui, "Thickness (mm)", &mut self.thickness);
        field(ui, "Inside bend radius (mm)", &mut self.radius);
        field(ui, "K factor (default 0.44)", &mut self.k);
        egui::ComboBox::from_label("Bend relief")
            .selected_text(match self.cut {
                Cut::None => "None",
                Cut::Rectangle => "Rectangular",
                Cut::Round => "Round",
            })
            .show_ui(ui, |ui| {
                for (cut, name) in [
                    (Cut::None, "None"),
                    (Cut::Rectangle, "Rectangular"),
                    (Cut::Round, "Round"),
                ] {
                    ui.selectable_value(&mut self.cut, cut, name);
                }
            });
        match self.cut {
            Cut::None => {}
            Cut::Rectangle => {
                field(ui, "Relief width (mm)", &mut self.width);
                field(ui, "Relief depth (mm)", &mut self.depth);
            }
            Cut::Round => field(ui, "Relief diameter (mm)", &mut self.diameter),
        }
    }
    pub(super) fn rule(&self) -> SheetRule<Scalar> {
        SheetRule {
            thickness: number(&self.thickness),
            bend_radius: number(&self.radius),
            k_factor: optional(&self.k),
            relief: match self.cut {
                Cut::None => Relief::None,
                Cut::Round => Relief::Round {
                    diameter: number(&self.diameter),
                },
                Cut::Rectangle => Relief::Rectangular {
                    width: number(&self.width),
                    depth: number(&self.depth),
                },
            },
        }
    }
    pub(super) fn base(&self) -> SketchProfile<Scalar> {
        SketchProfile {
            outer: SketchLoop::Rectangle {
                origin: [0., 0.].map(Scalar::Number),
                size: self.base_size.each_ref().map(|s| number(s)),
            },
            holes: vec![],
        }
    }
}

#[derive(Default)]
pub(super) struct FlangeForm {
    pub(super) target: Option<(String, u32)>,
    pub(super) index: Option<usize>,
    id: String,
    length: String,
    angle: String,
    start: String,
    end: String,
}
impl FlangeForm {
    pub(super) fn new(existing: &[Flange<Scalar>]) -> Self {
        let id = (1..)
            .map(|i| format!("flange_{i}"))
            .find(|id| !existing.iter().any(|f| f.id == *id))
            .unwrap();
        Self {
            id,
            length: "30".into(),
            angle: "90".into(),
            ..Default::default()
        }
    }
    pub(super) fn edit(index: usize, f: &Flange<Scalar>) -> Self {
        Self {
            target: Some((f.on.clone(), f.edge)),
            index: Some(index),
            id: f.id.clone(),
            length: value(&f.length),
            angle: optional_text(&f.angle_deg),
            start: optional_text(&f.inset_start),
            end: optional_text(&f.inset_end),
        }
    }
    pub(super) fn controls(&mut self, ui: &mut egui::Ui, ready: bool) {
        if let Some((wall, edge)) = &self.target {
            ui.label(format!("{wall} · edge {edge}"));
        } else if ready {
            ui.label("Pick a straight wall edge");
        } else {
            ui.label("Reading straight edges…");
        }
        if self.index.is_none() {
            field(ui, "Flange name", &mut self.id);
        }
        field(ui, "Flange length (mm)", &mut self.length);
        field(ui, "Bend angle (°)", &mut self.angle);
        field(ui, "Start inset (mm)", &mut self.start);
        field(ui, "End inset (mm)", &mut self.end);
    }
    pub(super) fn apply(&self, flanges: &mut Vec<Flange<Scalar>>) -> Result<(), String> {
        let (on, edge) = self.target.as_ref().ok_or("Pick a straight wall edge.")?;
        let f = Flange {
            id: self.id.trim().into(),
            on: on.clone(),
            edge: *edge,
            length: number(&self.length),
            angle_deg: optional(&self.angle),
            inset_start: optional(&self.start),
            inset_end: optional(&self.end),
            holes: self
                .index
                .map(|i| flanges[i].holes.clone())
                .unwrap_or_default(),
        };
        if let Some(i) = self.index {
            flanges[i] = f;
        } else {
            flanges.push(f);
        }
        Ok(())
    }
}
