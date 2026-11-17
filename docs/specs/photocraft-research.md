# PhotoCraft method: findings for Faber

Reviewed 2026-11-17 at commit `01b79958a7b73c3d840b76492069e9635f9f7394`.
This review reads public documentation and selected test source. No PhotoCraft
code or tests were run. Its contributor rules are evidence, not Faber rules.

## What the project claims

PhotoCraft describes itself as an independent Rust reimplementation of
Photoshop. Its stated method uses public specifications, papers and observed
behaviour. It forbids copying proprietary code, shaders and assets. This is
different from a source port, which translates or adapts an existing
implementation. These documents establish the project's stated method. They do
not prove the origin of every implementation or show a separate research team
and implementation team. [README][readme], [contributor rules][contributing],
[architecture][architecture].

## How it studies the target

The parity assessment identifies a target version and inspection date. It
reports inspection of installed application metadata, format plug-in flags,
shortcut files, language packs, presets and binary strings. It also uses public
documentation and user reports. Its menu list was written from observed menus;
the document admits that this list can omit new target features. Thus menu
coverage has a limited denominator. This is a feature inventory, not evidence
that Photoshop algorithms were decompiled and translated. [Parity method][parity].

A useful worked example is the Average filter. The authors report two small
experiments in Photoshop 26.6.1 on Windows. Each uses a 16 by 16 image with red
and blue halves and different alpha values. The observed result keeps each
pixel's alpha and averages colour without alpha weighting. The resulting tests
check the command, selection, history and undo at several storage depths. The
document limits its external evidence to RGB8; the other depth tests check
internal rules. [Average filter evidence][average].

The same example tests imported smart filters after clearing the saved layer
and filter caches. This is essential: saved Photoshop pixels can look correct
even when the new program cannot execute the filter. [Average filter evidence][average].

## Architecture that supports this method

The documented design places data and algorithms below the engine. A shared
command registry serves the UI, CLI, control channel and MCP server. Commands
have parameters, an enable condition, history behaviour and tests. The egui
shell is a client of that engine. A CPU renderer supplies the reference for GPU
checks. [Architecture][architecture], [command checklist][contributing].

The layer checker is present in source. It classifies workspace crates and
restricts UI dependencies to the upper layers. This is stronger evidence than
an architecture diagram alone, although this review did not run the checker.
[Layer checker source][layers].

## What its tests prove, and what they do not prove

The corpus test source defines two different checks:

| Check | Comparison | Limit |
|---|---|---|
| External render | PhotoCraft render against the saved Photoshop composite, or a thumbnail when needed | Some checks use cached type or smart-object pixels; thumbnail checks have separate tolerances |
| Internal round trip | PhotoCraft render before and after its own PSD export and import | Does not prove that Photoshop accepts the file or reproduces its editable behaviour |

The source contains separate minimum pass counts. For the 258 Photoshop
fixtures, the external render floor is 190 and the internal round-trip floor is
258. These are committed thresholds, not a fresh run from this review. The
corpora use pinned revisions and checksums. Missing fixtures fail when the
corpus tests are enabled. Ordinary tests omit this optional suite.
[Corpus test source][corpus], [agent guide][agents].

The generated scorecard reports 664 of 664 listed menu items connected to
commands. It also records partial features, unused settings and unmeasured
performance cases. Its performance baseline is explicitly marked as affected
by severe machine load. Do not treat those timings as a clean comparison.
[Scorecard][scorecard].

The roadmap reports about 76% feature breadth and about 45% readiness. The
second figure is an estimate with chosen weights. These October assessments
have not been fully measured again, and their counts differ from the current
scorecard. The parity document admits that an earlier estimate was too
optimistic because features counted as present still had many defects. Neither
the percentages nor its effort estimates transfer to Faber. [Roadmap][roadmap],
[parity assessment][parity].

## Development time and forum evidence (2026-11-17)

The project's effort assessment reports 1,152 commits on `main` and 1,004
merged pull requests from the first commit on 2026-09-30 to 2026-10-10: about
10.5 days. It estimates 2,000–4,000 agent-hours from that activity and reports
10–20 agents working at the same time. The hours are an estimate, not a time
log. This review checked the assessment, but did not recount the Git history.
[Effort assessment][effort].

