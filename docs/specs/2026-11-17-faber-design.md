# Faber design

Status: draft, 2026-11-17.

## 1. Intent

Faber is a parametric 3D modelling program in Rust. Its primary user is a
coding agent (Claude Code, Codex or any MCP client). A person gives the
direction, watches and writes comments. Faber must work when no window is
open. When the viewer is open, it must show each change of the agent at the
time that it occurs.

The tools that exist now do one of these:

- They are made for a mouse. An agent must drive them through a script
  bridge, with coordinates and camera commands (Fusion, SolidWorks).
- They are code libraries. The agent writes a program, runs it, and does not
  see the result (build123d, CadQuery, OpenSCAD).
- They put a thin MCP wrapper around a GUI program. The window must be open,
  and a reply does not tell the agent what changed.

None of them gives the agent a clean model, tools with results that the
agent can verify, and a fast way to see what it did. The same problem was
solved for PCB layout in Lamina. Faber uses the same method for solids.

## 2. Goals, non-goals, anti-goals

Goals

- An agent can take a part from an idea to files that are ready for a 3D
  printer or a machine shop, with only MCP tools. It can verify each step.
- Curved shapes are fast to make: revolved bodies, lofts between sections,
  sweeps along a path, fillets, shells.
- Each change gives a report and a picture in the same reply. The agent does
  not ask for a screenshot after each step.
- The agent refers to geometry by meaning (a face that a feature made, the
  edge between two faces), not by coordinates.
- The project is plain files in git. Diffs are easy to read.
- Purchased parts are real downloaded models. Faber imports them and never
  changes them.
- Assemblies have joints and poses. Faber finds interference in each pose
  with its own engine.
- Checks run after each change: valid solids, print limits, clearances,
  interference, mass and centre of mass.
- A native viewer shows the live model. The viewer can run or not run, and
  this has no effect on the agent.
- Fast and light: a change to a part of 50 features gives its reply in less
  than 1 s in the usual case, and the PC does not freeze.

Non-goals (not in version one)

- Edits by a person in the viewer. In version one, the person watches,
  selects and writes comments.
- A geometric constraint solver for sketches. The agent gives coordinates
  and expressions.
- Free-form surface modelling with control points (class A surfaces).
- Simulation (FEA, CFD, kinematics with forces).
- CAM toolpaths and slicer functions. Faber exports files for those tools.
- 2D drawings with full dimension standards. Simple views come late.
- More than one agent on one project at the same time.

Anti-goals (if Faber has one of these, it is a failure, also if it works)

- A window or a daemon that must be open before the agent can work.
- A new language. The agent uses JSON and tool calls.
- A reply that says "ok" and nothing more.
- A broken model on disk. If a change cannot be built, Faber refuses it and
  the files do not change.
- A purchased part that Faber or the agent modelled or changed.
- Names of other CAD products in the core code. The core knows ports.

## 3. Constraints

- Rust for all of the code that we write, in one Cargo workspace. The one
  exception is a thin C++ file set that connects the geometry kernel (5.14).
- MCP over stdio. The client starts one process for each session.
- Windows 11 first (the development computer), then macOS and Linux.
- Units: millimetres (f64) and degrees. Z is up. The axes are right-handed.
- All text is ASD-STE100 Simplified Technical English: docs, comments, tool
  descriptions, errors, commit messages.
- No AI attribution in the repository (commits, docs, code).
- Keep the system load low: at most 6 build jobs, and never two builds at
  the same time.
- Tests do not use the network.
- Licence of our code: MIT OR Apache-2.0. The kernel has its own licence
  (5.14).

## 4. Approaches considered

### 4.1 Geometry kernel

1. Our own B-rep kernel in Rust. All of the code is ours. But booleans and
   fillets that do not fail are the hardest part of CAD. We get no real
   part for months. Rejected for version one.
2. A Rust program with the Open CASCADE kernel (OCCT) behind a port.
   OCCT does lofts, sweeps, fillets, shells and STEP today. build123d uses
   the same kernel, so it can make all parts of our earlier projects.
   Chosen.
