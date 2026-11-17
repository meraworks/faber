# Curved parts

Run `cargo build --offline --workspace --all-targets`, then
`target/debug/examples/build_curves` from the repository root. On Windows the
executable has the `.exe` suffix. An optional argument sets the output folder.
The client uses a real stdio MCP session. It records the calls in
`mcp-transcript.jsonl` and writes the editable project to `faber.json`.

- Nose: a circular ogive profile, revolved through 360 degrees, with a 2 mm
  inward shell and an open base. The outer radius is 20 mm and height is 60 mm.
  The base face is selected by its centre.
- Ring: a revolved 30 to 40 mm radial section, 10 mm high. Four blind pockets
  have a 2 mm radius and 5 mm depth. The material volume is 6920 pi mm3.
- Wing: a ruled loft between two symmetric Bezier profiles. The root chord is
  100 mm and the tip chord is 60 mm. The tip has a 10 mm offset and -5 degree
  twist. Parameter `span` sets the distance between sections, initially 150 mm.
  This shape has no aerodynamic validation.

The output folder contains STEP and STL solids, an exterior image and a section
image. The section does not change the exported solids. These examples exercise
curved modelling; they do not specify a complete aircraft or gimbal assembly.

The stdio test checks the ring volume, shell volume range, wing volume scaling,
STEP output and section caps in both directions. It uses no network or window.
