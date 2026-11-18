# Fusion surface display evidence

Fusion is the behavioural reference for core CAD work. Open-source projects
provide implementation examples. Do not describe an inference as a Fusion fact.

## Target and method

REA 6.7.0 with Ghidra 12.1.4 inspected the installed Fusion module
`ASMFCT232.dll`, from deployment `39e8a3d5e919c70b05ef3ebfe9c6bfc8d9ee001e`.
SHA-256: `588fbf376611c21d83e5f7ffdd16e4d10bf3d365286be06c62bfa8223a3b5f5a`.
This was static analysis. It did not run Fusion or modify its installed files.
The complete local records are in `.cargo/fusion-facet-evidence.json`.

## Observations

- `REFINEMENT::set_surface_deviation` and its `set_surface_tol` alias store a
  surface tolerance separately from the normal tolerance. Address `0x180050220`.
  Evidence: `ev_eef64d35d803c19cc1eb30c980d3c5c5495b6fc78db276162bfbf2549e60adc7`.
- `REFINEMENT::set_normal_deviation` and `set_normal_tol` store a separate value.
  Address `0x18004fff0`.
  Evidence: `ev_c5f9929ee4361c4935639466d6eb697ab561fdd1eb7f3a646301748209963fe7`.
- The refinement constructor receives both values as inputs. It does not prove
  which values the Fusion viewport selects. Address `0x18004da60`.
  Evidence: `ev_c145a806fb02cbbc9c61b6fdc9083c899a5bfc5d96380397f335485946e48ef0`.
- `indexed_polygon::normal_transition` computes the polygon normal, compares it
  with each stored vertex normal and returns the greatest angle. Address
  `0x180106b30`.
  Evidence: `ev_1d4ad2b6bbb8b4f6a9c68ae7888bd938adb1c1da6ad89f455fe5a13763e4892f`.
- `af_node_instance::normal` reads three stored normal components when a normal
  mapping exists. Address `0x18004c1f0`.
  Evidence: `ev_8ca05dd975add8e10bad8a08db52a0917c56d4f4f6789dc167afd19fe811bba5`.
- The symbol inventory includes silhouette, pixel-area and view-dependent
  refinement controls. Their runtime use remains unverified.
  Evidence: `ev_30d9fe96d2f5d07ba715e34da9a84079561e0452cd737c86ee6fc3a4986796f3`.

## Decision for Faber

Keep exact surfaces separate from their display triangles. Control chord and
angular errors. Carry surface normals to the renderer. Do not blur real face
boundaries by averaging normals across every nearby face. Use the existing
geometry adapter to calculate normals from the surface; see the separate
[open CAD research](2026-11-18-open-cad-surface-research.md).

Faber's 0.03 mm chord tolerance and 0.1 radian angular limit are implementation
choices, not recovered Fusion defaults. Angle-weighted smoothing is a fallback
for mesh input that has no CAD surface data.

## Limits

The inspected module is one component of Fusion. Exported functions and static
calls do not establish the active viewport path. Direct callers of the inspected
normal-transition function were not resolved; indirect calls remain unknown.
This investigation does not establish full algorithm parity, automatic
screen-dependent refinement, Fusion's lighting values or its exact export presets.