3. An implicit (signed distance) kernel in Rust. Booleans and fillets never
   fail. But it gives no exact faces and edges, no real STEP output, and
   lofts are difficult. Rejected.

Existing Rust B-rep kernels are not an option. On 2026-11-03 we tested
them with real models. None of them loaded the models correctly.

The long-term aim of the owner is a kernel in Rust. The port of 5.14 makes
this possible one operation at a time. The function list of the adapter is
the list of work for that project.

### 4.2 Process model

The same decision as in Lamina, for the same reasons: a core library, a
headless engine with MCP over stdio, and a separate viewer process that
watches the project files. The agent and the viewer share only the files on
disk. A daemon and a GUI with a built-in server were rejected.

### 4.3 Source of a part

1. A script in a programming language (as build123d). Agents write code
   well. But the model is a program that you must run to read it, and a
   tool cannot change one step safely.
2. A list of features in JSON, with parameters and expressions. Each tool
   adds or changes one feature. A person can read the diff. Chosen.
3. Direct edits of a B-rep with no history. A change of a dimension is not
   possible later. Rejected.

## 5. Decisions

### 5.1 Workspace layout

Hexagonal, as in Lamina: domain crates, ports, core, adapters, a
composition root, and two binaries.

| Crate | Job |
|---|---|
| `faber-model` | Data types of the project files, JSON schemas, errors. No geometry code. |
| `faber-expr` | Expressions for parameters: parse and calculate. |
| `faber-ports` | Port traits (`Kernel`, `Renderer`, `Exchange`, `Standards`) and the `Ports` bundle. Shared types `Shape`, `Mesh`, `Topology`. |
| `faber-mesh` | Triangle meshes, BVH, ray and distance queries. Pure Rust. |
| `faber-clash` | Interference and clearance between placed meshes, for one pose or many. |
| `faber-check` | The check registry and the checks. |
| `faber-scene` | Camera, view presets, draw lists. The renderer and the viewer share it. |
| `faber-core` | Project operations: one operation is one MCP tool and one CLI command. Lock, sync, build, commit, journal. |
| `faber-kernel-occt` | Adapter: the `Kernel` port on OCCT. The only crate with C++. |
| `faber-render` | Adapter: pictures (PNG) of a scene, with labels and id maps. |
| `faber-exchange` | Adapter: STL, 3MF, glTF and OBJ in Rust; STEP goes through the kernel port. |
| `faber-std` | Adapter: standard parts and tables (screws, nuts, inserts, hole sizes). |
| `faber-wiring` | Composition root. The only crate that names adapters. |
| `faber-mcp` | Binary `faber`: the MCP server and the same operations as CLI commands. |
| `faber-viewer` | Binary `faber-viewer`: the live window. |

`faber-core` depends on the domain crates and `faber-ports` only. It never
names a kernel, a file format or a vendor.

### 5.2 Project on disk

A project is a folder in git. One project holds many parts and assemblies.

```
faber.json          name, shared parameters, materials, print profile, rules
parts/<name>.json   one part: parameters and the feature list
assemblies/<name>.json  instances, joints, poses, contacts
vendor/<id>.json    record of a purchased part: source, licence, hash, mass
vendor/<id>.step    the model as downloaded (STEP, or STL/OBJ if no STEP exists)
notes.json          comments of the person, attached to geometry
out/                exported files
cache/              shapes, meshes, pictures, journal (not in git)
```

Rules for the files:

- Each file has a `schema` integer. `faber schema` prints the JSON schema.
- Keyed collections are sorted maps, so that diffs stay stable.
- Faber writes each file atomically (temporary file, then rename).
- A `.faber.lock` file prevents a second engine on the same project. The
  viewer never takes the lock.
- A file in `vendor/` is read-only for all tools.

### 5.3 The part recipe

A part is a list of parameters and an ordered list of features.

- **Parameters.** A name, an expression, an optional note and optional
  limits. A part can use the shared parameters of `faber.json`. An
  expression has numbers, names, `+ - * / ^`, brackets and functions
  (`sin`, `cos`, `tan`, `sqrt`, `abs`, `min`, `max`, `round`, `if`). A
  length has no unit suffix (millimetres). An angle is in degrees.
