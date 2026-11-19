use super::*;
use faber_wiring::viewer_link::{self, ViewerAction, ViewerState};
use std::time::Duration;

/// How long the tool waits for a running viewer to apply a command.
const WAIT: Duration = Duration::from_secs(3);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewerCall {
    /// hide, show, isolate, show_all, select, camera, section or apply_view. Omit to read.
    action: Option<String>,
    #[serde(default)]
    parts: Vec<String>,
    view: Option<String>,
    name: Option<String>,
}

impl ViewerCall {
    fn action(self) -> Result<ViewerAction, String> {
        let parts = self.parts;
        let name = || self.name.clone().ok_or("Give a name.".to_string());
        Ok(match self.action.as_deref().unwrap_or_default() {
            "hide" => ViewerAction::Hide { parts },
            "show" => ViewerAction::Show { parts },
            "isolate" => ViewerAction::Isolate { parts },
            "show_all" => ViewerAction::ShowAll,
            "select" => ViewerAction::Select { parts },
            "camera" => ViewerAction::Camera {
                view: match &self.view {
                    Some(v) => Some(serde_json::from_value(Value::String(v.clone())).map_err(
                        |_| {
                            format!("View {v} is not iso, front, back, left, right, top or bottom.")
                        },
                    )?),
                    None => None,
                },
                parts,
            },
            "section" => ViewerAction::Section {
                id: self.name.clone(),
            },
            "apply_view" => ViewerAction::ApplyView { name: name()? },
            other => {
                return Err(format!(
                    "Action {other} is not hide, show, isolate, show_all, select, camera, section or apply_view."
                ));
            }
        })
    }

    pub fn run(self, root: &Path, p: &Project) -> Result<Value, String> {
        if self.action.is_none() {
            return Ok(match viewer_link::read_state(root) {
                Some(state) => reply(&state),
                None => json!({"viewer":false,"note":NO_VIEWER}),
            });
        }
        let action = self.action()?;
        check(&action, p)?;
        let seq = viewer_link::send(root, vec![action])?;
        Ok(match viewer_link::wait_applied(root, seq, WAIT) {
            Some(state) => reply(&state),
            None => json!({"viewer":false,"applied":false,"note":NO_VIEWER}),
        })
    }
}

const NO_VIEWER: &str =
    "No viewer shows this project. Open it in faber-viewer and send the actions again.";

/// Refuses an action that names a part, a view or a section that the project does not have.
fn check(action: &ViewerAction, p: &Project) -> Result<(), String> {
    let parts = match action {
        ViewerAction::Hide { parts }
        | ViewerAction::Show { parts }
        | ViewerAction::Isolate { parts }
        | ViewerAction::Select { parts }
        | ViewerAction::Camera { parts, .. } => parts.as_slice(),
        ViewerAction::ApplyView { name } => {
            if !p.data.views.contains_key(name) {
                return Err(format!("View {name} was not found."));
            }
            &[]
        }
        ViewerAction::Section { id: Some(id) } => {
            if !p.data.sections.contains_key(id) {
                return Err(format!("Section {id} was not found."));
            }
            &[]
        }
        ViewerAction::Section { id: None } | ViewerAction::ShowAll => &[],
    };
    if let Some(missing) = parts.iter().find(|id| !p.data.parts.contains_key(*id)) {
        return Err(format!("Part {missing} was not found."));
    }
    Ok(())
}

fn reply(state: &ViewerState) -> Value {
    let mut value = json!({
        "viewer": true,
        "applied": true,
        "revision": state.revision,
        "hidden": state.hidden,
        "selected": state.selected,
        "section": if state.section_on { state.section.clone() } else { None },
    });
    if !state.picked.is_empty() {
        value["picked"] = json!(
            state
                .picked
                .iter()
                .map(|p| json!({"part":p.part,"kind":p.kind,"index":p.index}))
                .collect::<Vec<_>>()
        );
    }
    if !state.notices.is_empty() {
        value["notices"] = json!(state.notices);
    }
    value
}
