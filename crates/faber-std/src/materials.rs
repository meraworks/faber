//! Typical bulk and printed material properties. See `docs/material-data.md`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Polymer,
    Metal,
}

#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub id: &'static str,
    pub name: &'static str,
    pub class: Class,
    pub density_g_cm3: f64,
    pub youngs_mpa: f64,
    pub poisson: f64,
    pub yield_mpa: f64,
    pub ultimate_mpa: f64,
    pub conductivity_w_mk: f64,
    pub expansion_um_mk: f64,
    pub specific_heat_j_kgk: f64,
    pub layer_strength_factor: f64,
    pub colour: [u8; 3],
    pub roughness: f64,
    pub metallic: f64,
    pub source: &'static str,
}

pub fn material(id: &str) -> Option<&'static Material> {
    ROWS.iter().find(|row| row.id == id)
}

pub fn materials() -> &'static [Material] {
    ROWS
}

const ROWS: &[Material] = &[
    Material {
        id: "abs",
        name: "ABS",
        class: Class::Polymer,
        density_g_cm3: 1.1,
        youngs_mpa: 1962.0,
        poisson: 0.35,
        yield_mpa: 38.1,
        ultimate_mpa: 38.1,
        conductivity_w_mk: 0.16,
        expansion_um_mk: 85.0,
        specific_heat_j_kgk: 1450.0,
        layer_strength_factor: 19.0 / 38.1,
        colour: [55, 60, 67],
        roughness: 0.64,
        metallic: 0.0,
        source: "https://ultimaker.com/materials/abs/",
    },
    Material {
        id: "aluminium-6061-t6",
        name: "Aluminium 6061-T6",
        class: Class::Metal,
        density_g_cm3: 2.7,
        youngs_mpa: 68300.0,
        poisson: 0.33,
        yield_mpa: 276.0,
        ultimate_mpa: 310.0,
        conductivity_w_mk: 167.0,
        expansion_um_mk: 23.6,
        specific_heat_j_kgk: 896.0,
        layer_strength_factor: 1.0,
        colour: [192, 198, 202],
        roughness: 0.33,
        metallic: 1.0,
        source: "https://online.kaiseraluminum.com/depot/PublicProductInformation/Document/1015/Kaiser_Aluminum_6061_Sheet_Coil_and_Plate.pdf",
    },
    Material {
        id: "asa",
        name: "ASA",
        class: Class::Polymer,
        density_g_cm3: 1.08,
        youngs_mpa: 2140.0,
        poisson: 0.35,
        yield_mpa: 32.8,
        ultimate_mpa: 32.8,
        conductivity_w_mk: 0.2,
        expansion_um_mk: 85.0,
        specific_heat_j_kgk: 1400.0,
        layer_strength_factor: 28.3 / 32.8,
        colour: [195, 194, 183],
        roughness: 0.64,
        metallic: 0.0,
        source: "https://info.stratasysdirect.com/rs/626-SBR-192/images/Data%20Sheet%20-%20ASA%20EN.pdf",
    },
    Material {
        id: "brass-cw614n",
        name: "Brass CW614N",
        class: Class::Metal,
        density_g_cm3: 8.5,
        youngs_mpa: 96000.0,
        poisson: 0.34,
        yield_mpa: 300.0,
        ultimate_mpa: 420.0,
        conductivity_w_mk: 118.0,
        expansion_um_mk: 20.9,
        specific_heat_j_kgk: 380.0,
        layer_strength_factor: 1.0,
        colour: [199, 160, 89],
        roughness: 0.3,
        metallic: 1.0,
        source: "https://www.bedra.com/asia/products/bedra-rod-bar-new/high-speed-free-cutting-alloys/cw614n/",
    },
    Material {
        id: "pa12",
        name: "Polyamide 12",
        class: Class::Polymer,
        density_g_cm3: 1.0,
        youngs_mpa: 1282.0,
        poisson: 0.39,
        yield_mpa: 32.0,
        ultimate_mpa: 46.0,
        conductivity_w_mk: 0.3,
        expansion_um_mk: 150.0,
        specific_heat_j_kgk: 1800.0,
        layer_strength_factor: 38.5 / 46.0,
        colour: [203, 201, 192],
        roughness: 0.7,
        metallic: 0.0,
        source: "https://aerospace.stratasys.com/assets/downloads/end-of-arm-tooling/MSS_FDM_FDMNylon12_1117a.pdf",
    },
    Material {
        id: "pc",
        name: "Polycarbonate",
        class: Class::Polymer,
        density_g_cm3: 1.19,
        youngs_mpa: 2394.0,
        poisson: 0.37,
        yield_mpa: 53.3,
        ultimate_mpa: 53.3,
        conductivity_w_mk: 0.25,
        expansion_um_mk: 80.0,
        specific_heat_j_kgk: 1300.0,
        layer_strength_factor: 25.2 / 53.3,
        colour: [196, 213, 224],
        roughness: 0.34,
        metallic: 0.0,
        source: "https://um-support-files.ultimaker.com/materials/2.85mm/tds/PC/Ultimaker-PC-TDS-v5.00.pdf",
    },
    Material {
        id: "petg",
        name: "PETG",
        class: Class::Polymer,
        density_g_cm3: 1.27,
        youngs_mpa: 1939.0,
        poisson: 0.38,
        yield_mpa: 46.2,
        ultimate_mpa: 46.2,
        conductivity_w_mk: 0.27,
        expansion_um_mk: 65.0,
        specific_heat_j_kgk: 1100.0,
        layer_strength_factor: 19.0 / 46.2,
        colour: [78, 152, 164],
        roughness: 0.43,
        metallic: 0.0,
        source: "https://ultimaker.com/materials/petg/",
    },
    Material {
        id: "pla",
        name: "PLA",
        class: Class::Polymer,
        density_g_cm3: 1.24,
        youngs_mpa: 3250.0,
        poisson: 0.36,
        yield_mpa: 52.5,
        ultimate_mpa: 52.5,
        conductivity_w_mk: 0.13,
        expansion_um_mk: 70.0,
        specific_heat_j_kgk: 1800.0,
        layer_strength_factor: 33.1 / 52.5,
        colour: [129, 180, 168],
        roughness: 0.58,
        metallic: 0.0,
        source: "https://ultimaker.com/materials/pla/",
    },
    Material {
        id: "stainless-304",
        name: "Stainless steel 304",
        class: Class::Metal,
        density_g_cm3: 7.9,
        youngs_mpa: 200000.0,
        poisson: 0.29,
        yield_mpa: 230.0,
        ultimate_mpa: 540.0,
        conductivity_w_mk: 15.0,
        expansion_um_mk: 16.0,
        specific_heat_j_kgk: 500.0,
        layer_strength_factor: 1.0,
        colour: [184, 187, 191],
        roughness: 0.24,
        metallic: 1.0,
        source: "https://www.outokumpu.com/sv-se/products/product-ranges/-/media/files/products/core/outokumpu-core-range-datasheet.pdf",
    },
    Material {
        id: "steel-s235",
        name: "Steel S235JR",
        class: Class::Metal,
        density_g_cm3: 7.85,
        youngs_mpa: 210000.0,
        poisson: 0.3,
        yield_mpa: 235.0,
        ultimate_mpa: 360.0,
        conductivity_w_mk: 42.5,
        expansion_um_mk: 12.0,
        specific_heat_j_kgk: 470.0,
        layer_strength_factor: 1.0,
        colour: [133, 143, 151],
        roughness: 0.45,
        metallic: 1.0,
        source: "https://steelnavigator.ovako.com/steel-grades/s235/",
    },
    Material {
        id: "tpu-95a",
        name: "TPU 95A",
        class: Class::Polymer,
        density_g_cm3: 1.22,
        youngs_mpa: 67.0,
        poisson: 0.49,
        yield_mpa: 0.67,
        ultimate_mpa: 23.7,
        conductivity_w_mk: 0.2,
        expansion_um_mk: 160.0,
        specific_heat_j_kgk: 2000.0,
        layer_strength_factor: 6.4 / 23.7,
        colour: [77, 87, 92],
        roughness: 0.72,
        metallic: 0.0,
        source: "https://um-support-files.ultimaker.com/materials/2.85mm/tds/TPU-95A/TDS-TPU95A-V2.01-250915.pdf",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_row_is_plausible() {
        let library = materials();
        assert_eq!(library.len(), 11);
        for id in [
            "pla",
            "petg",
            "abs",
            "asa",
            "pa12",
            "pc",
            "tpu-95a",
            "aluminium-6061-t6",
            "steel-s235",
            "stainless-304",
            "brass-cw614n",
        ] {
            let row = material(id).unwrap();
            assert_eq!(row.id, id);
            assert!(!row.name.is_empty() && row.source.starts_with("https://"));
            assert!(row.poisson > 0. && row.poisson < 0.5);
            for value in [
                row.density_g_cm3,
                row.youngs_mpa,
                row.yield_mpa,
                row.ultimate_mpa,
                row.conductivity_w_mk,
                row.expansion_um_mk,
                row.specific_heat_j_kgk,
            ] {
                assert!(value.is_finite() && value > 0., "{id}");
            }
            assert!(row.yield_mpa <= row.ultimate_mpa);
            assert!(row.layer_strength_factor > 0. && row.layer_strength_factor <= 1.);
            if row.class == Class::Metal {
                assert_eq!(row.layer_strength_factor, 1.);
            }
            assert!((0. ..=1.).contains(&row.roughness) && (0. ..=1.).contains(&row.metallic));
        }
        assert!(material("missing").is_none());
        assert_eq!(material("steel-s235").unwrap().density_g_cm3, 7.85);
    }
}
