# Fusion reference method

Date: 2026-11-17

## Scope and evidence

Use Fusion as a reference for CAD behaviour. Implement Faber operations through
the existing Rust model, core, ports and adapters. Do not treat a visible command
or a passing internal test as proof of matching Fusion.

Autodesk documents a parametric timeline, parameter-driven rebuilds, feature
editing, dependency order and suppression. Its extrusion API also distinguishes
one-sided, symmetric and two-sided extents and reports feature health.
Sources: [modelling modes](https://help.autodesk.com/cloudhelp/ENU/Fusion-Designs/files/ASM-DESIGN-MODELING-MODES.htm),
[extrusion API](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/fusion_ExtrudeFeature.htm).

## Local static inspection

REA opened the installed Fusion360.exe without running the application.
The target is PE x86-64 with SHA-256
`2cbbbc14ab4dd4dd40c21ba85861317d2d6f532abbc1b71c37835e22e7090abc`.
Ghidra 12.1.4 reported 2512 procedures and 1847 defined strings. These counts
describe its recovered inventory, not complete application behaviour.

A focused search found Qt6Widgets.dll, Qt6Gui.dll, Qt6Core.dll, NuCommands10.dll
and NuBase10.dll references. The section matches were Qt table-header methods;
they are not evidence for CAD section analysis. No runtime model was observed.
The session was closed after exporting `.cargo/fusion-rea-evidence.json`.
This inspection does not establish the implementation of Fusion's geometry engine.

## Proposed comparison cases

These are Faber acceptance cases. Fusion output remains unmeasured until a
reference run records the same inputs, version and units.

| Case | Required observation | Current evidence |
| --- | --- | --- |
| Rectangle sketch, 20 by 30, extrude 40 mm | One solid, volume 24000 mm3, bounds 20 by 30 by 40 | Related Faber sketch tests; exact comparison fixture pending |
| Change extrusion to 60 mm | Volume 36000 mm3; later features rebuild | Faber dependency tests; Fusion run pending |
| Undo, redo, save, reopen | Values, feature order and geometry persist | Faber core, stdio and GUI tests; Fusion run pending |
| Six-sided open bin | Constant wall, closed floor, editable dimensions | Faber MCP example; Fusion run pending |
| Section through bin centre | Closed cut regions and visible cavity; solid export unchanged | Faber section tests; matching camera capture pending |
| Move a feature before its source | Refusal with useful dependency error | Faber history tests and Autodesk documentation |
| Invalid radius or thickness | Error without loss of the last valid design | Faber tests; comparative failure behaviour pending |

For each case, record input, units, operation order, expected topology, volume,
bounds, error behaviour, save/reopen result and elapsed time. Compare geometry
with explicit tolerances. A screenshot alone does not prove geometry correctness.

## Work order

1. Complete typed feature controls and direct sketch creation in the GUI.
2. Add the comparison fixtures as failing tests before each implementation.
3. Run the same operations through GUI and stdio MCP.
4. Check save/reopen with a fresh process and without an existing shape cache.
5. Record reference output separately from inferred or analytical expectations.
6. Extend to constraints, face references, assemblies and the remaining main plan.

No Fusion window was opened during this investigation. Live reference captures
must respect the user's request to keep application windows off the screen.
