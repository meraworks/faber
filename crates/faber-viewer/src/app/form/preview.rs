use super::*;
use faber_form::Cage;
use faber_render::SurfacePaint;
pub(super) struct Request {
    pub serial: u64,
    pub cage: Arc<CageData<f64>>,
    pub op: Option<FormOp>,
    pub sym: Vec<SymmetryPlane>,
    pub frame: DAffine3,
}
pub(super) struct Wire {
    pub cage: CageData<f64>,
    pub edges: Vec<[u32; 2]>,
}
type Answer = (u64, Result<(Wire, SurfacePaint), String>);
pub(super) struct Worker {
    requests: mpsc::Sender<Request>,
    pub answers: mpsc::Receiver<Answer>,
}
impl Worker {
    pub fn new(part: String, device: egui_wgpu::wgpu::Device, ctx: egui::Context) -> Self {
        let (requests, rx) = mpsc::channel::<Request>();
        let (tx, answers) = mpsc::channel();
        std::thread::spawn(move || {
            let mut base: Option<(Arc<CageData<f64>>, Cage)> = None;
            while let Ok(first) = rx.recv() {
                let latest = rx.try_iter().last().unwrap_or(first);
                let id = latest.serial;
                let result = (|| {
                    if base
                        .as_ref()
                        .is_none_or(|(source, _)| !Arc::ptr_eq(source, &latest.cage))
                    {
                        base = Some((latest.cage.clone(), Cage::from_data(&latest.cage)?));
                    }
                    let original = &base.as_ref().unwrap().1;
                    let edited = latest
                        .op
                        .map(|op| faber_form::apply(original, &[op], &latest.sym))
                        .transpose()?;
                    let cage = edited.as_ref().unwrap_or(original);
                    let mut mesh = faber_form::subdivide(cage, 2)?;
                    for v in &mut mesh.vertices {
                        *v = latest.frame.transform_point3(DVec3::from(*v)).into();
                    }
                    for n in &mut mesh.normals {
                        *n = latest
                            .frame
                            .transform_vector3(DVec3::from(*n))
                            .normalize_or_zero()
                            .into();
                    }
                    let paint = SurfacePaint::lit(
                        &device,
                        &part,
                        &mesh,
                        [88, 171, 176],
                        &Default::default(),
                    )?;
                    Ok((
                        Wire {
                            edges: cage.edges(),
                            cage: cage.to_data(),
                        },
                        paint,
                    ))
                })();
                if tx.send((id, result)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { requests, answers }
    }
    pub fn request(&self, job: Request) {
        let _ = self.requests.send(job);
    }
}
