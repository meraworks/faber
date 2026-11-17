# Deferred work

Updated: 2026-11-17.

The owner deferred purchased parts and assembly support. Current work focuses
on an AI-first CAD application: fast model creation and edits through stdio
MCP, with a shared GUI. Keep these tasks for a later stage. Filament selection,
slicer settings and printer operation are outside the current task.

## Purchased parts and assemblies

- [ ] Add records for downloaded servos, motors, bearings, wheels and electronic
  boards, with source, licence, content hash and available mass data.
- [ ] Preserve purchased model files and prevent geometry edits to those parts.
- [ ] Add reusable part instances and assembly placements.
- [ ] Add joints, motion limits and named poses.
- [ ] Check assembly clearance and interference across poses.
- [ ] Test with real downloaded model fixtures, without network access in tests.

Owned-model import and general part placement already exist. Keep these tools
available for modelling. The hexagonal bin remains a plain open-top bin, without
motors, electronics, wheels or a robot base.
