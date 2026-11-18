use crate::solve::Elastic;
use glam::DVec3;

/// The corner order is X + 2Y + 4Z. Return the shape gradients in millimetres.
pub(crate) fn gradients(h: f64, natural: [f64; 3]) -> [DVec3; 8] {
    std::array::from_fn(|node| {
        let sign: [f64; 3] =
            std::array::from_fn(|axis| if node & (1 << axis) == 0 { -1. } else { 1. });
        let t = std::array::from_fn::<_, 3, _>(|i| 1. + sign[i] * natural[i]);
        DVec3::new(
            sign[0] * t[1] * t[2],
            sign[1] * t[0] * t[2],
            sign[2] * t[0] * t[1],
        ) / (4. * h)
    })
}

impl Elastic {
    pub(crate) fn lame(&self) -> (f64, f64) {
        let shear = self.youngs_mpa / (2. * (1. + self.poisson));
        (2. * shear * self.poisson / (1. - 2. * self.poisson), shear)
    }
}

/// Stiffness of a linear eight-node cube with full Gauss integration.
pub fn element_matrix(material: &Elastic, h: f64) -> [[f64; 24]; 24] {
    let mut k = [[0.; 24]; 24];
    let (lambda, mu) = material.lame();
    let a = 1. / 3_f64.sqrt();
    for x in [-a, a] {
        for y in [-a, a] {
            for z in [-a, a] {
                let grad = gradients(h, [x, y, z]);
                let weight = (h / 2.).powi(3);
                for (i, gi) in grad.iter().enumerate() {
                    for (j, gj) in grad.iter().enumerate() {
                        for u in 0..3 {
                            for v in 0..3 {
                                k[i * 3 + u][j * 3 + v] += weight
                                    * (lambda * gi[u] * gj[v]
                                        + mu * gi[v] * gj[u]
                                        + if u == v { mu * gi.dot(*gj) } else { 0. });
                            }
                        }
                    }
                }
            }
        }
    }
    k
}
