use super::*;

pub(super) fn selector(
    ui: &mut egui::Ui,
    data: &ProjectData,
    body: &Body,
    enabled: bool,
) -> Option<String> {
    let part = data.parts.get(&body.id)?;
    let current_id = part.material.as_deref();
    let current = current_id.and_then(|id| faber_core::materials::get(data, id).ok());
    let selected_name = current
        .as_ref()
        .map(|row| row.name.as_str())
        .or(current_id)
        .unwrap_or("Unassigned");
    let mut choice = None;
    ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_label("Material")
            .selected_text(selected_name)
            .truncate()
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(current_id.is_none(), "Unassigned")
                    .clicked()
                    && current_id.is_some()
                {
                    choice = Some(String::new());
                }
                for id in faber_core::materials::ids(data) {
                    if let Ok(row) = faber_core::materials::get(data, id)
                        && ui
                            .selectable_label(current_id == Some(id), row.name)
                            .on_hover_text(id)
                            .clicked()
                        && current_id != Some(id)
                    {
                        choice = Some(id.to_owned());
                    }
                }
            });
    });
    if let Some(row) = current {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if body.shape.is_some() {
                    "Mass"
                } else {
                    "Mass estimate"
                })
                .color(MUTED),
            );
            ui.label(format!("{:.2} g", body.volume / 1000. * row.density_g_cm3));
        });
    }
    choice
}

impl Shell {
    pub(super) fn save_material(&mut self, ctx: &egui::Context, id: String, material: String) {
        let root = self.root.clone();
        let expected = self.data.clone();
        let strict = self.strict_edits;
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.saving = true;
        self.error = None;
        std::thread::spawn(move || {
            let result = (|| {
                let mut project = faber_wiring::open(&root, false, None)?;
                if project.data != expected {
                    return Err("Project changed. Select the material again.".into());
                }
                let change = faber_core::Operation::set_part_info(faber_core::PartInfo {
                    id,
                    material: Some(material),
                    ..Default::default()
                });
                project.batch_operations(vec![change], strict)?;
                Ok((project.data, project.bodies))
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}
