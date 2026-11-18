"""Exercise the archive mapping with local API records."""

import importlib.util
from pathlib import Path
import sys
import types
import unittest
from unittest.mock import patch


class Items(list):
    count = property(len)
    item = list.__getitem__


class Point:
    def __init__(self, x, y, z=0):
        self.x, self.y, self.z = x, y, z

    def asArray(self):
        return self.x, self.y, self.z

    def crossProduct(self, other):
        return Point(self.y * other.z - self.z * other.y, self.z * other.x - self.x * other.z,
                     self.x * other.y - self.y * other.x)


def record(**fields):
    return types.SimpleNamespace(**fields)


def api_class(name):
    class ApiType(types.SimpleNamespace):
        @classmethod
        def cast(cls, value):
            return value if isinstance(value, cls) else None
    ApiType.objectType = name
    return ApiType


def fixture():
    adsk = types.ModuleType("adsk")
    adsk.core = types.ModuleType("adsk.core")
    adsk.fusion = types.ModuleType("adsk.fusion")
    for name in ("CustomEventHandler", "Circle3D", "Line3D", "Arc3D"):
        setattr(adsk.core, name, api_class(name))
    f = adsk.fusion
    for name in ("Sketch", "ExtrudeFeature", "DistanceExtentDefinition", "ProfilePlaneStartDefinition",
                 "Profile", "SketchDiameterDimension", "SketchRadialDimension"):
        setattr(f, name, api_class(name))
    f.FeatureExtentTypes = record(OneSideFeatureExtentType=0)
    f.FeatureOperations = record(NewBodyFeatureOperation=3)
    f.CalculationAccuracy = record(VeryHighCalculationAccuracy=3)
    modules = {"adsk": adsk, "adsk.core": adsk.core, "adsk.fusion": f}
    source = Path(__file__).resolve().parents[1] / "tools/FaberArchive/FaberArchive.py"
    spec = importlib.util.spec_from_file_location("archive_addin", source)
    adapter = importlib.util.module_from_spec(spec)
    with patch.dict(sys.modules, modules), patch.dict("os.environ", {"LOCALAPPDATA": "."}):
        spec.loader.exec_module(adapter)
    circle = adsk.core.Circle3D(center=Point(0, 0), radius=1)
    curve = record(geometry=circle, sketchEntity=record())
    dim = f.SketchDiameterDimension(entity=curve.sketchEntity, isDriving=True,
                                    parameter=record(value=2, expression="Width"))
    sketch = f.Sketch(origin=Point(0, 0), xDirection=Point(1, 0), yDirection=Point(0, 1),
                      timelineObject=record(index=0), geometricConstraints=Items(),
                      sketchDimensions=Items([dim]), sketchCurves=Items([curve.sketchEntity]))
    profile = f.Profile(parentSketch=sketch, profileLoops=Items([record(
        isOuter=True, profileCurves=Items([curve]))]))
    sketch.profiles = Items([profile])
    pad = f.ExtrudeFeature(isSolid=True, isThinExtrude=False, extentType=0, operation=3,
                           startExtent=f.ProfilePlaneStartDefinition(), profile=profile,
                           extentOne=f.DistanceExtentDefinition(distance=record(expression="2 mm", value=.2)),
                           taperAngleOne=record(value=0))
    timeline = Items([record(name="Circle", entity=sketch, isSuppressed=False),
                      record(name="Pad", entity=pad, isSuppressed=False)])
    timeline.markerPosition = len(timeline)
    body = record(preciseBoundingBox=record(minPoint=Point(-1, -1), maxPoint=Point(1, 1, .2)),
                  getPhysicalProperties=lambda _: record(volume=.2 * 3.141592653589793))
    component = record(name="Disc", occurrences=Items(), meshBodies=Items(), bRepBodies=Items([body]))
    design = record(rootComponent=component, timeline=timeline, unitsManager=record(
        isValidExpression=lambda expression, unit: unit == "mm"), userParameters=Items([
            record(name="Width", expression="20 mm", value=2, unit="mm")]))
    return adapter, design


class ArchiveMapping(unittest.TestCase):
    def test_circle_dimension_and_extrusion_keep_their_parameters(self):
        adapter, design = fixture()
        result = adapter.extract(design, "Disc")
        project = result["project"]
        self.assertEqual(result["bounds"], [[-10, -10, 0], [10, 10, 2]])
        params = project["params"]
        self.assertEqual(float(params["Width"]), 20)
        self.assertEqual(params["sketch_1_dim_1"], "Width")
        self.assertEqual(float(params["distance_2"]), 2)
        sketch, pad = project["parts"]["body"]["geometry"]["features"]
        self.assertEqual(sketch["geometry"]["profile"]["outer"]["radius"], "sketch_1_dim_1 / 2")
        self.assertEqual(pad["geometry"]["source"], sketch["id"])
        self.assertIn("distance_2", pad["geometry"]["vector"][2])

    def test_the_extrusion_uses_the_selected_region(self):
        adapter, design = fixture()
        sketch = design.timeline[0].entity
        curve = record(geometry=adapter.adsk.core.Circle3D(center=Point(3, 0), radius=.5),
                       sketchEntity=record(other=True))
        second = adapter.adsk.fusion.Profile(parentSketch=sketch, profileLoops=Items([
            record(isOuter=True, profileCurves=Items([curve]))]))
        sketch.profiles.append(second)
        sketch.sketchCurves.append(curve.sketchEntity)
        design.timeline[1].entity.profile = second
        result = adapter.extract(design, "Regions")
        features = result["project"]["parts"]["body"]["geometry"]["features"]
        self.assertEqual(len(features), 3)
        self.assertEqual(features[2]["geometry"]["source"], features[1]["id"])
        self.assertEqual(features[1]["geometry"]["profile"]["outer"]["centre"], [30, 0])

    def test_unsupported_operation_names_the_source_item(self):
        adapter, design = fixture()
        design.timeline[1].entity = record(objectType="LoftFeature")
        with self.assertRaisesRegex(ValueError, "Timeline item 2 .*LoftFeature"):
            adapter.extract(design, "Disc")


if __name__ == "__main__":
    unittest.main()
