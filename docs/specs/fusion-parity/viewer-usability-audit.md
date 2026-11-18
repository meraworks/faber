# Viewer usability audit against Fusion

Scope: `crates/faber-viewer` (`src/app.rs`, `src/app/*.rs`, `src/selection.rs` is `src/app/selection.rs`, `src/sketch.rs`, `src/feature_form.rs`). Method: code reading only. No build, no window.

Main finding: the viewer picks whole parts only. It has no face, edge or point picking in the 3D view, except the points of the Move tool. Feature forms are text forms that show raw JSON paths. Fusion users expect to click.

## A. Windows that cannot be closed or that leave state

Closing keys: X = title bar button. Esc = Escape key. Cancel = button. Tool = a second tool replaces it.

| Window | file:line | X | Esc | Cancel | Other tool | State left behind |
|---|---|---|---|---|---|---|
| Create project | app.rs:410 | no | no | yes (427) | n/a (welcome screen) | `self.error` stays |
| Move / rotate part | app.rs:492 | no | no | yes | blocked (`placement_available` 1641) | `self.error` stays |
| Import model | app.rs:590 | no | no | yes (660) | blocked | `self.error` stays |
| Export design | app.rs:672 | yes | no | none | not blocked | `export_message`, `export_path`, `export_force` stay; a running export goes on |
| Design checks | app.rs:721 | yes | no | none | not blocked | `check_result` stays; `strict_edits` stays on while the window is closed and hidden |
| Edit parameters | app.rs:823 | no (`collapsible(false)`) | no | yes | blocked | `self.error` stays |
| New feature / Edit feature | app.rs:1206 | no | no | yes ("Cancel feature") | A feature name button (app.rs:1955) replaces the draft without a warning; the typed values are lost | `self.error` stays |
| Create sketch | app.rs:1301, sketch.rs:150 | no | no | yes ("Cancel sketch") | blocked | drawn points are lost without a warning; `self.error` stays |
| MOVE/COPY panel | move_tool.rs:975 | no | yes (945) | yes | blocked | `cancel_move` (831) clears the error; good |
| Section analysis (canvas handles) | app.rs:2430 and later | n/a | no | n/a | S key or Section button | `section_pivot` resets only when the section is off (2278) |
| Right-click menus | app.rs:1682 | n/a | egui closes it | n/a | n/a | none |

Defects:

1. Five windows have no X button and no Escape: Create project, Move / rotate part, Import model, Edit parameters, feature form, Create sketch. Only the Cancel button closes them.
2. `self.error` is one global field. Cancel does not clear it (only `cancel_move` does). A stale error from one tool shows in the next window and in the status bar (app.rs:3040).
3. Escape does different things. In the Move tool it cancels. With no window it clears the selection (selection.rs:114). With any other window open and no text focus, it also clears the selection under the form. It never closes a window.
4. Escape in the Move tool cancels the whole tool, also while a point is being picked (move_tool.rs:945) or a combo box is open. Fusion cancels the inner step first.
5. The "Create sketch" window is about 700 px high and has no scroll area (app.rs:1301, canvas 360 px at sketch.rs:177). On a 768 px screen the buttons "Create solid" and "Cancel sketch" can be outside the screen. The window then cannot be closed or finished.
6. The MOVE/COPY panel is fixed, not movable and has no scroll (move_tool.rs:975-981). The OK and Cancel buttons can be outside a small viewport. Escape still works.
7. Export and Checks open at the default position and can cover the left tree and the header. Their open flag is not tied to a tool; they stay open while the user edits.
8. A background save (`poll`, app.rs:1450) closes all drafts at once. Typed values in an open form are lost if an external file change starts a reload with `saving` set. The external reload path (1492) does not close drafts, so the form later fails with "Project changed".

## B. Mouse and keyboard gaps versus Fusion

