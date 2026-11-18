use faber_model::{
    Feature, Geometry, Part, Placement, ProjectData, Scalar, SketchLoop, SketchProfile,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Plane {
    Xy,
    Xz,
    Yz,
}
#[derive(Clone, Copy, PartialEq)]
pub enum Tool {
    Spline,
    Rectangle,
    Slot,
    Circle,
    Polygon,
}

pub struct SketchDraft {
    pub expected: ProjectData,
    pub plane: Plane,
    pub tool: Tool,
    pub points: Vec<[f64; 2]>,
    pub height: String,
    pub name: String,
    pub scale: f32,
    pub dimensions: [String; 2],
    periodic: bool,
    spline: SplinePreview,
}

type PreviewResult = Result<Vec<[[f64; 2]; 2]>, String>;

fn spline_outline(profile: &SketchProfile) -> PreviewResult {
    let mesh = faber_wiring::sketch_preview(profile)?;
    let mut edges = std::collections::BTreeMap::new();
    for triangle in &mesh.triangles {
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
        }
    }
    let outline: Vec<_> = edges
        .into_iter()
        .filter(|(_, count)| *count == 1)
        .map(|((a, b), _)| {
            let a = mesh.vertices[a as usize];
            let b = mesh.vertices[b as usize];
            [[a[0], a[1]], [b[0], b[1]]]
        })
        .collect();
    if outline.is_empty() {
        return Err("Sketch preview has no boundary".into());
    }
    Ok(outline)
}
#[derive(Default)]
struct SplinePreview {
    requested: Vec<[f64; 2]>,
    periodic: bool,
    pending: Option<std::sync::mpsc::Receiver<PreviewResult>>,
    result: Option<PreviewResult>,
}

fn spline_profile(points: &[[f64; 2]], periodic: bool) -> Result<SketchProfile, String> {
    if points.len() < 3 {
        return Err("Spline needs at least three points".into());
    }
    let mut segments = vec![faber_model::SketchSegment::Spline {
        points: points[1..].to_vec(),
        tangents: None,
        periodic,
    }];
    if !periodic {
        segments.push(faber_model::SketchSegment::Line { end: points[0] });
    }
    Ok(SketchProfile {
        outer: SketchLoop::Path {
            start: points[0],
            segments,
        },
        holes: vec![],
    })
}