That interval produced an alpha application. The 2026-10-10 assessment put
readiness for professional work at about 45% and estimated another
2,300–4,100 agent-hours for full parity. These are dated estimates, not a
current measurement of completion. The 10.5 days therefore describe initial
development, not the time needed to match Photoshop in full.
[Effort assessment][effort].

On Hacker News, the developer says he stopped using Tauri and moved to 100%
egui, with links to PhotoCraft and the other applications. This supports the
reported UI choice; it does not establish product quality or development time.
[Developer comment][hn].

A Reddit post repeats the commit counts, estimated hours and agent count. It
attributes them to the project's own assessment and says the hours were not
measured token use. Thus it is forum discussion of the same evidence, not an
independent measurement. [Reddit discussion][reddit].

## Application to Faber

These are proposed lessons from the evidence, not changes to Faber's plan:

1. Define a small target workflow and its exact Fusion version. Record inputs,
   operations, parameters, output and known limits for each observation.
2. Write a behaviour specification from those observations and public sources.
   Keep the evidence separate from the implementation.
3. Keep the shared command path for UI, CLI and MCP. Test geometry and history
   through that path before adding UI controls.
4. Use small synthetic CAD cases with known dimensions. Compare volume, area,
   bounds, validity, tolerances and feature results. A screenshot is useful for
   UI checks but cannot prove a solid is correct.
5. Rebuild from the feature history after removing saved display meshes and
   other caches. A correct imported display does not prove editable features.
6. Separate command coverage, geometry correctness, file exchange, editability,
   UI behaviour and performance. Keep a list of unsupported cases.
7. Prepare reference files before tests. Pin and hash them. Keep Faber tests
   offline. Run performance checks without other builds.

The strongest lesson is the evidence loop: observe a narrow case, state the
expected behaviour, implement it independently, then retain a regression test.
PhotoCraft offers a concrete example of this loop. It does not establish that a
broad professional application can be replaced by copying its menu list.

### Suggested first CAD cases

This matrix is an inference for Faber. It is not a report of tests in Fusion or
a claim of matching behaviour.

| Case | Evidence to retain |
|---|---|
| Constrain a rectangle and extrude it | Dimensions, constraint state, volume, bounds and solid validity |
| Change a sketch dimension | Rebuilt geometry and the effect on later features |
| Undo and redo the change | Model state before and after each command |
| Save, close and reopen | Feature data, references and a rebuild without saved meshes |
| Inspect a section | Section position, contour geometry and an offscreen image |

[readme]: https://github.com/storytold/photocraft/blob/01b79958a7b73c3d840b76492069e9635f9f7394/README.md
[agents]: https://github.com/storytold/photocraft/blob/01b79958a7b73c3d840b76492069e9635f9f7394/AGENTS.md
[roadmap]: https://github.com/storytold/photocraft/blob/01b79958a7b73c3d840b76492069e9635f9f7394/ROADMAP.md
[contributing]: https://github.com/storytold/photocraft/blob/01b79958a7b73c3d840b76492069e9635f9f7394/docs/contributing.md
[architecture]: https://github.com/storytold/photocraft/blob/01b79958a7b73c3d840b76492069e9635f9f7394/docs/architecture.md
[parity]: https://github.com/storytold/photocraft/blob/01b79958a7b73c3d840b76492069e9635f9f7394/docs/target-app-parity.md
[average]: https://github.com/storytold/photocraft/blob/01b79958a7b73c3d840b76492069e9635f9f7394/docs/average-filter.md
[scorecard]: https://github.com/storytold/photocraft/blob/01b79958a7b73c3d840b76492069e9635f9f7394/docs/scorecard.md
[layers]: https://github.com/storytold/photocraft/blob/01b79958a7b73c3d840b76492069e9635f9f7394/xtask/src/layers.rs
[corpus]: https://github.com/storytold/photocraft/blob/01b79958a7b73c3d840b76492069e9635f9f7394/crates/io/tests/it/corpus.rs
[effort]: https://github.com/storytold/photocraft/blob/01b79958a7b73c3d840b76492069e9635f9f7394/docs/target-app-parity.md#remaining-effort-and-calibration
[hn]: https://news.ycombinator.com/item?id=49922904
[reddit]: https://www.reddit.com/r/RackBrainsResearch/comments/1x53tx9/the_hidden_costs_of_artcraft_did_one_developer/