- **Features.** Each feature has an `id` (a name that the agent gives or
  Faber makes), an `op` and the fields of that operation. Each number field
  accepts an expression.
- **Sketches.** A sketch is a plane and a list of 2D items: polyline, path
  (lines, arcs, splines), circle, rectangle, slot, regular polygon, ellipse,
  and arrays of items. Each item can have an `id`. There are no constraints.
  The plane is a base plane with an offset, a named reference plane, or a
  flat face.
- **Bodies.** A feature makes a new body, or adds to, cuts from or
  intersects a body. A part can hold more than one body.

Operations of version one:

| Group | Operations |
|---|---|
| Sketch and curves | `sketch`, `curve` (3D polyline, spline, helix) |
| Solids from sketches | `extrude`, `revolve`, `loft`, `sweep` |
| Primitives | `box`, `cylinder`, `sphere`, `cone`, `torus` |
| Dress-up | `fillet`, `chamfer`, `shell`, `draft`, `offset`, `thicken` |
| Holes | `hole` (plain, counterbore, countersink, for a screw size, for a heat-set insert) |
| Copies | `pattern` (linear, circular), `mirror` |
| Bodies | `boolean`, `move`, `split`, `import` (a STEP body of our own) |
| References | `plane`, `axis`, `point` |

### 5.4 References to geometry

A feature must point at faces and edges of earlier features. Faber stores a
selector, never a number of a face.

- **History.** Each face knows the feature that made it, its role (start
  cap, end cap, side, fillet, hole wall) and the sketch item that it came
  from. The kernel port gives this history for each operation (5.14).
- **Selector.** A JSON filter that Faber calculates again at each build:
  `created_by`, `role`, `from`, surface or curve type, direction of the
  normal, radius, position, `between` (the edge between two face sets),
  `convex` or `concave`, `near` a point, then `sort` and `take`. A selector
  can state the number of items that it expects (`expect`).
- **Ids for one build.** A query or a picture with labels gives short ids
  (`f12`, `e40`). They are valid until the next change. When the agent
  gives such an id to a tool, Faber writes a selector for that item into
  the feature and shows it in the reply.
- If a selector finds nothing, or not the expected number, the change is
  refused with the reason.

### 5.5 Build, cache and transactions

- **Build.** Faber calculates the features in sequence. Each result has a
  key (a hash of the feature, its values and the keys before it). A change
  to feature N builds again only N and the features after it. Shapes and
  meshes are also in `cache/`, so that a new process starts fast.
- **Transaction.** One tool call is one transaction: read the files again
  if they changed on disk, apply the change in memory, build, check, then
  write. If the build fails, nothing is written.
- **Strict.** By default, a change that adds a check violation is applied
  and reported. With `strict: true`, Faber refuses it. Malformed input and
  a failed build are always refused.
- **Undo.** Each transaction writes a journal record. `undo` and `redo`
  restore the files. Faber refuses an undo if a file changed after that
  record. `checkpoint` and `restore` keep named states.
- **Batch.** `batch` runs a list of tool calls as one transaction: all or
  nothing, one build, one report.
- **Edits by hand.** The files are the source. An agent or a person can
  edit them as text. The next call builds them and reports errors with the
  file, the feature and the reason.
- **Failures with a hint.** When a fillet, a chamfer or a shell fails, Faber
  tries smaller values (at most 6 kernel calls) and reports the largest
  value that works.

### 5.6 MCP tool surface

The tools are small and strictly typed. This is the plan of names. The
builder can change a name when a better one shows up in use.

- project: `open`, `new_project`, `status`, `schema`, `guide`, `undo`,
  `redo`, `checkpoint`, `restore`, `batch`
- parameters: `params`, `set_param`, `check_params`
- parts: `parts`, `new_part`, `copy_part`, `rename_part`, `delete_part`,
  `part_info`
- features: one tool for each operation of 5.3, and `edit_feature`,
  `delete_feature`, `move_feature`, `suppress_feature`
- queries: `find`, `describe`, `measure`, `mass`, `pick`
- views: `render`, `save_view`
- checks: `check`, `checks`, `set_rules`, `exclude_violation`,
  `include_violation`, `review`
