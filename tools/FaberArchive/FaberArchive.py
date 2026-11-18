"""Read an archive on the Fusion application thread."""

import json
import math
import os
import re
from pathlib import Path
import threading

import adsk.core
import adsk.fusion

EVENT = "org.faber.readArchive"
SPOOL = Path(os.environ["LOCALAPPDATA"]) / "Faber" / "Archive"
handlers = []
stop_event = None
worker = None


def require(condition, message):
    if not condition:
        raise ValueError(message)


def xy(point):
    return [point.x * 10, point.y * 10]


def close(a, b):
    return math.dist(a, b) < 1e-6


def length_parameter(parameter, key, params, lengths, notes):
    while key in params:
        key += "_"
    original = parameter.expression.strip()
    params[key] = lengths.get(original, str(10 * parameter.value))
    if original not in lengths and not re.fullmatch(r"[+-]?[\d.]+(?:\s*[A-Za-z]+)?", original):
        notes.append(f"{key}: source expression {original} uses its solved value")
    return key


def loop_recipe(loop, radius_for):
    curves = [curve.geometry for curve in loop.profileCurves]
    if len(curves) == 1 and adsk.core.Circle3D.cast(curves[0]):
        entity = loop.profileCurves.item(0).sketchEntity
        return dict(kind="circle", centre=xy(curves[0].center), radius=radius_for(entity, curves[0]))
    edges = []
    for curve in curves:
        line = adsk.core.Line3D.cast(curve)
        arc = adsk.core.Arc3D.cast(curve)
        require(line or arc, "Unsupported profile curve " + curve.objectType)
        middle = None
        if arc:
            ok, start, end = arc.evaluator.getParameterExtents()
            require(ok, "Cannot read the arc parameters")
            ok, point = arc.evaluator.getPointAtParameter((start + end) / 2)
            require(ok, "Cannot read the arc midpoint")
            middle = xy(point)
        edges.append((xy(curve.startPoint), xy(curve.endPoint), middle))
    require(edges, "Empty sketch loop")
    start = edges[0][0]
    current = start
    segments = []
    while edges:
        index = next((i for i, (a, b, _) in enumerate(edges)
                      if close(a, current) or close(b, current)), None)
        require(index is not None, "The profile curves do not form a loop")
        a, b, middle = edges.pop(index)
        current = b if close(a, current) else a
        segments.append(dict(kind="arc", via=middle, end=current) if middle
                        else dict(kind="line", end=current))
    require(close(start, current), "Open sketch loop")
    return dict(kind="path", start=start, segments=segments)


def sketch_recipe(sketch, profile, radius_for):
    outer = None
    holes = []
    for loop in profile.profileLoops:
        if loop.isOuter:
            require(outer is None, "More than one outer loop")
            outer = loop_recipe(loop, radius_for)
        else:
            holes.append(loop_recipe(loop, radius_for))
    require(outer is not None, "Missing outer loop")
    x, y = sketch.xDirection, sketch.yDirection
    normal = x.crossProduct(y)
    pitch = math.atan2(-x.z, math.hypot(x.x, x.y))
    if math.hypot(x.x, x.y) > 1e-9:
        roll, yaw = math.atan2(y.z, normal.z), math.atan2(x.y, x.x)
    else:
        roll, yaw = 0, math.atan2(-y.x, y.y)
    origin = sketch.origin
    placement = dict(translation=[10 * n for n in origin.asArray()],
                     rotation=[math.degrees(v) for v in (roll, pitch, yaw)])
    return dict(op="sketch", profile=dict(outer=outer, holes=holes), placement=placement)


