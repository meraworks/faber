# Render and animation: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make pictures that show the design as a product: material
appearance, a studio look with a ground shadow, a slow high-quality render,
named views, exploded views, turntables and motion animations.

**Architecture:** The real-time renderer of `faber-render` gets a physically
based surface model with the roughness and metallic values of the material
library. A CPU path tracer in a new crate `faber-trace` makes the
high-quality picture from the same scene. Named views are project data.
Animations are frame lists that the present render path draws; the file is an
animated PNG.

**Tech Stack:** Rust 2024, wgpu, `rayon`, `png` (animated PNG),
`faber-geometry::bvh`.

**Spec:** `docs/specs/fusion-parity/00-roadmap.md`. Fusion scope covered:
appearance, scene settings, in-canvas and final render, turntable, exploded
view and its animation, named views. Not covered: decals, textures, depth of
field, cloud render.

## Global Constraints

- All shared rules of `00-roadmap.md` apply. Needs chunk 01 (materials, ray
  queries).
- Task 2 changes `faber-model`.
- A render with the same input gives the same image bytes. The path tracer
  uses a fixed seed and a pixel order that does not depend on the thread count.
- The default `render` style stays `technical`. No existing image test changes.
- An exploded view never changes a placement. It is a view state.

## Review Focus

- A part with no material uses its colour with roughness 0.6 and metallic 0
  (Task 1: `part_without_material_uses_its_colour`).
- A path-traced render stops at its time limit and returns the picture it has,
  with the sample count in the reply (Task 3: `time_limit_returns_a_picture`).
- An exploded view of an assembly with one part is the normal view (Task 4:
  `single_part_does_not_move`).
- A view that names a part that no longer exists still renders and reports the
  name (Task 2: `view_with_a_removed_part_reports_it`).
- An animation with one frame is refused (Task 5: `one_frame_is_refused`).

---

### Task 1: Studio style

**Files:**
- Modify: `crates/faber-render/src/shader.wgsl`, `crates/faber-render/src/lib.rs`, `crates/faber-scene/src/lib.rs` (surface values for each body), `crates/faber-wiring/src/lib.rs`, `src/server.rs` (`render` gains `style`, `ground`, `background`)
- Test: `crates/faber-render` tests

**Interfaces:**

```rust
pub struct Surface { pub base: [f32; 3], pub roughness: f32, pub metallic: f32 }
pub enum Style { Technical, Studio }
pub struct StudioOptions { pub ground: bool, pub background: [u8; 3], pub exposure: f32 }
```

- Studio: a microfacet surface model (GGX), one key light, one fill light, a rim light, a sky gradient for reflections, a ground plane at the lowest Z of the scene with a soft shadow, ambient occlusion from a vertex bake (`faber-geometry::bvh`, 32 rays for each vertex, kept with the unplaced body).

- [ ] **Step 1: Write failing tests.**
  - `studio_differs_from_technical`.
  - `sphere_has_a_highlight`: the brightest pixel of a studio sphere is above 0.9 and at most 10 % of the pixels are above 0.8.
  - `metal_and_plastic_differ`: the same sphere with `metallic` 1 and 0 gives a mean difference above 5 %.
  - `inside_corner_is_darker`: the pixels of an inside corner are at least 10 % darker than the open face next to it.
  - `ground_shadow_is_below_the_part`.
  - `part_without_material_uses_its_colour`.
  - `technical_render_is_unchanged` (compare with a stored image of the present renderer).
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-render -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 2: Named views

**Files:**
- Modify: `crates/faber-model/src/lib.rs` (`NamedView`, `ProjectData.views`, `Edit::SetView`, `Edit::RemoveView`), `crates/faber-files/src/layout.rs` (`analysis.json` gains `views`), `crates/faber-wiring/src/lib.rs`, `src/server.rs`, `crates/faber-viewer/src/app.rs`
- Test: `crates/faber-wiring/tests/views.rs`, `tests/mcp.rs`

**Interfaces:**

```rust
pub struct NamedView { pub camera: Camera /* eye, target, up, fov_deg or ortho height */, pub hidden: Vec<String>, pub section: Option<String>,
    pub explode: Option<f64> /* 0..=1 */, pub pose: Option<String> /* motion pose */, pub style: Option<String>, pub note: String }
```

- `render {view: <name>}` uses the view. The viewer has a "Views" list with "Save view" and "Apply".

- [ ] **Step 1: Write failing tests:** `view_round_trips`, `render_with_a_view_hides_its_parts`, `view_with_a_removed_part_reports_it` (`View iso: part lid was not found.` in `warnings`), `project_without_views_is_byte_identical`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test views -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 3: Path tracer

**Files:**
- Create: `crates/faber-trace/Cargo.toml`, `src/lib.rs`, `src/camera.rs`, `src/material.rs`, `src/integrate.rs`
- Modify: `crates/faber-wiring/src/lib.rs`, `src/server.rs` (`render` gains `quality: "high"`, `samples`, `seconds`)
- Test: `crates/faber-trace/tests/trace.rs`

**Interfaces:**

