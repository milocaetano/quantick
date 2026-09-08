"""Verify original Quantick RGBA8 screenshot bundles using only the stdlib.

Hash raw downloaded bytes: parsing and reserializing is not recovery. Geometry
uses the published decimal scale, independently of application implementation.
This verifies integrity and association, not visible target identity or OS DPI.
"""

import argparse
import base64
from decimal import Decimal, InvalidOperation, ROUND_HALF_EVEN
import hashlib
import json
from pathlib import Path
import struct
import zlib


# Match crates/control/src/limits.rs::CONTROL_EVIDENCE_MAX_BUNDLE_BYTES
# (64 MiB divided among eight retained bundles); recovery imports this owner.
MAX_ARTIFACT_BYTES = 8 * 1024 * 1024
# Allow a 4096-by-4096 image while bounding decoded RGBA storage to 64 MiB.
MAX_PNG_PIXELS = 16_777_216


class EvidenceError(ValueError):
    """An artifact failed a named integrity or association check."""


def require(condition, message):
    if not condition:
        raise EvidenceError(message)


def digest(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def read_json(data):
    return json.loads(data, object_pairs_hook=unique_object)


def decimal(value):
    require(isinstance(value, str), "wire decimal must be a string")
    try:
        number = Decimal(value)
    except InvalidOperation as error:
        raise EvidenceError("invalid wire decimal") from error
    require(number.is_finite(), "nonfinite wire decimal")
    return number


def unsigned(value):
    require(isinstance(value, str) and value.isascii() and value.isdigit(),
            "wire unsigned integer must be a digit string")
    return int(value)


def decode_png(data):
    """Fully decode the noninterlaced RGBA8 PNG emitted by production."""
    require(data[:8] == b"\x89PNG\r\n\x1a\n", "PNG signature")
    offset, width, height = 8, None, None
    compressed = bytearray()
    idat_started, idat_closed, ended = False, False, False
    while offset < len(data):
        require(offset + 12 <= len(data), "truncated PNG chunk")
        length = struct.unpack_from(">I", data, offset)[0]
        kind = data[offset + 4:offset + 8]
        end = offset + 12 + length
        require(end <= len(data), "truncated PNG payload")
        payload = data[offset + 8:end - 4]
        crc = struct.unpack_from(">I", data, end - 4)[0]
        require(zlib.crc32(kind + payload) & 0xFFFFFFFF == crc, "PNG chunk CRC")
        if width is None:
            require(kind == b"IHDR" and length == 13, "PNG first IHDR")
            width, height, bits, color, compression, filtering, interlace = struct.unpack(
                ">IIBBBBB", payload)
            require(0 < width * height <= MAX_PNG_PIXELS, "PNG pixel budget")
            require((bits, color, compression, filtering, interlace) == (8, 6, 0, 0, 0),
                    "expected production noninterlaced RGBA8 PNG")
        elif kind == b"IHDR":
            raise EvidenceError("duplicate PNG IHDR")
        elif kind == b"IDAT":
            require(not idat_closed, "noncontiguous PNG IDAT")
            idat_started = True
            compressed.extend(payload)
        elif kind == b"IEND":
            require(length == 0 and idat_started and end == len(data), "PNG IEND")
            ended = True
            break
        else:
            require(kind[0] & 32 or kind == b"PLTE", "unknown critical PNG chunk")
            idat_closed = idat_started
        offset = end
    require(ended, "missing PNG IEND")
    stride = width * 4
    expected = (stride + 1) * height
    inflater = zlib.decompressobj()
    rows = inflater.decompress(compressed, expected + 1)
    require(len(rows) == expected and inflater.eof and not inflater.unused_data
            and not inflater.unconsumed_tail, "PNG decompressed size/stream")
    previous = bytearray(stride)
    pixels = bytearray()
    for row_start in range(0, expected, stride + 1):
        filter_type = rows[row_start]
        require(filter_type <= 4, "PNG row filter")
        current = bytearray(rows[row_start + 1:row_start + 1 + stride])
        for column in range(stride):
            left = current[column - 4] if column >= 4 else 0
            above = previous[column]
            upper_left = previous[column - 4] if column >= 4 else 0
            if filter_type == 1:
                prediction = left
            elif filter_type == 2:
                prediction = above
            elif filter_type == 3:
                prediction = (left + above) // 2
            elif filter_type == 4:
                estimate = left + above - upper_left
                distances = [abs(estimate - value) for value in (left, above, upper_left)]
                prediction = (left, above, upper_left)[distances.index(min(distances))]
            else:
                prediction = 0
            current[column] = (current[column] + prediction) & 255
        pixels.extend(current)
        previous = current
    return width, height, bytes(pixels)


def verify_geometry(document, descriptor):
    controls = document["snapshot"]["scopes"]["scene.controls"]["value"]["controls"]
    regions, gaps, identities = {}, {}, set()
    for region in descriptor["control_regions"]:
        key = region["control_id"]
        require(key not in regions, "duplicate region ID")
        regions[key] = region
    for gap in descriptor["controls_without_region"]:
        key = gap["subject"]
        require(key not in gaps, "duplicate missing-bounds ID")
        gaps[key] = gap["reason"]
    scale = decimal(descriptor["pixels_per_point"])
    require(scale > 0, "nonpositive scale")
    require(scale == scale.quantize(Decimal("0.01")), "published scale precision")
    for control in controls:
        key = control["control_id"]
        require(key not in identities, "duplicate scene ID")
        identities.add(key)
        bounds = control.get("bounds")
        if bounds is None:
            reason = control["bounds_availability"].get("reason", "bounds_unavailable")
            require(key not in regions and gaps.get(key) == reason, "missing-bounds coded gap")
            continue
        require(key in regions and key not in gaps, "region/scene ID association")
        region = regions[key]
        products = []
        for logical, physical in zip(("x_pt", "y_pt", "width_pt", "height_pt"),
                                     ("x_px", "y_px", "width_px", "height_px")):
            product = decimal(bounds[logical]) * scale
            products.append(product)
            expected = product.quantize(Decimal("0.01"), rounding=ROUND_HALF_EVEN)
            require(decimal(region[physical]) == expected, f"region geometry: {key}/{physical}")
        x, y, width, height = products
        within = (x >= 0 and y >= 0 and width >= 0 and height >= 0
                  and x + width <= descriptor["width_px"]
                  and y + height <= descriptor["height_px"])
        require(isinstance(region["within_image"], bool) and region["within_image"] == within,
                f"region within_image: {key}")
    require(set(regions).isdisjoint(gaps) and set(regions) | set(gaps) == identities,
            "regions/gaps exactly partition scene IDs")
    return len(regions), len(gaps)


def verify(manifest, bundle_bytes, original_png=None):
    require(len(bundle_bytes) <= MAX_ARTIFACT_BYTES, "bundle byte budget")
    require(digest(bundle_bytes) == manifest["content_digest"], "original bundle digest")
    require(len(bundle_bytes) == unsigned(manifest["encoded_bytes"]), "bundle encoded_bytes")
    chunk_size = unsigned(manifest["chunk_bytes"])
    require(chunk_size > 0, "zero chunk size")
    chunks = [bundle_bytes[start:start + chunk_size] for start in range(0, len(bundle_bytes), chunk_size)]
    require(len(chunks) == manifest["chunk_count"], "chunk count")
    require([digest(chunk) for chunk in chunks] == manifest["chunk_digests"], "chunk digests")
    document = read_json(bundle_bytes)
    for field in ("evidence_id", "instance_id", "session_id", "bundle_version", "capture_revision",
                  "captured_at_unix_ms", "environment", "coverage"):
        require(document[field] == manifest[field], f"manifest/document association: {field}")
    for field in ("instance_id", "capture_revision"):
        require(document["snapshot"][field] == document[field], f"snapshot association: {field}")
    image = document["screenshot"]
    descriptor = image["descriptor"]
    require(descriptor == manifest["screenshot"], "manifest/image descriptor")
    require(descriptor["capture_revision"] == document["capture_revision"], "image revision")
    require(descriptor["format"] == "png", "image format")
    png = base64.b64decode(image["image_base64"], validate=True)
    require(digest(png) == descriptor["image_digest"], "original PNG digest")
    require(len(png) == unsigned(descriptor["image_bytes"]), "PNG image_bytes")
    if original_png is not None:
        require(png == original_png, "retained original PNG differs from bundle")
    width, height, pixels = decode_png(png)
    require((width, height) == (descriptor["width_px"], descriptor["height_px"]), "PNG dimensions")
    regions, gaps = verify_geometry(document, descriptor)
    skew = {"subject": "screenshot.state_skew", "reason": "pixels_precede_projections_by_one_drain"}
    require(skew in document["coverage"]["not_captured"], "missing one-drain skew disclosure")
    return {"status": "PASS", "evidence_id": document["evidence_id"],
            "instance_id": document["instance_id"], "capture_revision": document["capture_revision"],
            "bundle_digest": digest(bundle_bytes), "png_digest": digest(png),
            "decoded_rgba_digest": digest(pixels), "width_px": width, "height_px": height,
            "pixels_per_point": descriptor["pixels_per_point"], "regions": regions,
            "missing_bounds": gaps, "state_skew": skew["reason"],
            "limitation": "Integrity/geometry only; original-pixel identity and genuine OS DPI require independent evidence."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--png", type=Path)
    args = parser.parse_args()
    try:
        result = verify(read_json(args.manifest.read_bytes()), args.bundle.read_bytes(),
                        args.png.read_bytes() if args.png else None)
    except (ValueError, KeyError, TypeError, OSError, zlib.error, struct.error) as error:
        parser.exit(1, f"FAIL: {error}\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