def extract(design, name):
    component = design.rootComponent
    require(component.occurrences.count == 0, "Component occurrences require an assembly importer")
    require(component.meshBodies.count == 0, "Mesh bodies require a mesh importer")
    require(component.bRepBodies.count == 1, "The design must contain one solid body")
    features = []
    params = {}
    notes = []
    lengths = {}
    for index, user in enumerate(design.userParameters):
        if not design.unitsManager.isValidExpression("1 mm", user.unit):
            notes.append(f"{user.name}: unsupported parameter unit {user.unit}")
            continue
        key = user.name if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", user.name) else f"user_{index + 1}"
        if key == "pi" or key in params:
            key = f"user_{index + 1}_{key}"
        lengths[user.name] = key
        params[key] = str(10 * user.value)
        if user.expression.strip() in lengths and user.expression.strip() != user.name:
            params[key] = lengths[user.expression.strip()]
        elif re.search(r"[A-Za-z_]", user.expression) and not re.fullmatch(
                r"[+-]?[\d.]+\s*[A-Za-z]+", user.expression.strip()):
            notes.append(f"{user.name}: source expression uses its solved value")
    output = None
    sketches = {}
    timeline = design.timeline
    require(timeline.markerPosition == timeline.count, "The timeline ends before the last feature")
    for i in range(timeline.count):
        item = timeline.item(i)
        entity = item.entity
        label = f"Timeline item {i + 1} ({item.name})"
        try:
            require(entity and not item.isSuppressed, "Missing or suppressed feature")
            sketch = adsk.fusion.Sketch.cast(entity)
            if sketch:
                require(sketch.profiles.count > 0, "Expected a closed sketch profile")
                key = f"sketch_{i + 1}"
                sketches[i] = []
                linked = {}

                def radius_for(entity, circle):
                    for index, dim in enumerate(sketch.sketchDimensions):
                        diameter = adsk.fusion.SketchDiameterDimension.cast(dim)
                        radial = adsk.fusion.SketchRadialDimension.cast(dim)
                        if (diameter or radial) and dim.isDriving and dim.entity == entity:
                            if index not in linked:
                                linked[index] = length_parameter(dim.parameter, f"{key}_dim_{index + 1}",
                                                                 params, lengths, notes)
                            return f"{linked[index]} / 2" if diameter else linked[index]
                    return 10 * circle.radius

                used_curves = []
                for region, profile in enumerate(sketch.profiles):
                    profile_id = key if region == 0 else f"{key}_region_{region + 1}"
                    geometry = sketch_recipe(sketch, profile, radius_for)
                    sketches[i].append((profile, profile_id))
                    features.append(dict(id=profile_id, geometry=geometry))
                    used_curves.extend(c.sketchEntity for loop in profile.profileLoops for c in loop.profileCurves)
                if sketch.profiles.count > 1:
                    notes.append(f"{item.name}: each closed profile has a separate sketch feature")
                if sketch.sketchDimensions.count > len(linked) or sketch.geometricConstraints.count:
                    notes.append(f"{item.name}: other dimensions and geometric constraints use solved coordinates")
                if any(c not in used_curves for c in sketch.sketchCurves):
                    notes.append(f"{item.name}: curves outside the solid profile were omitted")
                notes.append(f"{key}: {item.name}. The sketch uses its source plane position")
                continue
            extrusion = adsk.fusion.ExtrudeFeature.cast(entity)
            require(extrusion, "Unsupported feature " + entity.objectType)
            require(extrusion.isSolid and not extrusion.isThinExtrude, "Unsupported extrusion wall")
            require(extrusion.extentType == adsk.fusion.FeatureExtentTypes.OneSideFeatureExtentType,
                    "Expected a one-sided extrusion")
            require(adsk.fusion.ProfilePlaneStartDefinition.cast(extrusion.startExtent),
                    "Expected an extrusion from the sketch plane")
            extent = adsk.fusion.DistanceExtentDefinition.cast(extrusion.extentOne)
            require(extent and abs(extrusion.taperAngleOne.value) < 1e-10,
                    "Expected a distance extrusion with no taper")
            profile = adsk.fusion.Profile.cast(extrusion.profile)
            require(profile, "Expected one sketch profile")
            source_sketch = profile.parentSketch
            source = next((key for region, key in sketches[source_sketch.timelineObject.index]
                           if region == profile), None)
            require(source, "The extrusion profile does not match its source sketch")
            normal = source_sketch.xDirection.crossProduct(source_sketch.yDirection)
            parameter = length_parameter(extent.distance, f"distance_{i + 1}", params, lengths, notes)
            vector = [f"{n:.16f} * {parameter}" if abs(n) > 1e-12 else 0 for n in normal.asArray()]
            key = f"extrude_{i + 1}"
            notes.append(f"{key}: {item.name}")
            features.append(dict(id=key, geometry=dict(op="extrude_sketch", source=source, vector=vector)))
            ops = adsk.fusion.FeatureOperations
            if extrusion.operation == ops.NewBodyFeatureOperation:
                require(output is None, "More than one body in the timeline")
                output = key
            else:
                op = {ops.JoinFeatureOperation: "union", ops.CutFeatureOperation: "difference",
                      ops.IntersectFeatureOperation: "intersection"}.get(extrusion.operation)
                require(op and output, "Unsupported extrusion operation")
                result = f"{op}_{i + 1}"
                features.append(dict(id=result, geometry=dict(op=op, left=output, right=key)))
                output = result
        except Exception as error:
            raise ValueError(f"{label}: {error}") from error
    require(output, "No supported solid features in the archive")
    part = dict(name=component.name, geometry=dict(op="history", features=features, output=output),
                colour=[180, 180, 180], note="\n".join(notes))
    project = dict(schema=1, revision=0, name=name, params=params, parts={"body": part})
    source = component.bRepBodies.item(0)
    box = source.preciseBoundingBox
    bounds = [[10 * n for n in point.asArray()] for point in (box.minPoint, box.maxPoint)]
    properties = source.getPhysicalProperties(adsk.fusion.CalculationAccuracy.VeryHighCalculationAccuracy)
    return dict(project=project, volume=1000 * properties.volume, bounds=bounds)