- purchased and standard parts: `import_model`, `vendor`, `standard_part`,
  `standards`, `hole_for`
- assemblies: `new_assembly`, `add_instance`, `place`, `mate`, `joint`,
  `pose`, `remove_instance`, `assembly_info`, `bom`, `contacts`,
  `interference`, `clearance`
- output: `orient`, `plate`, `export`, `formats`
- notes: `notes`, `add_note`, `resolve_notes`

Rules for all tools:

- Validate the input before you change the project.
- Return what changed, the numbers of the result, and each rule that is now
  violated.
- Never succeed without a report.
- A domain error is a tool error with a clear text, not a protocol error.

### 5.7 What a reply contains

Each tool that changes geometry returns one result with these parts:

- `summary`: one sentence.
- `changed`: the files and the features that changed.
- `geometry`: for each body that changed: volume before and after, the
  bounding box, the number of faces and edges, the new faces with their ids.
- `stored`: the selectors that Faber wrote from ids.
- `new_violations`, `fixed`, `errors`, `warnings`: as in Lamina.
- `picture`: a small picture (640 x 480) that looks at the changed region,
  with the changed faces in a highlight colour. `picture: "none"` switches
  it off. `picture: "labels"` adds the ids.
- `time`: the build time in milliseconds.

### 5.8 Rendering

One renderer makes pictures for the agent and for the viewer.

- `render` takes a target (project, assembly, part, body, feature,
  selector, point or box), a view (`iso`, `front`, `back`, `left`, `right`,
  `top`, `bottom`, or yaw and pitch), `zoom`, and options.
- Options: `section` (cut by a plane, with a filled cut face), `explode`,
  `hide`, `only`, `ghost` (transparent), `highlight`, `labels` (faces,
  edges, parts, features), `dims` (draws the last measurements), `sheet`
  (several views in one picture), `size`.
- A picture with labels comes with a legend in JSON: for each label, the
  id, the type, the size and the history of the item.
- The renderer keeps an id for each pixel. `pick` gives the face, the part
  and the 3D point at a pixel of an earlier picture.
- Surfaces come from the GPU, also with no window (a software adapter is
  the fallback). Edges, labels and dimensions are drawn on top in 2D, so
  that lines are sharp.
- Pictures go to `cache/render/` and into the reply as PNG.

### 5.9 Checks

A registry lists each check with an id, a family, a default severity, a
release severity, a summary and the reason. Limits are data, not code.

| Family | Checks of version one |
|---|---|
| Model | unknown parameter, expression error, unknown reference, selector with no result or the wrong count, feature that failed, duplicate id |
| Geometry | invalid shape, open shell, more solids than expected, edge or face that is too small |
| Print | larger than the bed, wall thinner than the limit, hole smaller than the limit, overhang area for the print direction, small first layer |
| Assembly | interference, clearance below the limit, interference during motion |
| Vendor | record with no source or licence, model file that changed (hash) |

- A print profile gives the print limits (bed, nozzle, layer). It is a JSON
  file with a source. Version one has a profile for the Bambu Lab A1.
- `export` refuses a part or an assembly with errors, unless `force` is set.
- The person can exclude one violation with a reason.
- `mass` gives volume, mass, centre of mass and inertia. The density comes
  from the material. A vendor record can carry the real mass.

### 5.10 Purchased parts and standard parts

- `import_model` copies a STEP file (or STL, OBJ) into `vendor/` and writes
  the record: source URL, licence, SHA-256, date, notes. The names, colours
  and the solids of the file stay as they are.
- A vendor part can be an instance in an assembly. No feature can change
  it. A part of ours can use its faces as references (for example, a pocket
  with an offset from the servo body).
- Standard parts (ISO screws, nuts, washers, heat-set inserts) come from
  `faber-std` as generated geometry with the standard and the size in the
  record. `hole_for` gives the hole sizes for a screw or an insert.

### 5.11 Assemblies, poses and interference

- An assembly has instances of parts, vendor parts and other assemblies.
  Each instance has a placement.
