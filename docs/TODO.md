# Deferred work

Updated 2026-11-18.

The viewer and MCP use the same assembly motion definitions.

## Purchased parts and assemblies

- [x] Add records for downloaded servos, motors, bearings, wheels and electronic
  boards, with source, licence, content hash and available mass data.
- [x] Preserve purchased model files and prevent geometry edits to those parts.
- [ ] Add reusable part instances and assembly placements.
- [ ] Save and recall named poses.
- [x] Add revolute and slider joints with viewport drag controls.
- [x] Link joint motion by ratio for gears and drive connected linkages.
- [x] In the viewer, drag a hinged control surface. Check that the servo arm
  and connecting rod follow it, the rod length stays fixed and the assembly
  stops at its motion limits. Use the same joint definitions through MCP.
- [x] Check assembly clearance and interference across poses.
- [x] Check interference along a continuous joint motion.
- [x] Read native Fusion `.f3d` archives. Restore supported sketches,
  parameters and ordered features. Report unsupported features. Test that
  an imported dimension can change and its dependent features rebuild.
- [ ] Verify an archive round trip in a running Fusion host.
- [ ] Test with real downloaded model fixtures, without network access in tests.

Owned-model import and general part placement already exist. Keep these tools
available for modelling. The hexagonal bin remains a plain open-top bin, without
motors, electronics, wheels or a robot base.