def read_archive(path):
    require(path.is_absolute() and path.suffix.lower() == ".f3d" and path.is_file(),
            "Expected a local .f3d file")
    app = adsk.core.Application.get()
    manager = app.importManager
    doc = manager.importToNewDocument(manager.createFusionArchiveImportOptions(str(path)))
    require(doc, "Fusion could not open the archive")
    try:
        design = adsk.fusion.Design.cast(doc.products.itemByProductType("DesignProductType"))
        require(design, "Archive has no design")
        return extract(design, path.stem)
    finally:
        doc.close(False)


class ReadJob(adsk.core.CustomEventHandler):
    def notify(self, args):
        for request in SPOOL.glob("job-*/request"):
            try:
                claimed = request.with_name("reading")
                request.rename(claimed)
                try:
                    path = Path(json.loads(claimed.read_text(encoding="utf-8")))
                    reply = dict(imported=read_archive(path))
                except Exception as error:
                    reply = dict(error=str(error))
                pending = claimed.with_name("pending")
                pending.write_text(json.dumps(reply, allow_nan=False), encoding="utf-8")
                pending.replace(claimed.with_name("reply"))
            except FileNotFoundError:
                continue


def run(context):
    global stop_event, worker
    app = adsk.core.Application.get()
    event = app.registerCustomEvent(EVENT)
    handler = ReadJob()
    event.add(handler)
    handlers.extend([event, handler])
    SPOOL.mkdir(parents=True, exist_ok=True)
    stop_event = threading.Event()

    def poll():
        while not stop_event.wait(0.5):
            (SPOOL / "active").touch()
            if any(SPOOL.glob("job-*/request")):
                app.fireCustomEvent(EVENT)

    worker = threading.Thread(target=poll, daemon=True)
    worker.start()


def stop(context):
    if stop_event:
        stop_event.set()
        worker.join()
    adsk.core.Application.get().unregisterCustomEvent(EVENT)
    handlers.clear()
    (SPOOL / "active").unlink(missing_ok=True)
