use super::*;
use faber_scene::View;
use glam::Vec3;

fn view(axes: [i8; 3]) -> View {
    match axes {
        [1, 0, 0] => View::Right,
        [-1, 0, 0] => View::Left,
        [0, 1, 0] => View::Back,
        [0, -1, 0] => View::Front,
        [0, 0, 1] => View::Top,
        [0, 0, -1] => View::Bottom,
        a if a.contains(&0) => View::Edge(a),
        a => View::Corner(a),
    }
}

fn name([x, y, z]: [i8; 3]) -> String {
    let mut words = Vec::new();
    if z != 0 {
        words.push(if z > 0 { "top" } else { "bottom" });
    }
    if y != 0 {
        words.push(if y > 0 { "back" } else { "front" });
    }
    if x != 0 {
        words.push(if x > 0 { "right" } else { "left" });
    }
    let mut label = words.join(" ");
    label[..1].make_ascii_uppercase();
    label
}

struct Tile {
    axes: [i8; 3],
    points: [egui::Pos2; 4],
    face: usize,
}

impl Tile {
    fn contains(&self, point: egui::Pos2) -> bool {
        let cross = |i: usize| {
            let a = self.points[i];
            let b = self.points[(i + 1) % 4];
            (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x)
        };
        (0..4).all(|i| cross(i) >= 0.) || (0..4).all(|i| cross(i) <= 0.)
    }
    fn centre(&self) -> egui::Pos2 {
        self.points[0].lerp(self.points[2], 0.5)
    }
}

impl Shell {
    pub(super) fn view_cube(&mut self, ui: &mut egui::Ui, canvas: egui::Rect) {
        if canvas.width() < 140. || canvas.height() < 150. {
            return;
        }
        let home = egui::Rect::from_min_size(
            canvas.right_top() + Vec2::new(-98., 5.),
            Vec2::new(72., ui.spacing().interact_size.y),
        );
        let home = ui
            .put(home, egui::Button::new("Home"))
            .on_hover_text("Fit the isometric view");
        if home.clicked() {
            self.run_command(ui.ctx(), keys::Command::Iso);
        }
        let centre = home.rect.center_bottom() + Vec2::new(0., 56.);
        let bounds = egui::Rect::from_center_size(centre, Vec2::splat(104.));
        let eye = (self.camera.eye() - self.camera.target).normalize();
        let (right, up) = self.camera.screen_axes();
        let point = |v: Vec3| centre + Vec2::new(v.dot(right), -v.dot(up)) * 29.;
        let mut tiles = Vec::with_capacity(27);
        let mut anchors = BTreeMap::new();
        let edges = [-1., -0.56, 0.56, 1.];
        for face in 0..3 {
            if eye[face].abs() < 0.015 {
                continue;
            }
            let side = eye[face].signum();
            let a = (face + 1) % 3;
            let b = (face + 2) % 3;
            for row in 0..3 {
                for col in 0..3 {
                    let mut axes = [0; 3];
                    axes[face] = side as i8;
                    axes[a] = col as i8 - 1;
                    axes[b] = row as i8 - 1;
                    let points = [
                        [col, row],
                        [col + 1, row],
                        [col + 1, row + 1],
                        [col, row + 1],
                    ]
                    .map(|[x, y]| {
                        let mut v = Vec3::ZERO;
                        v[face] = side;
                        v[a] = edges[x];
                        v[b] = edges[y];
                        point(v)
                    });
                    let tile = Tile { axes, points, face };
                    let anchor = anchors.entry(axes).or_insert((0., tile.centre()));
                    if eye[face].abs() > anchor.0 {
                        *anchor = (eye[face].abs(), tile.centre());
                    }
                    tiles.push(tile);
                }
            }
        }
        let response = ui.interact(bounds, ui.scope_id().with("view_cube"), Sense::click());
        let pointer = ui.ctx().pointer_hover_pos();
        let hover = pointer
            .and_then(|pos| tiles.iter().find(|t| t.contains(pos)))
            .map(|t| t.axes);
        let mut choose = response.clicked().then_some(hover).flatten();
        for tile in &tiles {
            let face_label = tile.axes.iter().filter(|&&n| n != 0).count() == 1;
            let colour = if hover == Some(tile.axes) {
                ACCENT
            } else {
                [
                    Color32::from_rgb(175, 192, 205),
                    Color32::from_rgb(192, 207, 219),
                    Color32::from_rgb(218, 228, 235),
                ][tile.face]
            };
            ui.painter().add(egui::Shape::convex_polygon(
                tile.points.to_vec(),
                colour,
                egui::Stroke::new(0.5, Color32::from_black_alpha(70)),
            ));
            if face_label && eye[tile.face].abs() > 0.26 {
                ui.painter().text(
                    tile.centre(),
                    egui::Align2::CENTER_CENTER,
                    name(tile.axes),
                    egui::FontId::proportional(10.),
                    PANEL,
                );
            }
        }
        for (axes, (_, pos)) in anchors {
            let label = name(axes);
            let handle = ui.interact(
                egui::Rect::from_center_size(pos, Vec2::splat(4.)),
                ui.scope_id().with(("cube_view", axes)),
                Sense::click(),
            );
            handle.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, format!("View cube {label}"))
            });
            if handle.clicked() {
                choose = Some(axes);
            }
        }
        if let Some(axes) = choose {
            self.camera.set_view(view(axes));
            self.dirty = true;
            ui.ctx().request_repaint();
        }
    }
}
