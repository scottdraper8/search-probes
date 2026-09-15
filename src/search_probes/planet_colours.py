from __future__ import annotations

import math
import struct
from collections.abc import Callable

from . import native_memory as memory

SKY_COLOUR_FIELDS = {
    "Cloud": 0x00,
    "SkyFog": 0x20,
    "SkyHeightFog": 0x30,
    "SkyHorizon": 0x40,
    "Sky": 0x70,
}


def water_reflectance(raw: bytes) -> list[float]:
    if len(raw) != 0x80:
        raise ValueError("water colour record must contain 128 bytes")
    scattering = struct.unpack_from("<3f", raw, 0x40)
    transmission = struct.unpack_from("<3f", raw, 0x50)
    scatter_distance, transmit_distance = struct.unpack_from("<2f", raw, 0x68)
    if not all(
        math.isfinite(value) and value > 0
        for value in (scatter_distance, transmit_distance)
    ):
        raise ValueError("water optical distances must be finite and positive")
    result = []
    for scatter, transmit in zip(scattering, transmission, strict=True):
        if not all(
            math.isfinite(value) and 0 <= value <= 1 for value in (scatter, transmit)
        ):
            raise ValueError("water optical coefficients must be between zero and one")
        scattering_rate = -math.log(max(scatter, 0.01)) / scatter_distance
        absorption_rate = -math.log(max(transmit, 0.01)) / transmit_distance
        extinction = scattering_rate + absorption_rate
        if extinction == 0:
            raise ValueError("water colour has no scattering or absorption")
        albedo = scattering_rate / extinction
        attenuation = math.sqrt((1 - albedo) / (1 - 0.8 * albedo))
        result.append(
            5 * (1 - 0.139 * attenuation) * (1 - attenuation) / (1 + 1.17 * attenuation)
        )
    return [*result, 1.0]


def capture_selected_colours(
    planet: int,
    generator: int,
    read: Callable[[int, int], bytes] = memory.read_memory,
) -> dict[str, object]:
    def pointer(address: int) -> int:
        return memory.validate_pointer(
            "colour table", struct.unpack("<Q", read(address, 8))[0]
        )

    def record(header: int, index: int, stride: int) -> bytes:
        data, count = struct.unpack_from("<Qi", read(header, 16))
        if not 0 <= index < count <= 4096:
            raise ValueError(f"colour index {index} outside table of {count} records")
        memory.validate_pointer("colour records", data)
        return read(data + index * stride, stride)

    biome, subtype = struct.unpack("<2i", read(planet + 0x32B8, 8))
    if not 0 <= biome < 17:
        raise ValueError(f"invalid colour-table biome: {biome}")
    if subtype in (25, 26):
        biome = 12 if subtype == 25 else 13
    indices = struct.unpack("<3i", read(planet + 0x1E34, 12))
    sky = {}
    for period, offset, index in zip(
        ("day", "dusk", "night"), (0x518, 0x520, 0x528), indices, strict=True
    ):
        table = pointer(generator + offset)
        header = table + biome * 0x10
        count = struct.unpack_from("<i", read(header, 16), 8)[0]
        if count <= 0:
            header = table + 0x120
        sky[period] = record(header, index, 0xE0)
    water_index = struct.unpack("<i", read(planet + 0x3518, 4))[0]
    water = record(pointer(generator + 0x530), water_index, 0x80)
    colours = {
        name: list(struct.unpack_from("<4f", sky["day"], offset))
        for name, offset in SKY_COLOUR_FIELDS.items()
    }
    colours["SkySunset"] = list(struct.unpack_from("<4f", sky["dusk"], 0x70))
    colours["SkyNight"] = list(struct.unpack_from("<4f", sky["night"], 0x70))
    colours["Water"] = water_reflectance(water)
    return {
        "colours": colours,
        "sky_indices": dict(zip(("day", "dusk", "night"), indices, strict=True)),
        "water_index": water_index,
        "screen_filter": struct.unpack("<i", read(planet + 0x1E44, 4))[0],
    }