impl SplinePreview {
    fn update(&mut self, points: &[[f64; 2]], periodic: bool, ctx: &eframe::egui::Context) {
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok(result) => {
                    self.result = Some(result);
                    self.pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.result = Some(Err("Sketch preview worker stopped".into()));
                    self.pending = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.pending.is_none() && (self.requested != points || self.periodic != periodic) {
            self.requested = points.to_vec();
            self.periodic = periodic;
            self.result = None;
            if let Ok(profile) = spline_profile(points, periodic) {
                let (tx, rx) = std::sync::mpsc::channel();
                let ctx = ctx.clone();
                self.pending = Some(rx);
                std::thread::spawn(move || {
                    let _ = tx.send(spline_outline(&profile));
                    ctx.request_repaint();
                });
            }
        }
    }
}

impl SketchDraft {
    pub fn preview_points(&self) -> Result<Vec<[f64; 2]>, String> {
        if self.points.len() < 2 || matches!(self.tool, Tool::Polygon | Tool::Spline) {
            return Ok(self.points.clone());
        }
        let values = faber_expr::resolve(&self.expected.params)?;
        let dimension = |index: usize, fallback: f64| -> Result<f64, String> {
            if self.dimensions[index].trim().is_empty() {
                return Ok(fallback);
            }
            let value = faber_expr::evaluate(&self.dimensions[index], |name| {
                values
                    .get(name)
                    .copied()
                    .ok_or_else(|| format!("Unknown parameter: {name}"))
            })?;
            if value <= 0. || value > 1e6 {
                return Err("Dimension must be positive and at most 1000000 mm".into());
            }
            Ok(value)
        };
        let a = self.points[0];
        let b = self.points[1];
        Ok(match self.tool {
            Tool::Rectangle | Tool::Slot => {
                let origin = [a[0].min(b[0]), a[1].min(b[1])];
                let length = dimension(0, (a[0] - b[0]).abs())?;
                let width = dimension(1, (a[1] - b[1]).abs())?;
                if self.tool == Tool::Slot && length < width {
                    return Err("Slot length must be at least its width".into());
                }
                vec![origin, [origin[0] + length, origin[1] + width]]
            }
            Tool::Circle => vec![
                a,
                [a[0] + dimension(0, (a[0] - b[0]).hypot(a[1] - b[1]))?, a[1]],
            ],
            Tool::Polygon | Tool::Spline => unreachable!(),
        })
    }
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) -> bool {
        use eframe::egui::{self, Color32, Stroke};
        ui.horizontal(|ui| {
            ui.label("Plane");
            for (name, plane) in [("XY", Plane::Xy), ("XZ", Plane::Xz), ("YZ", Plane::Yz)] {
                ui.selectable_value(&mut self.plane, plane, name);
            }
        });
        ui.horizontal(|ui| {
            for (name, tool) in [
                ("Rectangle", Tool::Rectangle),
                ("Circle", Tool::Circle),
                ("Slot", Tool::Slot),
                ("Polygon", Tool::Polygon),
                ("Spline", Tool::Spline),
            ] {
                if ui.selectable_value(&mut self.tool, tool, name).changed() {
                    self.points.clear();
                    self.dimensions = Default::default();
                }
            }
            if ui.button("Remove point").clicked() {
                self.points.pop();
            }
            if ui.button("Clear sketch").clicked() {
                self.points.clear();
            }
        });
        if self.tool == Tool::Spline {
            ui.checkbox(&mut self.periodic, "Closed spline");
        }
        ui.label(match self.tool {
            Tool::Rectangle => "Click two opposite corners.",
            Tool::Circle => "Click the centre, then a point on the circle.",
            Tool::Slot => "Click opposite corners of the slot bounds. Length runs along X.",
            Tool::Polygon => "Click vertices in order. The last vertex joins the first.",
            Tool::Spline if self.periodic => {
                "Click at least three points. The curve closes smoothly at the first point."
            }
            Tool::Spline => {
                "Click at least three points on the curve. A straight edge closes the profile."
            }
        });
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 360.), egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Unknown, ui.is_enabled(), "Sketch canvas")
        });
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4., Color32::from_rgb(15, 21, 29));
        let centre = rect.center();
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.scale = (self.scale * (scroll * 0.002).exp()).clamp(0.1, 20.);
        }
        let spacing = 10. * self.scale;
        for i in -(rect.width() / spacing) as i32..=(rect.width() / spacing) as i32 {
            let x = centre.x + i as f32 * spacing;
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                Stroke::new(1., Color32::from_gray(35)),
            );
        }
        for i in -(rect.height() / spacing) as i32..=(rect.height() / spacing) as i32 {
            let y = centre.y + i as f32 * spacing;
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                Stroke::new(1., Color32::from_gray(35)),
            );
        }
        painter.line_segment(
            [
                egui::pos2(rect.left(), centre.y),
                egui::pos2(rect.right(), centre.y),
            ],
            Stroke::new(1., Color32::from_rgb(140, 80, 80)),
        );
        painter.line_segment(
            [
                egui::pos2(centre.x, rect.top()),
                egui::pos2(centre.x, rect.bottom()),
            ],
            Stroke::new(1., Color32::from_rgb(70, 130, 110)),
        );
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.click([
                ((pos.x - centre.x) / self.scale).round() as f64,
                ((centre.y - pos.y) / self.scale).round() as f64,
            ]);
        }
        let screen =
            |p: [f64; 2]| centre + egui::vec2(p[0] as f32 * self.scale, -p[1] as f32 * self.scale);
        let stroke = Stroke::new(2., Color32::from_rgb(100, 205, 192));
        if self.tool == Tool::Spline {
            self.spline.update(&self.points, self.periodic, ui.ctx());
            if self.points.len() >= 3 {
                if self.spline.requested == self.points
                    && self.spline.periodic == self.periodic
                    && let Some(result) = &self.spline.result
                {
                    match result {
                        Ok(outline) => {
                            for [a, b] in outline {
                                painter.line_segment([screen(*a), screen(*b)], stroke);
                            }
                            ui.label("Spline preview ready");
                        }
                        Err(error) => {
                            ui.colored_label(Color32::LIGHT_RED, error);
                        }
                    }
                } else {
                    ui.label("Building spline preview…");
                }
            }
        }
        let preview = self.preview_points();
        if let Err(error) = &preview {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        let points = preview.unwrap_or_else(|_| self.points.clone());
        for point in &points {
            painter.circle_filled(screen(*point), 3., stroke.color);
        }
        if points.len() >= 2 {
            let a = screen(points[0]);
            let b = screen(points[1]);
            match self.tool {
                Tool::Spline => {}
                Tool::Rectangle => {
                    painter.rect_stroke(
                        egui::Rect::from_two_pos(a, b),
                        0.,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                Tool::Circle => {
                    painter.circle_stroke(a, a.distance(b), stroke);
                }
                Tool::Slot => {
                    let bounds = egui::Rect::from_two_pos(a, b);
                    let radius = bounds.height() / 2.;
                    if bounds.width() >= bounds.height() {
                        let mut outline = Vec::new();
                        for (x, start) in [
                            (bounds.right() - radius, -90_f32),
                            (bounds.left() + radius, 90_f32),
                        ] {
                            for step in 0..=24 {
                                let angle = (start + step as f32 * 7.5).to_radians();
                                outline.push(egui::pos2(
                                    x + radius * angle.cos(),
                                    bounds.center().y + radius * angle.sin(),
                                ));
                            }
                        }
                        painter.add(egui::Shape::closed_line(outline, stroke));
                    }
                }
                Tool::Polygon => {
                    for i in 0..points.len() {
                        painter.line_segment(
                            [screen(points[i]), screen(points[(i + 1) % points.len()])],
                            stroke,
                        );
                    }
                }
            }
        }
        ui.label("Grid: 10 mm · Snap: 1 mm · Scroll to zoom");
        if !matches!(self.tool, Tool::Polygon | Tool::Spline) {
            for (index, label) in if self.tool == Tool::Circle {
                vec![(0, "Radius")]
            } else if self.tool == Tool::Slot {
                vec![(0, "Length"), (1, "Width")]
            } else {
                vec![(0, "Width"), (1, "Height")]
            } {
                ui.horizontal(|ui| {
                    let label = ui.label(label);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.dimensions[index])
                            .hint_text("Use drawn size"),
                    )
                    .labelled_by(label.id);
                    ui.label("mm");
                });
            }
            ui.label("Dimensions accept parameter expressions. Empty fields use the drawn size.");
        }
        ui.horizontal(|ui| {
            let label = ui.label("Body name");
            ui.text_edit_singleline(&mut self.name)
                .labelled_by(label.id);
        });
        ui.horizontal(|ui| {
            let label = ui.label("Extrusion distance");
            ui.text_edit_singleline(&mut self.height)
                .labelled_by(label.id);
            ui.label("mm");
        });
        ui.button("Create solid").clicked()
    }
    pub fn new(expected: ProjectData) -> Self {
        Self {
            expected,
            plane: Plane::Xy,
            tool: Tool::Rectangle,
            points: vec![],
            height: "40".into(),
            name: "Body".into(),
            scale: 2.,
            dimensions: Default::default(),
            periodic: false,
            spline: Default::default(),
        }
    }
    pub fn click(&mut self, point: [f64; 2]) {
        if !matches!(self.tool, Tool::Polygon | Tool::Spline) && self.points.len() == 2 {
            self.points.clear();
        }
        self.points.push(point);
    }
    pub fn part(&self) -> Result<Part<Scalar>, String> {
        if self.name.trim().is_empty() {
            return Err("Enter a body name".into());
        }
        if self.points.len() < 2 {
            return Err("Draw a closed profile first".into());
        }
        let a = self.points[0];
        let b = self.points[1];
        let dimension = |index: usize, fallback: f64| {
            if self.dimensions[index].trim().is_empty() {
                Scalar::Number(fallback)
            } else {
                Scalar::Expression(self.dimensions[index].clone())
            }
        };
        let outer = match self.tool {
            Tool::Spline => spline_profile(&self.points, self.periodic)?
                .outer
                .map_numbers(&mut |value| Ok(Scalar::Number(*value)))?,
            Tool::Slot => {
                self.preview_points()?;
                let length = dimension(0, (a[0] - b[0]).abs());
                let width = dimension(1, (a[1] - b[1]).abs());
                let centre = |origin: f64, size: &Scalar| match size {
                    Scalar::Number(value) => Scalar::Number(origin + value / 2.),
                    Scalar::Expression(value) => {
                        Scalar::Expression(format!("{origin} + ({value}) / 2"))
                    }
                };
                SketchLoop::Slot {
                    centre: [
                        centre(a[0].min(b[0]), &length),
                        centre(a[1].min(b[1]), &width),
                    ],
                    length,
                    width,
                    rotation: 0_f64.into(),
                }
            }
            Tool::Rectangle => SketchLoop::Rectangle {
                origin: [a[0].min(b[0]).into(), a[1].min(b[1]).into()],
                size: [
                    dimension(0, (a[0] - b[0]).abs()),
                    dimension(1, (a[1] - b[1]).abs()),
                ],
            },
            Tool::Circle => SketchLoop::Circle {
                centre: a.map(Scalar::from),
                radius: dimension(0, (a[0] - b[0]).hypot(a[1] - b[1])),
            },
            Tool::Polygon => {
                if self.points.len() < 3 {
                    return Err("Polygon needs at least three points".into());
                }
                SketchLoop::Polygon {
                    points: self.points.iter().map(|p| p.map(Scalar::from)).collect(),
                    corner_radius: None,
                }
            }
        };
        let mut placement = Placement::<Scalar>::default();
        let h = Scalar::Expression(self.height.clone());
        let vector = match self.plane {
            Plane::Xy => [0.0.into(), 0.0.into(), h],
            Plane::Xz => {
                placement.rotation = [90.0.into(), 0.0.into(), 0.0.into()];
                [
                    0.0.into(),
                    Scalar::Expression(format!("-({})", self.height)),
                    0.0.into(),
                ]
            }
            Plane::Yz => {
                placement.rotation = [90.0.into(), 0.0.into(), 90.0.into()];
                [h, 0.0.into(), 0.0.into()]
            }
        };
        Ok(Part {
            name: self.name.clone(),
            colour: [151, 172, 176],
            group: String::new(),
            note: String::new(),
            placement: Placement::default(),
            geometry: Geometry::History {
                features: vec![
                    Feature {
                        id: "sketch".into(),
                        geometry: Geometry::Sketch {
                            profile: SketchProfile {
                                outer,
                                holes: vec![],
                            },
                            placement,
                        },
                        suppressed: false,
                    },
                    Feature {
                        id: "extrude".into(),
                        geometry: Geometry::ExtrudeSketch {
                            source: "sketch".into(),
                            vector,
                        },
                        suppressed: false,
                    },
                ],
                output: "extrude".into(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_expressions_remain_linked_to_parameters() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = faber_wiring::open(dir.path(), true, Some("Dimensions".into())).unwrap();
        project
            .set_params([("width".into(), "25".into())].into())
            .unwrap();
        let mut draft = SketchDraft::new(project.data.clone());
        draft.click([0., 0.]);
        draft.click([20., 30.]);
        draft.dimensions = ["width".into(), "width * 2".into()];
        assert_eq!(draft.preview_points().unwrap(), vec![[0., 0.], [25., 50.]]);
        project
            .put_part("body".into(), draft.part().unwrap())
            .unwrap();
        assert!((project.bodies[0].volume - 50000.).abs() < 1e-6);
        project
            .set_params([("width".into(), "30".into())].into())
            .unwrap();
        assert!((project.bodies[0].volume - 72000.).abs() < 1e-6);
        draft.tool = Tool::Circle;
        draft.dimensions[0] = "width / 2".into();
        project
            .put_part("circle".into(), draft.part().unwrap())
            .unwrap();
        assert!(
            (project
                .bodies
                .iter()
                .find(|b| b.id == "circle")
                .unwrap()
                .volume
                - 9000. * std::f64::consts::PI)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn rectangle_on_each_plane_builds_and_keeps_editable_history() {
        for plane in [Plane::Xy, Plane::Xz, Plane::Yz] {
            let dir = tempfile::tempdir().unwrap();
            let mut project = faber_wiring::open(dir.path(), true, Some("Sketch".into())).unwrap();
            let mut draft = SketchDraft::new(project.data.clone());
            draft.plane = plane;
            draft.click([0., 0.]);
            draft.click([20., 30.]);
            draft.height = "40".into();
            project
                .put_part("body".into(), draft.part().unwrap())
                .unwrap();
            assert!((project.bodies[0].volume - 24000.).abs() < 1e-6);
            let size: Vec<_> = (0..3)
                .map(|i| {
                    (project.bodies[0].bounds[1][i] - project.bodies[0].bounds[0][i]).round() as i64
                })
                .collect();
            assert_eq!(
                size,
                match plane {
                    Plane::Xy => vec![20, 30, 40],
                    Plane::Xz => vec![20, 40, 30],
                    Plane::Yz => vec![40, 20, 30],
                }
            );
            let faber_model::Geometry::History { features, .. } =
                &project.data.parts["body"].geometry
            else {
                panic!("Missing history")
            };
            assert_eq!(features.len(), 2);
        }
    }

    #[test]
    fn circle_and_polygon_keep_exact_profiles_and_incomplete_input_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = faber_wiring::open(dir.path(), true, Some("Sketch".into())).unwrap();
        let mut draft = SketchDraft::new(project.data.clone());
        assert!(draft.part().is_err());
        draft.tool = Tool::Circle;
        draft.click([0., 0.]);
        draft.click([10., 0.]);
        draft.height = "20".into();
        project
            .put_part("circle".into(), draft.part().unwrap())
            .unwrap();
        assert!((project.bodies[0].volume - 2000. * std::f64::consts::PI).abs() < 1e-5);
        draft.tool = Tool::Polygon;
        draft.points = vec![[0., 0.], [20., 0.], [20., 30.], [0., 30.]];
        project
            .put_part("polygon".into(), draft.part().unwrap())
            .unwrap();
        assert!(
            (project
                .bodies
                .iter()
                .find(|b| b.id == "polygon")
                .unwrap()
                .volume
                - 12000.)
                .abs()
                < 1e-6
        );
    }
}
