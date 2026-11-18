# Material data

The library contains typical values for one grade or printed specimen per row.
A project material with the same id supplies all its own values. Use coupon
measurements for the actual process when a result depends on strength.

## Tensile data

For a printed polymer, the reference direction runs along the bed. The layer
factor is the ratio of upright tensile strength to reference tensile strength.
For sheets that give only yield and break stress, Faber takes the larger
reported stress in each direction as the peak estimate. Fracture can occur
before yield in the upright specimen. The table uses engineering stress in MPa.

| Material | Reference yield / peak | Upright peak / reference peak | Grade and source |
| --- | --- | --- | --- |
| PLA | 52.5 / 52.5 | 33.1 / 52.5 | [UltiMaker PLA, flat and upright specimens](https://ultimaker.com/materials/pla/) |
| PETG | 46.2 / 46.2 | 19.0 / 46.2 | [UltiMaker PETG, flat and upright specimens](https://ultimaker.com/materials/petg/) |
| ABS | 38.1 / 38.1 | 19.0 / 38.1 | [UltiMaker ABS, flat and upright specimens](https://um-support-files.ultimaker.com/materials/2.85mm/tds/ABS/Ultimaker-ABS-TDS-v5.00.pdf) |
| ASA | 32.8 / 32.8 | 28.3 / 32.8 | [Stratasys black ASA, F900, table 5, XZ and ZX](https://info.stratasysdirect.com/rs/626-SBR-192/images/Data%20Sheet%20-%20ASA%20EN.pdf) |
| PA12 | 32 / 46 | 38.5 / 46 | [Stratasys Nylon 12, conditioned, XZ and ZX](https://aerospace.stratasys.com/assets/downloads/end-of-arm-tooling/MSS_FDM_FDMNylon12_1117a.pdf) |
| PC | 53.3 / 53.3 | 25.2 / 53.3 | [UltiMaker PC, flat and upright specimens](https://um-support-files.ultimaker.com/materials/2.85mm/tds/PC/Ultimaker-PC-TDS-v5.00.pdf) |
| TPU 95A | 0.67 estimate / 23.7 | 6.4 / 23.7 | [UltiMaker TPU 95A, flat and upright specimens](https://um-support-files.ultimaker.com/materials/2.85mm/tds/TPU-95A/TDS-TPU95A-V2.01-250915.pdf) |
| Aluminium 6061-T6 | 276 / 310 | 1 | [Kaiser 6061 sheet, coil and plate, typical T6 values](https://online.kaiseraluminum.com/depot/PublicProductInformation/Document/1015/Kaiser_Aluminum_6061_Sheet_Coil_and_Plate.pdf) |
| Steel S235JR | 235 / 360 | 1 | [Ovako S235JR, small as-rolled sections](https://steelnavigator.ovako.com/steel-grades/s235/) |
| Stainless steel 304 | 230 / 540 | 1 | [Outokumpu Core 304/4301, cold-rolled strip, minimum proof and tensile strength](https://www.outokumpu.com/sv-se/products/product-ranges/-/media/files/products/core/outokumpu-core-range-datasheet.pdf) |
| Brass CW614N | 300 / 420 | 1 | [Bedra H04 rod, 15 to 35 mm, minimum proof and tensile strength](https://www.bedra.com/asia/products/bedra-rod-bar-new/high-speed-free-cutting-alloys/cw614n/) |

TPU has no measured tensile yield point in the source. The required yield
field holds the linear stress estimate at 1% strain, 67 MPa times 0.01. A yield
safety factor for TPU has no physical interpretation. Large strains require
a nonlinear material model. The single layer factor for each printed material
does not describe a full directional stiffness or failure law.

## Density, thermal properties and appearance

The density represents solid material. Porosity, infill and moisture change
printed mass. S235 uses the nominal density of 7.85 g/cm³. Ovako lists a
rounded value of 7.8. [S235JR physical properties](https://siderticino.it/en/steel-datasheets/s235jr/)

The aluminium, steel, stainless steel and brass rows use the thermal data from
the metal sources above. For S235, the values use the middle of the ranges
for heat capacity and conductivity. The brass heat capacity is 380 J/(kg K).
[CW614N rod properties](https://www.nordicbrass.se/download/611-481B4799B166CEF12ACC9B01A958AF84/CW614N_Rod_eng_utg-5.pdf)

Thermal data for the polymers are estimates from related materials. Conductivity
and heat capacity depend on temperature and porosity.

- PLA uses 0.13 W/(m K) and 1800 J/(kg K). [PLA conductivity](https://www.sciencedirect.com/science/article/pii/S2352710223001584) and [FFF heat transfer, table 1](https://doi.org/10.1016/j.jmapro.2023.03.030).
- ABS, ASA and PETG use the thermal ranges for ABS, ASA and recycled PETG in table 2. [Tensile specimen thermal model](https://www.mdpi.com/1424-8220/25/17/5494).
- PA12 and PC use solid stock values. Linear expansion uses the 23–60 °C interval. [TECAMID 12 natural](https://www.ensingerplastics.com/en/shapes/pa12-tecamid-12-natural) and [TECANAT natural](https://www.ensingerplastics.com/en-us/shapes/polycarbonate-tecanat-natural).
- TPU uses values within the room-temperature ranges for related TPU grades. [BASF Elastollan material properties, table 1](https://download.basf.com/p1/8a8082587fd4b608017ff4e900830157/en/Elastollan%C2%AE_%E2%80%93_Material_Properties_Brochure_English.pdf).

The Poisson ratios for the polymers, aluminium, stainless steel and brass are
isotropic estimates. The expansion coefficients for PLA, PETG, ABS, ASA and
TPU are family estimates in µm/(m K). Supply measured values for a thermal
stress study. Colour, roughness and metallic appearance are display settings.
