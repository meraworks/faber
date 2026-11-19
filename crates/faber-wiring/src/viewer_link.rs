//! A link between the MCP server and a running viewer of the same project.
//!
//! Both sides use files in the `.faber` folder of the project. These files are not
//! project data: they do not change the revision or the fingerprint.
//! - The server writes `viewer-command.json` with a new sequence number.
//! - The viewer applies each sequence number once and writes `viewer-state.json`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const COMMAND_FILE: &str = "viewer-command.json";
pub const STATE_FILE: &str = "viewer-state.json";

/// A camera direction for the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ViewDirection {
    Iso,
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

/// One change of the viewer display. No action changes the project files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewerAction {
    /// Hide these parts.
    Hide { parts: Vec<String> },
    /// Show these parts again.
    Show { parts: Vec<String> },
    /// Show only these parts.
    Isolate { parts: Vec<String> },
    /// Show all parts.
    ShowAll,
    /// Select these parts. An empty list clears the selection.
    Select { parts: Vec<String> },
    /// Turn the camera to a direction. Frame the listed parts, or all visible parts.
    Camera {
        #[serde(default)]
        view: Option<ViewDirection>,
        #[serde(default)]
        parts: Vec<String>,
    },
    /// Show a saved section as the live cut. Null turns the live cut off.
    Section { id: Option<String> },
    /// Apply a saved view: camera, hidden parts, section, style and pose.
    ApplyView { name: String },
}

/// A command file. The viewer applies a sequence number once.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewerCommand {
    pub seq: u64,
    pub actions: Vec<ViewerAction>,
}

/// A face or edge that the person picked in the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PickedItem {
    pub part: String,
    pub kind: String,
    pub index: u32,
}

/// What the viewer shows now. The viewer writes it after each command and each change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewerState {
    /// The last command sequence number that the viewer applied.
    pub applied: u64,
    /// Messages from the last command, for example a part that was not found.
    #[serde(default)]
    pub notices: Vec<String>,
    pub revision: u64,
    pub hidden: Vec<String>,
    pub selected: Vec<String>,
    #[serde(default)]
    pub picked: Vec<PickedItem>,
    pub section: Option<String>,
    pub section_on: bool,
}

fn folder(root: &Path) -> PathBuf {
    root.join(".faber")
}

/// Writes a file in one step, so that the reader never sees half a file.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("The link file has no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&temp, text).map_err(|e| format!("Cannot write {}: {e}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("Cannot write {}: {e}", path.display())
    })
}

pub fn read_command(root: &Path) -> Option<ViewerCommand> {
    let text = std::fs::read_to_string(folder(root).join(COMMAND_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn read_state(root: &Path) -> Option<ViewerState> {
    let text = std::fs::read_to_string(folder(root).join(STATE_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_state(root: &Path, state: &ViewerState) -> Result<(), String> {
    let text = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    write_atomic(&folder(root).join(STATE_FILE), &text)
}

/// The modification time of the command file. The viewer reads the file only when it changes.
pub fn command_stamp(root: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(folder(root).join(COMMAND_FILE))
        .and_then(|m| m.modified())
        .ok()
}

/// Writes a command with the next sequence number and returns that number.
pub fn send(root: &Path, actions: Vec<ViewerAction>) -> Result<u64, String> {
    let last = read_command(root).map_or(0, |c| c.seq);
    let applied = read_state(root).map_or(0, |s| s.applied);
    let seq = last.max(applied) + 1;
    let command = ViewerCommand { seq, actions };
    let text = serde_json::to_string_pretty(&command).map_err(|e| e.to_string())?;
    write_atomic(&folder(root).join(COMMAND_FILE), &text)?;
    Ok(seq)
}

/// Waits until the viewer applied the command, or until the time ends.
pub fn wait_applied(root: &Path, seq: u64, timeout: Duration) -> Option<ViewerState> {
    let start = Instant::now();
    loop {
        if let Some(state) = read_state(root)
            && state.applied >= seq
        {
            return Some(state);
        }
        if start.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_increments_the_sequence_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let first = send(dir.path(), vec![ViewerAction::ShowAll]).unwrap();
        let second = send(
            dir.path(),
            vec![ViewerAction::Hide {
                parts: vec!["roll".into()],
            }],
        )
        .unwrap();
        assert_eq!((first, second), (1, 2));
        let command = read_command(dir.path()).unwrap();
        assert_eq!(command.seq, 2);
        assert_eq!(
            command.actions,
            vec![ViewerAction::Hide {
                parts: vec!["roll".into()]
            }]
        );
    }

    #[test]
    fn the_next_sequence_follows_the_applied_state() {
        let dir = tempfile::tempdir().unwrap();
        write_state(
            dir.path(),
            &ViewerState {
                applied: 7,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(send(dir.path(), vec![ViewerAction::ShowAll]).unwrap(), 8);
    }

    #[test]
    fn wait_returns_none_without_a_viewer() {
        let dir = tempfile::tempdir().unwrap();
        let seq = send(dir.path(), vec![ViewerAction::ShowAll]).unwrap();
        assert!(wait_applied(dir.path(), seq, Duration::from_millis(120)).is_none());
    }

    #[test]
    fn actions_read_the_mcp_shape() {
        let action: ViewerAction =
            serde_json::from_str(r#"{"action":"camera","view":"top","parts":["clamp"]}"#).unwrap();
        assert_eq!(
            action,
            ViewerAction::Camera {
                view: Some(ViewDirection::Top),
                parts: vec!["clamp".into()]
            }
        );
        assert!(
            serde_json::from_str::<ViewerAction>(r#"{"action":"hide","parts":[],"x":1}"#).is_err()
        );
    }
}