```rust
pub struct TraceScene { pub bodies: Vec<TraceBody> /* mesh, Surface */, pub camera: Camera, pub sky: Sky /* gradient, sun direction and size */, pub ground: Option<f64> }
pub struct TraceOptions { pub width: u32, pub height: u32, pub samples: u32 /* default 64 */, pub max_bounces: u32 /* 6 */, pub seconds: Option<f64>, pub seed: u64 }
pub struct Traced { pub pixels: Vec<[f32; 3]> /* linear */, pub samples_done: u32, pub seconds: f64 }
pub fn trace(scene: &TraceScene, options: &TraceOptions) -> Result<Traced>;
pub fn tone_map(pixels: &[[f32; 3]], exposure: f32) -> Vec<[u8; 3]>;
```

- Sampling: one sample pass over the whole picture at a time, so a time limit gives a complete picture. Each pixel has its own random stream from the seed and the pixel index.
- Surface: a diffuse and a GGX specular lobe with the same `Surface` values as Task 1. Next-event estimation for the sun.

- [ ] **Step 1: Write failing tests.**
  - `furnace`: a closed white sky of radiance 1 and a diffuse sphere of albedo 0.5 and no sun: the mean pixel on the sphere is 0.5 within 3 %.
  - `shadow_under_a_box`: the ground below a box is darker than the open ground by at least 30 %.
  - `same_seed_same_bytes`: two runs with 1 and with 4 threads give equal pixels.
  - `more_samples_less_noise`: the variance between two seeds falls by at least 40 % from 8 to 32 samples.
  - `time_limit_returns_a_picture`: `seconds: 0.5` returns `samples_done >= 1` and no pixel is NaN.
  - `mirror_shows_the_sky_gradient`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 --release -p faber-trace -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass. Record the time of a 1280 x 720 picture of `examples/trash-bin` with 64 samples.

### Task 4: Exploded view

**Files:**
- Create: `crates/faber-core/src/explode.rs`
- Modify: `crates/faber-wiring/src/lib.rs`, `src/server.rs`, `crates/faber-viewer/src/app.rs`
- Test: `crates/faber-wiring/tests/explode.rs`

**Interfaces:**

```rust
pub struct Offset { pub part: String, pub vector: [f64; 3] }
pub fn explode(bodies: &[Body], contacts: &[Contact], motion: &MotionStudy, factor: f64) -> Vec<Offset>;
```

- Rule: the part with the largest volume stays. Each other part moves along the direction from the centre of the fixed part to its own centre, snapped to the nearest principal axis when the angle to it is below 30 degrees. A part of a joint moves along the joint axis. The distance is `factor` times the distance that separates its bounds from the bounds of the parts between it and the fixed part, plus 20 % of its own size.
- `render {explode: 0.7}` and a named view `explode` value use it. The viewer has an "Explode" slider.

- [ ] **Step 1: Write failing tests:** `factor_zero_is_the_assembly`, `factor_one_separates_all_bounds` (no two bounds overlap on `examples/assembly-motion`), `stacked_plates_move_along_z`, `screw_moves_along_its_axis`, `single_part_does_not_move`, `placements_on_disk_are_unchanged`.
- [ ] **Step 2: Run** `cargo test --offline -j 4 -p faber-wiring --test explode -- --test-threads=1`. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 5: Animations

**Files:**
- Create: `crates/faber-wiring/src/animate.rs`, `crates/faber-files/src/apng.rs`
- Modify: `src/server.rs`, `src/main.rs` (`faber animate PROJECT KIND FILE`), `crates/faber-viewer/src/app.rs` ("Export animation")
- Test: `crates/faber-files/tests/apng.rs`, `crates/faber-wiring/tests/animate.rs`

**Interfaces:**

```rust
pub enum Animation { Turntable { turns: f64 }, Explode { from: f64, to: f64 }, Motion { joint: String, from: f64, to: f64 },
                     Views { names: Vec<String> } /* camera moves between named views */ }
pub struct AnimationOptions { pub frames: u32 /* 2..=600 */, pub fps: u32, pub width: u32, pub height: u32, pub style: Style, pub view: Option<String> }
pub fn animate(project: &Project, animation: &Animation, options: &AnimationOptions, path: &Path) -> Result<AnimationReport>;
```

- The file is an animated PNG (`.png` or `.apng`). With `frames_dir` the frames are also written as numbered PNG files.
- MCP: `animate {kind, ..., path}`. The reply has the path, the frame count and the duration. No image is sent inline.

- [ ] **Step 1: Write failing tests.**
  - `apng_has_the_frame_count`: the `acTL` chunk gives 12 frames; the file opens as a normal PNG with the first frame.
  - `turntable_first_and_middle_frames_differ` and `turntable_last_frame_meets_the_first` for a part with no rotation symmetry.
  - `explode_animation_starts_assembled`.
  - `motion_animation_respects_the_limits`: a range outside the joint limits is refused with the limits.
  - `one_frame_is_refused`: `An animation needs 2 frames or more.`
- [ ] **Step 2: Run** the named tests. Expected: fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: pass.

### Task 6: Guide and example

**Files:**
- Modify: `docs/modelling-guide.md` (section `## Pictures and animations`), `README.md` (one studio picture and one exploded animation of an example)

- [ ] **Step 1:** Render `examples/assembly-motion` in the studio style, as a path-traced picture, and as an exploded animation. Put the three files in `docs/media/`.
- [ ] **Step 2:** Write the guide section: styles, named views, `quality: "high"` with its time, and each animation kind with one example call.
