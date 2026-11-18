# Import a Fusion archive

On Windows, use the local Fusion installation to read a single-body parametric design.

1. Open the **Scripts and Add-Ins** dialogue in Fusion. Select **Add-Ins** and add the [FaberArchive folder](../tools/FaberArchive). Start the add-in.
2. In Faber, select **Open project** and enter the archive path. Choose a new destination folder or accept the suggested sibling folder. The CLI uses `faber import DESIGN.f3d NEW_DIRECTORY`. MCP uses `import_archive` with absolute `path` and `destination` fields. The parent folder must exist.
3. Open the new project in Faber. The part note gives the source feature names and any dimension or constraint that uses its solved value.

The add-in opens a temporary document in the running Fusion application, reads the timeline, then closes the document without saving it. The CLI and add-in exchange files in the current user's local application data folder. The CLI does not start Fusion.

The reader accepts one solid in the root component. Each sketch must have a closed profile. Each profile becomes a separate sketch feature. Its outer loop and holes can contain lines and arcs or a complete circle. Each extrusion must use one profile. Extrusions must start on the sketch plane, have one distance and have no taper. The history can create the body, add material, cut or intersect. A feature outside these limits stops the import and identifies the timeline item.

Extrusion distances and driving circle radii and diameters become editable parameters. Direct references to named length parameters remain linked. Other dimension expressions use their solved values. Other sketch constraints use solved coordinates. Each sketch keeps its source plane position. Geometry outside the selected profile does not form part of the imported sketch. Assemblies, mesh bodies and direct designs require a different import path.

Faber builds the history before it writes the project. The rebuilt bounds must match within 0.01 mm. The volume must match within 0.1% or 0.00001 cubic millimetres, whichever is larger. An import failure leaves the destination free.
