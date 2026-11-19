mod common;
use common::*;
use faber_cam::*;
use faber_model::machining::*;
use std::f64::consts::PI;
fn setup(leave: f64) -> Operation {
    let mut o = op(OperationKind::Pocket {
        floor: Selection::Z(-5.),
        stepover: 3.,
        stock_to_leave: leave,
    });
    o.heights.bottom = Some(-5.);
    o.stepdown = Some(5.);
    o
}
fn stock() -> StockBox {
    StockBox {
        min: [-2., -2., -10.],
        max: [42., 32., 0.],
    }
}
#[test]
fn rectangular_pocket_is_cleared_and_entry_is_two_degrees() {
    let mesh = part(vec![rect(5., 5., 30., 20.)], 5.);
    let path = pocket(&mesh, &stock(), &tool(6.), &setup(0.)).unwrap();
    let mut floor = Vec::new();
    let mut ramp = 0;
    for w in path.moves.windows(2) {
        let (Some(a), Some(b)) = (w[0].target(), w[1].target()) else {
            continue;
        };
        if !matches!(w[1], Move::Feed { .. }) {
            continue;
        }
        if a[2] == -5. && b[2] == -5. {
            floor.push((a, b));
        }
        let xy = (b[0] - a[0]).hypot(b[1] - a[1]);
        if b[2] < a[2] && xy > 1e-7 {
            let angle = ((a[2] - b[2]) / xy).atan().to_degrees();
            assert!(angle <= 2.0001);
            ramp += 1;
        }
    }
    assert!(ramp >= 12);
    let mut uncut = 0;
    for x in 0..300 {
        for y in 0..200 {
            let q = [5.05 + f64::from(x) * 0.1, 5.05 + f64::from(y) * 0.1];
            if !floor.iter().any(|&(a, b)| distance(q, a, b) <= 3.) {
                uncut += 1;
            }
        }
    }
    let target = 4. * (9. - PI * 9. / 4.);
    assert!(
        (f64::from(uncut) * 0.01 - target).abs() < 0.1 * target,
        "uncut {uncut}"
    );
    let left = pocket(&mesh, &stock(), &tool(6.), &setup(0.3)).unwrap();
    let xmin = left
        .moves
        .iter()
        .filter_map(|m| {
            if let Move::Feed { to, .. } = m {
                Some(to[0])
            } else {
                None
            }
        })
        .fold(f64::INFINITY, f64::min);
    assert!((xmin - 8.3).abs() < 0.02);
}
#[test]
fn tool_larger_than_the_pocket_is_refused() {
    let mesh = part(vec![rect(5., 5., 20., 10.)], 5.);
    assert_eq!(
        pocket(&mesh, &stock(), &tool(12.), &setup(0.)).unwrap_err(),
        "Tool diameter 12 mm does not fit the pocket width 10 mm."
    );
}
#[test]
fn rings_are_ordered_inside_out_and_keep_an_island() {
    let region = Region {
        outer: rect(0., 0., 50., 40.),
        holes: vec![rect(19., 16., 12., 8.)],
    };
    let rings = faber_cam::pocket::clear(&[region], 3., 3.).unwrap();
    assert!(!rings.is_empty());
    assert!(
        rings
            .last()
            .unwrap()
            .iter()
            .any(|p| (p[0] - 3.).abs() < 0.011)
            || rings[rings.len() - 2]
                .iter()
                .any(|p| (p[0] - 3.).abs() < 0.011)
    );
    for ring in rings {
        assert_eq!(ring.first(), ring.last());
        for edge in ring.windows(2) {
            for i in 0..=10 {
                let t = f64::from(i) / 10.;
                let x = edge[0][0] * (1. - t) + edge[1][0] * t;
                let y = edge[0][1] * (1. - t) + edge[1][1] * t;
                let gap = (19. - x)
                    .max(x - 31.)
                    .max(0.)
                    .hypot((16. - y).max(y - 24.).max(0.));
                assert!(gap > 2.985, "{x}, {y}: {gap}");
            }
        }
    }
}

#[test]
fn selected_floor_only_clears_its_own_pocket() {
    let mesh = part(vec![rect(2., 3., 14., 20.), rect(24., 3., 14., 20.)], 5.);
    let selected = faber_ports::Mesh {
        vertices: vec![
            [2., 3., -5.],
            [16., 3., -5.],
            [16., 23., -5.],
            [2., 23., -5.],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        normals: vec![],
    };
    let path = pocket_on_floor(&mesh, &stock(), &tool(4.), &setup(0.), &selected).unwrap();
    assert!(
        path.moves
            .iter()
            .filter_map(Move::target)
            .all(|p| p[0] <= 14.01)
    );
    assert!(path.cut_length() > 30.);
    let all = pocket(&mesh, &stock(), &tool(4.), &setup(0.)).unwrap();
    assert!(
        all.moves
            .iter()
            .filter_map(Move::target)
            .any(|p| p[0] > 25.)
    );
}

#[test]
fn full_depth_pocket_and_ramp_keep_the_island() {
    use faber_model::{Geometry, Profile};
    use faber_ports::Kernel;
    let mut mesh = part(vec![rect(3., 3., 34., 24.)], 5.);
    let island = faber_geometry::NativeKernel
        .build(&Geometry::Extrude {
            profile: Profile {
                outer: rect(16., 12., 8., 6.),
                holes: vec![],
            },
            height: 5.,
        })
        .unwrap();
    let offset = mesh.vertices.len() as u32;
    mesh.vertices
        .extend(island.vertices.iter().map(|p| [p[0], p[1], p[2] - 5.]));
    mesh.triangles.extend(
        island
            .triangles
            .iter()
            .map(|tri| tri.map(|index| index + offset)),
    );
    let path = pocket(&mesh, &stock(), &tool(6.), &setup(0.)).unwrap();
    for pair in path.moves.windows(2) {
        let Some(a) = pair[0].target() else { continue };
        let Move::Feed { to: b, .. } = pair[1] else {
            continue;
        };
        for i in 0..=32 {
            let t = f64::from(i) / 32.;
            let x = a[0] + t * (b[0] - a[0]);
            let y = a[1] + t * (b[1] - a[1]);
            let z = a[2] + t * (b[2] - a[2]);
            if z >= 0. {
                continue;
            }
            let gap = (16. - x)
                .max(x - 24.)
                .max(0.)
                .hypot((12. - y).max(y - 18.).max(0.));
            assert!(gap > 2.985, "Island clearance at {x}, {y}, {z} = {gap}");
        }
    }
}