- `mate` calculates a placement from geometry (face on face, axis on axis)
  and stores the rule, so that the placement follows a parameter change.
- A joint (revolute, prismatic, fixed) connects two instances and has
  limits. A pose is a named set of joint values.
- `interference` examines one pose, a list of poses or a sweep of joint
  values. The engine is `faber-clash`: a BVH for each mesh, the boxes of
  one mesh moved into the frame of the other, exact triangle tests, then
  the kernel calculates the common volume for each pair that it reports.
- `contacts` declares pairs that touch or press on purpose. Those pairs
  give no violation.
- Target: 60 parts in 45 poses in less than 10 s.

### 5.12 Output

- STEP (parts and assemblies, with names and colours), STL, 3MF, glTF.
- `orient` sets the print direction of a part. It can propose a direction
  with the smallest overhang area.
- `plate` puts parts on print plates of the profile. `export` writes one
  3MF file for each plate.
- Each export records the project state (a hash) in the file name or in the
  metadata.

### 5.13 Viewer

- An egui and wgpu window. It watches the project folder and draws again
  after each change. The changed faces flash.
- Orbit, pan, zoom, section, explode, hide. A tree shows assemblies, parts
  and features. A panel shows parameters and violations.
- A click shows the id and the history of a face. The person can attach a
  note to it. The agent reads the notes with a tool.
- `faber-viewer shot` makes a picture of the window with no window on the
  screen, for tests of the interface.

### 5.14 Kernel port and adapter

- `faber-ports::Kernel` is the only door to geometry: make, change,
  combine, measure, tessellate, read and write shapes. A shape is an opaque
  handle. Each operation returns the new shape and its history: for each
  new face and edge, the input item that it came from and how (kept,
  modified, generated).
- The names of 5.4 are calculated in `faber-core` from that history. They
  do not depend on the kernel.
- `faber-kernel-occt` implements the port on OCCT through a small C
  interface that we write (plain functions, error codes, no C++ types in
  Rust). Each function catches all C++ exceptions.
- OCCT is LGPL-2.1 with the Open CASCADE exception. We link it and give
  its licence text. Our own code stays MIT OR Apache-2.0.
- Kernel calls run one at a time. A crash of the kernel must not lose the
  project: the files on disk are always a state that builds.
- A second adapter in Rust can replace operations one at a time later.

### 5.15 Order of work

1. Kernel adapter and port: shapes, primitives, booleans, mesh, STEP.
2. Model, expressions, build with cache, project files, `faber` CLI.
3. Renderer and MCP server. From this step, the agent can see and work.
4. Selectors, queries, dress-up features, patterns.
5. Curved shapes: revolve, loft, sweep, splines.
6. Purchased parts and assemblies.
7. Checks and the interference engine.
8. Output for print and for exchange.
9. Viewer.
10. Standard parts and tools that save time.
11. Release.

MCP wraps each core operation when it is available. From step 3, the agent
uses Faber to make the test parts of Faber.

### 5.16 First targets

Faber is its own project. The first targets come from real work:

- A desk clamp for a pegboard: a prismatic part with holes. Known result:
  50 866.2 mm3, 30 x 76 x 145.3 mm, 21 faces.
- A pegboard plate with 64 octagon holes and 49 round holes: sketch arrays.
  Known result: 60 900.5 mm3.
- An ogive nose with a shell, and a gimbal ring with pockets: revolve,
  shell, fillets.
- A wing section: a loft between two airfoils with taper and twist.
- A gimbal with a purchased servo model: assembly, joint, poses,
  interference.

## 6. Left to the builder

- The exact JSON schemas, in the file split above.
- Tool names and argument shapes, in the plan above.
- Mesh tolerances and the default picture sizes.
- The layout of labels in a picture.
- The search that writes a selector from an id.
- The test strategy. But each operation needs a test with a number that is
  known from a formula, and each tool needs a test through MCP.

## 7. Open

- Do we publish on crates.io or not?
- Do we ship OCCT as static libraries in the binary or as DLL files next to
  it? Phase 1 of the plan measures the two.
- Does the kernel run in a child process later, so that a kernel crash
  cannot stop the server? Decide after the first 100 real parts.
