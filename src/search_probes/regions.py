"""Isolated 7.04 region generation; call only on the gameplay thread."""

from __future__ import annotations

import ctypes
import math
import struct

from search_probes import resident_search_protocol as protocol
from search_probes.native_memory import validate_pointer

REGION_MASK = (1 << 40) - 1
MAX_REGION_SYSTEMS = 4095
VOXEL_SIZE = 0x260


def region_coordinates(address: int) -> tuple[int, int, int]:
    def signed(value: int, bits: int) -> int:
        return value - (1 << bits) if value & (1 << (bits - 1)) else value

    return (
        signed(address & 0xFFF, 12),
        signed((address >> 24) & 0xFF, 8),
        signed((address >> 12) & 0xFFF, 12),
    )


def region_address(x: int, y: int, z: int, reality: int) -> int:
    # Native coordinate consumers clamp to these bounds. Never emit their aliases.
    if not (-2047 <= x <= 2047 and -127 <= y <= 127 and -2047 <= z <= 2047):
        raise ValueError("region coordinates are outside the native galaxy bounds")
    return protocol.universe_address_from_components(
        planet_index=0,
        solar_system_index=0,
        voxel_x=x,
        voxel_y=y,
        voxel_z=z,
        reality_index=reality,
    )


def expanding_regions(origin: int):
    """Unique, clipped cube shells, with constant traversal storage."""
    cx, cy, cz = region_coordinates(origin)
    reality = protocol.universe_address_reality_index(origin)
    region_address(cx, cy, cz, reality)
    maximum = max(cx + 2047, 2047 - cx, cy + 127, 127 - cy, cz + 2047, 2047 - cz)
    yield origin & REGION_MASK
    for radius in range(1, maximum + 1):
        xmin, xmax = max(-2047, cx - radius), min(2047, cx + radius)
        ymin, ymax = max(-127, cy - radius), min(127, cy + radius)
        zmin, zmax = max(-2047, cz - radius), min(2047, cz + radius)
        # Visit faces without scanning the interior or repeating edges/corners.
        for x in (cx - radius, cx + radius):
            if xmin <= x <= xmax:
                for y in range(ymin, ymax + 1):
                    for z in range(zmin, zmax + 1):
                        yield region_address(x, y, z, reality)
        for y in (cy - radius, cy + radius):
            if ymin <= y <= ymax:
                for x in range(
                    max(xmin, cx - radius + 1), min(xmax, cx + radius - 1) + 1
                ):
                    for z in range(zmin, zmax + 1):
                        yield region_address(x, y, z, reality)
        for z in (cz - radius, cz + radius):
            if zmin <= z <= zmax:
                for x in range(
                    max(xmin, cx - radius + 1), min(xmax, cx + radius - 1) + 1
                ):
                    for y in range(
                        max(ymin, cy - radius + 1), min(ymax, cy + radius - 1) + 1
                    ):
                        yield region_address(x, y, z, reality)


def generate_region(native, address: int) -> list[dict[str, object]]:
    """Copy actual native addresses, including the noncontiguous purple-system range.

    The voxel constructor, Populate, and destructor are the same isolated lifecycle
    used by native portal validation and system-distance calculations. No cache or
    Galaxy Map object is passed to them. All allocations die before returning.
    """
    if address != address & REGION_MASK:
        raise ValueError("region address must have zero system and planet fields")
    region_address(
        *region_coordinates(address), protocol.universe_address_reality_index(address)
    )
    storage = (ctypes.c_uint8 * (VOXEL_SIZE + 47))()
    pointer = (ctypes.addressof(storage) + 15) & ~15
    ctypes.memset(pointer + VOXEL_SIZE, 0xA5, 32)
    root_storage = (ctypes.c_uint8 * 31)()
    root = (ctypes.addressof(root_storage) + 15) & ~15
    # Populate uses aligned SIMD memory operands; ctypes arrays are only 8-byte
    # aligned on the bundled Windows Python. Explicitly align both owned inputs.
    ctypes.c_float.from_address(root + 12).value = 1.0
    native.construct_voxel(pointer)
    try:
        native.populate_voxel(address, pointer, root)
        count = ctypes.c_uint32.from_address(pointer + 0x254).value
        positions_count = ctypes.c_uint32.from_address(pointer + 0x214).value
        if count != positions_count or count > MAX_REGION_SYSTEMS:
            raise RuntimeError(
                f"invalid generated region counts: {count}/{positions_count}"
            )
        if not count:
            return []
        addresses_pointer = validate_pointer(
            "region addresses", ctypes.c_void_p.from_address(pointer + 0x258).value or 0
        )
        positions_pointer = validate_pointer(
            "region positions", ctypes.c_void_p.from_address(pointer + 0x218).value or 0
        )
        addresses = struct.unpack(
            f"<{count}Q", ctypes.string_at(addresses_pointer, count * 8)
        )
        positions = struct.iter_unpack(
            "<3f", ctypes.string_at(positions_pointer, count * 12)
        )
        if len(set(addresses)) != count:
            raise RuntimeError("native region contains duplicate system addresses")
        result = []
        reality = protocol.universe_address_reality_index(address)
        for system, position in zip(addresses, positions, strict=True):
            if (
                system & REGION_MASK != address
                or not 1 <= system >> 40 <= 0xFFF
                or not all(math.isfinite(value) for value in position)
            ):
                raise RuntimeError("native region returned an invalid system")
            result.append(
                {
                    "address": f"0x{system:016X}",
                    "reality_index": reality,
                    "galaxy_number": reality + 1,
                    "region_address": f"0x{address:016X}",
                    "source": "region",
                }
            )
        return result
    finally:
        native.destroy_voxel(pointer)
        if ctypes.string_at(pointer + VOXEL_SIZE, 32) != b"\xa5" * 32:
            raise RuntimeError("native region output exceeded its owned allocation")


class CandidateStream:
    """Keep only the initial local set and one generated region per search."""

    def __init__(self, nearby, origin: int, limit: int):
        self.origin = origin
        self.limit = limit
        self.local_addresses = {int(row["address"], 0) for row in nearby}
        self.local_addresses.add(origin)
        self.batch = [row for row in nearby if int(row["address"], 0) != origin][:limit]
        self.index = 0
        self.checked = 0
        self.regions_generated = 0
        self.region_checked = 0
        self.exhausted = False
        self.regions = expanding_regions(origin)

    @property
    def done(self) -> bool:
        return self.checked >= self.limit or self.exhausted

    def take(self):
        if self.done or self.index == len(self.batch):
            return None
        candidate = dict(self.batch[self.index])
        self.index += 1
        self.checked += 1
        candidate["rank"] = self.checked
        if candidate.get("source") == "region":
            self.region_checked += 1
        else:
            candidate["source"] = "nearby"
        return candidate

    def expand(self, generate):
        """Generate at most one region per update, including empty/overlapping regions."""
        if self.done or self.index < len(self.batch):
            return
        address = next(self.regions, None)
        if address is None:
            self.exhausted = True
            return
        rows = generate(address)
        self.regions_generated += 1
        self.batch = [
            row for row in rows if int(row["address"], 0) not in self.local_addresses
        ]
        self.index = 0
