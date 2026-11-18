# Model and slicer checks

The installed OrcaSlicer opened these STL files, saved 3MF projects and sliced
each project. All commands ran without a window. No printer connection or send
command was used. The checks use a temporary 400 by 400 by 400 mm virtual build
volume and default slicing settings. They do not change the user's printer or
material profiles and are not prepared print jobs.

| Model | Triangles | Layers |
| --- | ---: | ---: |
| Honeycomb bin | 24096 | 1600 |
| Ogive nose | 22492 | 299 |
| Pocketed ring | 3024 | 50 |
| Twisted wing | 10956 | 750 |
| Modelled thread | 3426 | 75 |
| Downloaded screw | 3173 | 50 |

The thread's initial swept tip produced an empty first layer. An intersection
through MCP trimmed both ends to flat faces. The corrected model sliced without
that error. The downloaded screw geometry was not changed.

The slicer reported zero repaired edges, removed or reversed facets, degenerate
facets and backwards edges for the bin, nose, ring, wing and initial thread.
A separate rounded-coordinate edge count found two unpaired-count edges on the
nose and one on the downloaded screw. This check rounds to 0.00001 mm and can
merge very small edges; it does not establish a hole or a self-intersection.
Do not treat successful slicing as a complete geometric validity proof.

Local records: `.cargo/slicer-check/results.json`, `.cargo/check-slicer.py` and
the sliced 3MF files in `.cargo/slicer-check`. The original model outputs stay
in their example directories. The bin remains full size; no scale change was
made to fit a particular printer.

CLI reference: [OrcaSlicer actions](https://github.com/OrcaSlicer/OrcaSlicer/wiki/cli_actions).