| Interaction | Status | file:line | Note |
|---|---|---|---|
| Click selects a part | present | selection.rs:35, app.rs:2616 | Part level only. No face, edge or vertex. |
| Ctrl click toggles | present | selection.rs:65 | Part level. |
| Shift click adds | missing on canvas | app.rs:2625 | Shift works only in the tree (app.rs:1846). |
| Rectangle select | partial | selection.rs:127-222 | Uses the bounding box of each part, not the shape. Right to left touches. Fusion rule, but only for parts. |
| Click on empty space clears | present | selection.rs:41 | |
| Double-click selects loop or tangent chain | missing | none | No `double_clicked` in the crate. |
| Hover highlight | missing | none | No hover state on bodies. Only grips (move_tool.rs:1375) and section handles (app.rs:2525). |
| Selection filter or "select other" | missing | none | |
| Right-click context menu | partial | app.rs:2638, 1682 | Part actions only. No empty-space menu with Undo, Repeat. The menu has no "Edit feature" or "Properties". |
| Marking menu (radial) | missing | none | |
| Face or edge pick for Fillet, Chamfer | missing | feature_form.rs:121 | The form asks for edge direction as numbers and an edge count. |
| Face pick for Plane on face, Thicken, Hole | missing | feature_form.rs:73, 112, 127 | Help text says "Select exactly one planar face" but the user cannot select. |
| Pick points for Move | present | move_tool.rs:1231 | The only picking in the app. |
| Press-pull a face | missing | none | No tool. |
| Extrude tool and distance arrow | missing | sketch.rs:393 | "Create sketch" makes a solid with a typed height. No feature `Extrude` in `ToolKind` (app.rs:90). |
| Drag handles on Move triad | present | move_tool.rs:1200 | Snap 1 mm, 15 degrees, Shift for fine. |
| Section handle drag | present | app.rs:2430-2560 | |
| Sketch on a plane or face in 3D | missing | sketch.rs:150 | A separate 2D window with whole-millimetre snap. No drag of points, no pan, no constraints. |
| Orbit | present | app.rs:2597 | Shift + middle or Alt + left. No plain left-drag orbit on empty space (left drag selects). Orbit centre is the fixed target, not the cursor or the selection. |
| Pan | present | app.rs:2606 | Middle drag. Right drag also pans. |
| Zoom with wheel | partial | app.rs:2275 | Zooms to the target, not to the cursor. Touchpad pinch (`zoom_delta`) is not used. |
| View cube | missing | app.rs:335 | Only buttons Top, Front, Iso. The "Iso" button calls `fit()` (app.rs:352), so it is the same as "Fit". No Back, Left, Right, Bottom. |
| Fit | present | app.rs:2917, 347 | F key and button. |
| Double-middle-click fit | missing | none | |
| Zoom window, look at face | missing | none | |
| Delete key | missing | none | Delete exists only in the context menu (app.rs:1730), with no confirmation. |
| Ctrl+Z / Ctrl+Y | missing | app.rs:2930 | Only the Undo and Redo buttons (app.rs:3003). |
| Ctrl+A | present | selection.rs:120 | |
| S (toolbox in Fusion) | conflict | app.rs:2920 | S toggles Section here. Fusion opens the toolbox. |
| Q press pull, E extrude | missing | none | The tools do not exist. |
| M move | present | app.rs:2924 | Needs a selection. |
| F fit | present | app.rs:2917 | |
| Enter applies, Esc cancels in forms | partial | move_tool.rs:941 | Only the Move tool. All other forms need the mouse. |
| Tab between fields | present | egui default | |
| Edit feature by double-click in the tree or the view | missing | app.rs:1955 | A click on the feature name opens the form. No double-click. |

## C. Other UI bugs

