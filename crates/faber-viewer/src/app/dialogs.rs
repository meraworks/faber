use super::*;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Wake, Waker},
};

pub(super) enum Choice {
    Folder(Option<PathBuf>),
    Project(Option<(PathBuf, PathBuf, bool)>),
    Drawing(PathBuf, String, Option<PathBuf>),
    FlatPattern(PathBuf, String, Option<PathBuf>),
    Invalid,
}

struct Repaint(egui::Context);

impl Wake for Repaint {
    fn wake(self: Arc<Self>) {
        self.0.request_repaint();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.request_repaint();
    }
}

pub(super) struct PendingDialog(Pin<Box<dyn Future<Output = Choice>>>);

impl PendingDialog {
    pub(super) fn new(f: impl Future<Output = Choice> + 'static) -> Self {
        Self(Box::pin(f))
    }

    pub(super) fn poll(&mut self, ctx: &egui::Context) -> Option<Choice> {
        let waker = Waker::from(Arc::new(Repaint(ctx.clone())));
        let mut cx = Context::from_waker(&waker);
        match self.0.as_mut().poll(&mut cx) {
            std::task::Poll::Pending => None,
            std::task::Poll::Ready(choice) => Some(choice),
        }
    }
}

impl Shell {
    pub(super) fn poll_dialog(&mut self, ctx: &egui::Context) {
        let Some(choice) = self.dialog.as_mut().and_then(|d| d.poll(ctx)) else {
            return;
        };
        self.dialog = None;
        match choice {
            Choice::Folder(Some(path)) => {
                if let Some(draft) = &mut self.project_draft {
                    draft.path = path.display().to_string();
                    self.window.dirty = true;
                }
            }
            Choice::Project(Some((source, target, archive))) => {
                self.start_project_load(ctx, source, target, None, archive);
            }
            Choice::Drawing(root, id, Some(path)) => self.sheets.file_chosen(&root, &id, path),
            Choice::FlatPattern(root, id, Some(path)) if self.root == root => {
                if let Some(p) = &mut self.sheet_metal {
                    p.file_chosen(&id, path);
                }
            }
            Choice::Invalid => self.error = Some("Select faber.json in the project folder.".into()),
            _ => {}
        }
    }
}

pub(super) fn project_choice(path: Option<PathBuf>) -> Choice {
    match path {
        None => Choice::Project(None),
        Some(path)
            if path
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("faber.json")) =>
        {
            let root = path.parent().unwrap_or(&path).to_owned();
            Choice::Project(Some((root.clone(), root, false)))
        }
        _ => Choice::Invalid,
    }
}

#[cfg(not(test))]
pub(super) fn open(frame: &eframe::Frame, root: &std::path::Path) -> PendingDialog {
    let picker = rfd::AsyncFileDialog::new()
        .set_parent(frame)
        .set_title("Open faber.json or a Fusion archive")
        .add_filter("Faber projects and Fusion archives", &["json", "f3d"]);
    let picker = if root.is_dir() {
        picker.set_directory(root)
    } else {
        picker
    };
    let destination = rfd::AsyncFileDialog::new()
        .set_parent(frame)
        .set_title("Choose a new folder name for the imported project");
    PendingDialog::new(async move {
        let path = picker.pick_file().await.map(|f| f.path().to_owned());
        if let Some(path) = path.as_ref().filter(|p| is_archive_path(p)) {
            let name = format!(
                "{}-Faber",
                path.file_stem().unwrap_or_default().to_string_lossy()
            );
            let saved = destination
                .set_directory(path.parent().unwrap_or(path))
                .set_file_name(name)
                .save_file()
                .await;
            Choice::Project(saved.map(|f| (path.clone(), f.path().to_owned(), true)))
        } else {
            project_choice(path)
        }
    })
}

#[cfg(not(test))]
pub(super) fn drawing_file(frame: &eframe::Frame, root: PathBuf, id: String) -> PendingDialog {
    let picker = rfd::AsyncFileDialog::new()
        .set_parent(frame)
        .set_title("Export drawing")
        .set_directory(&root)
        .set_file_name(format!("{id}.pdf"))
        .add_filter("Drawing", &["pdf", "svg", "dxf", "png"]);
    PendingDialog::new(async move {
        Choice::Drawing(
            root,
            id,
            picker.save_file().await.map(|f| f.path().to_owned()),
        )
    })
}
#[cfg(test)]
pub(super) fn drawing_file(_frame: &eframe::Frame, root: PathBuf, id: String) -> PendingDialog {
    PendingDialog::new(async move { Choice::Drawing(root, id, None) })
}

#[cfg(not(test))]
pub(super) fn flat_pattern_file(frame: &eframe::Frame, root: PathBuf, id: String) -> PendingDialog {
    let picker = rfd::AsyncFileDialog::new()
        .set_parent(frame)
        .set_title("Export flat pattern")
        .set_directory(&root)
        .set_file_name(format!("{id}.dxf"))
        .add_filter("Cutting pattern", &["dxf", "svg"]);
    PendingDialog::new(async move {
        Choice::FlatPattern(
            root,
            id,
            picker.save_file().await.map(|f| f.path().to_owned()),
        )
    })
}
#[cfg(test)]
pub(super) fn flat_pattern_file(
    _frame: &eframe::Frame,
    root: PathBuf,
    id: String,
) -> PendingDialog {
    PendingDialog::new(async move { Choice::FlatPattern(root, id, None) })
}
