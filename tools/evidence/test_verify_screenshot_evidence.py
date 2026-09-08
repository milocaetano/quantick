"""Independent authored artifacts and semantic corruption tests; no OS DPI claim."""

import base64
import copy
import json
import struct
import unittest
import zlib

from verify_screenshot_evidence import EvidenceError, decode_png, digest, verify


def png_chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def fixture():
    pixels = bytes([20, 60, 100, 255]) * 12
    rows = b"".join(b"\0" + pixels[start:start + 16] for start in range(0, 48, 16))
    png = (b"\x89PNG\r\n\x1a\n"
           + png_chunk(b"IHDR", struct.pack(">IIBBBBB", 4, 3, 8, 6, 0, 0, 0))
           + png_chunk(b"IDAT", zlib.compress(rows)) + png_chunk(b"IEND", b""))
    controls = [
        {"control_id": "pane.7.canvas", "bounds": {"x_pt": "1", "y_pt": "1",
         "width_pt": "2", "height_pt": "1"}, "bounds_availability": {"available": True}},
        {"control_id": "pane.8.canvas", "bounds": {"x_pt": "-1", "y_pt": "0",
         "width_pt": "1", "height_pt": "1"}, "bounds_availability": {"available": True}},
        {"control_id": "toolbar.layers", "bounds_availability":
         {"available": False, "reason": "bounds_not_recorded"}},
    ]
    descriptor = {"capture_revision": "9", "width_px": 4, "height_px": 3,
                  "pixels_per_point": "1.5", "format": "png", "image_digest": digest(png),
                  "image_bytes": str(len(png)), "control_regions": [
                      {"control_id": "pane.7.canvas", "x_px": "1.5", "y_px": "1.5",
                       "width_px": "3", "height_px": "1.5", "within_image": False},
                      {"control_id": "pane.8.canvas", "x_px": "-1.5", "y_px": "0",
                       "width_px": "1.5", "height_px": "1.5", "within_image": False}],
                  "controls_without_region": [{"subject": "toolbar.layers", "reason": "bounds_not_recorded"}]}
    document = {"evidence_id": "evidence.fixture", "instance_id": "instance.fixture",
                "session_id": "session.fixture", "bundle_version": 1, "capture_revision": "9",
                "captured_at_unix_ms": 123, "environment": {"fixture": "authored synthetic geometry"},
                "coverage": {"not_captured": [{"subject": "screenshot.state_skew",
                             "reason": "pixels_precede_projections_by_one_drain"}]},
                "snapshot": {"instance_id": "instance.fixture", "capture_revision": "9", "scopes": {
                    "scene.controls": {"value": {"controls": controls}}}},
                "screenshot": {"descriptor": descriptor, "image_base64": base64.b64encode(png).decode("ascii")}}
    return document, png, pixels


def package(document):
    """Rehash deliberate corruptions so negative tests reach semantic checks."""
    bundle = json.dumps(document, sort_keys=True, separators=(",", ":")).encode("utf-8")
    manifest = {key: copy.deepcopy(document[key]) for key in (
        "evidence_id", "instance_id", "session_id", "bundle_version", "capture_revision",
        "captured_at_unix_ms", "environment", "coverage")}
    chunks = [bundle[start:start + 256] for start in range(0, len(bundle), 256)]
    manifest.update(content_digest=digest(bundle), encoded_bytes=str(len(bundle)), chunk_bytes="256",
                    chunk_count=len(chunks), chunk_digests=[digest(chunk) for chunk in chunks],
                    screenshot=copy.deepcopy(document["screenshot"]["descriptor"]))
    return manifest, bundle


class ScreenshotEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.document, self.png, self.pixels = fixture()

    def assert_rehashed_rejected(self, message):
        manifest, bundle = package(self.document)
        with self.assertRaisesRegex(EvidenceError, message):
            verify(manifest, bundle)

    def test_original_fractional_clipped_artifact_and_every_pixel(self):
        manifest, bundle = package(self.document)
        result = verify(manifest, bundle, self.png)
        self.assertEqual(result["status"], "PASS")
        self.assertEqual((result["regions"], result["missing_bounds"]), (2, 1))
        self.assertEqual(decode_png(self.png), (4, 3, self.pixels))
        self.assertGreater(manifest["chunk_count"], 1)

    def test_reformatted_original_bundle_is_not_silently_canonicalized(self):
        manifest, bundle = package(self.document)
        with self.assertRaisesRegex(EvidenceError, "original bundle digest"):
            verify(manifest, bundle + b"\n")

    def test_retained_png_tampering_is_rejected(self):
        manifest, bundle = package(self.document)
        with self.assertRaisesRegex(EvidenceError, "retained original PNG"):
            verify(manifest, bundle, self.png[:-1] + bytes([self.png[-1] ^ 1]))

    def test_rehashed_png_corruption_still_fails_crc(self):
        broken = self.png[:-1] + bytes([self.png[-1] ^ 1])
        self.document["screenshot"]["image_base64"] = base64.b64encode(broken).decode("ascii")
        self.document["screenshot"]["descriptor"]["image_digest"] = digest(broken)
        self.assert_rehashed_rejected("PNG chunk CRC")

    def test_rehashed_wrong_region_id_is_rejected(self):
        self.document["screenshot"]["descriptor"]["control_regions"][0]["control_id"] = "pane.99.canvas"
        self.assert_rehashed_rejected("region/scene ID")

    def test_rehashed_wrong_coordinate_is_rejected(self):
        self.document["screenshot"]["descriptor"]["control_regions"][0]["x_px"] = "2"
        self.assert_rehashed_rejected("region geometry")

    def test_rehashed_wrong_clipping_status_is_rejected(self):
        self.document["screenshot"]["descriptor"]["control_regions"][0]["within_image"] = True
        self.assert_rehashed_rejected("within_image")

    def test_rehashed_wrong_revision_and_instance_are_rejected(self):
        for field in ("capture_revision", "instance_id"):
            with self.subTest(field=field):
                original = self.document["snapshot"][field]
                self.document["snapshot"][field] = "wrong-association"
                self.assert_rehashed_rejected("snapshot association")
                self.document["snapshot"][field] = original

    def test_rehashed_missing_bounds_reason_is_not_interchangeable(self):
        self.document["screenshot"]["descriptor"]["controls_without_region"][0]["reason"] = "different_reason"
        self.assert_rehashed_rejected("missing-bounds coded gap")

    def test_rehashed_duplicate_region_id_is_rejected(self):
        regions = self.document["screenshot"]["descriptor"]["control_regions"]
        regions.append(copy.deepcopy(regions[0]))
        self.assert_rehashed_rejected("duplicate region ID")

    def test_rehashed_missing_skew_disclosure_is_rejected(self):
        self.document["coverage"]["not_captured"] = []
        self.assert_rehashed_rejected("one-drain skew")

    def test_manifest_chunk_digest_corruption_is_rejected(self):
        manifest, bundle = package(self.document)
        manifest["chunk_digests"][0] = digest(b"wrong chunk")
        with self.assertRaisesRegex(EvidenceError, "chunk digests"):
            verify(manifest, bundle)


if __name__ == "__main__":
    unittest.main()