1. Shortcuts with open windows. `F`, `S` and Escape fire when a form is open and no text field has focus (app.rs:2917-2928, selection.rs:114). `S` toggles the section under a modal form. Escape clears the selection that the form depends on. The guard only tests `egui_wants_keyboard_input`.
2. Wrong part. The tool buttons use `selected` or else the first part (app.rs:1873: `part_id = selected.or(first)`). A click on "Fillet" with no selection edits an unseen part. The user gets no hint which part is the target.
3. No way to add a parameter. The "Edit parameters" button shows only when `params` is not empty (app.rs:2068). The viewer cannot create the first parameter, and cannot add, rename or delete one (app.rs:823 edits values only).
4. No close of a project. The tree "New project" and "Open project" buttons exist (app.rs:1777), but they replace the project without a check for open drafts being saved (button disabled while a draft is open, so the user must first cancel it).
5. Numbers without units. The feature form shows labels like "size · X" with no unit per field (feature_form.rs:4-45). One line says "Lengths use mm. Angles use degrees." (app.rs:1214). The parameter editor and the parameter list (app.rs:2085) show `name = expression` with no unit. The placement form has no per-field unit (app.rs:492). The status bar shows `mm` only in the corner.
6. Forms apply only on a button, so there is no apply on each key. This is good. But Enter does not apply in the parameter, feature, placement, import and sketch forms. Parameter edits give no preview and rebuild the whole model.
7. No live preview. Feature forms, hole, fillet, shell and pattern show nothing in 3D until saved. A wrong value shows only as a red error after the rebuild.
8. Error feedback. Errors show as red text in the status bar and again in each open window (app.rs:3040, 1270). A modal can show the same text twice. There is no toast and no auto clear. The text stays after Cancel (see A.2). An error from the Checks thread (app.rs:1430) goes to the global field with no link to the Checks window.
9. Hidden strict mode. `strict_edits` is a checkbox inside the Checks window (app.rs:727). It changes every later save, but nothing shows it once the window is closed.
10. Deletes without confirmation. "Delete part" (app.rs:1730), "Delete section" (app.rs:2030) and "Suppress" act at once. Undo exists but only through the toolbar button, and the Undo and Redo buttons have no tooltip with the action name and are always enabled.
11. Selection hidden state. Hiding a selected part drops it from the selection (selection.rs:23). "Isolate" and hide are not part of undo, and they are lost when a project reloads (app.rs:1460).
12. Panels overlap. Floating windows open at default positions over the tree and inspector. The right inspector (up to 640 px) and the Move panel both use the top right of the viewport. On a narrow window the header grows to 104 px (app.rs:2938).
13. Sketch canvas. Wheel zoom has no cursor anchor, clicks snap to whole millimetres with no way to turn the snap off (sketch.rs:224), no point can be dragged, and "Remove point" is the only undo. The scale label "Grid: 10 mm" does not update with zoom (sketch.rs:343).
14. Hint text is wrong or duplicated. The inspector says "Right-drag to pan" and, in the next label, "Middle drag to pan" (app.rs:2258-2262). The context menu on a right-drag release can open after a pan.
15. Blocking file dialog. `project_panel` opens a native dialog on the UI thread (app.rs:420). The viewer freezes while it is open.
16. Cursor feedback. The cursor changes only on the Move triad (move_tool.rs:1220). There is no pan, orbit or pick cursor.

## D. Ten fixes of highest value, in order

1. Close every window the same way: add an X and Escape to all windows, and one `close_all_drafts()` that also clears `self.error`. Test: open each window with kittest, press Escape, assert that the window label is gone and `error` is `None`.
2. Guard shortcuts: no `F`, `S`, `M`, Delete or Escape-clears-selection while any draft window is open. Test: open the feature form, press `S`, assert `section_on` is false.
3. Add Ctrl+Z, Ctrl+Y and the Delete key (with Undo as the answer to a mistake). Test: apply a parameter edit, press Ctrl+Z, assert the revision goes back.
4. Pick faces and edges with the mouse for Fillet, Chamfer, Plane on face, Hole and Thicken. Highlight on hover and on selection. Test: send a click at a known pixel, assert the feature form holds the picked face or edge id.
5. Hover highlight and click-to-select with Shift on the canvas, plus double-click to select a loop or chain. Test: move the pointer over a part, assert the hover id is set; double-click an edge, assert the loop is selected.
6. Make the "Create sketch" window and the Move panel scrollable and keep them inside the screen. Test: run a 1024 x 640 harness, assert that "Cancel sketch" and "OK" are inside the screen rectangle.
7. Add a Press-pull / Extrude tool with a distance arrow and a live preview. Test: select a face, drag the arrow by 10 px, assert that the preview volume changes and one undo step results.
8. Add a view cube (or six view buttons) and a real Iso view, zoom to the cursor and double-middle-click fit. Test: click "Back", assert the camera yaw and pitch; scroll at the left edge, assert the point under the cursor stays.
9. Add parameter creation (name, value with unit) and units on every field in the feature, placement and parameter forms. Test: open the parameters form with no parameters, add "width = 20 mm", assert it is saved and every form field shows a unit label.
10. One error channel with a toast, an automatic clear on Cancel and on success, and a visible strict-mode switch in the header. Test: cause an error, press Cancel, assert no red text remains; toggle strict, close Checks, assert the header shows it.
