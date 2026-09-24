import ctypes

from search_probes.native_layout import (
    ALLOCATOR_DEALLOCATE,
    CANDIDATE_EVALUATOR,
    COORDINATE_TO_GRAPH,
    NEAREST_SYSTEM_RESOLVER,
    OBJECT_LIST_ELEMENT_DESTROY_RANGE,
    OBJECT_LIST_RESOLVER,
    PLANET_DATA_DESTRUCTOR,
    PUBLISH_SCAN_EVENT,
    REMOVE_SCAN_EVENT,
    SCAN_EVENT_LOOKUP,
    SOLAR_QUERY_CONSTRUCTOR,
    SOLAR_QUERY_DESTRUCTOR,
    SOLAR_QUERY_RUN,
    SPATIAL_NEAREST,
    SYSTEM_NAME_GENERATOR,
)


class NativeCalls:
    def __init__(self, resolve):
        expected_prefixes = {
            SPATIAL_NEAREST: bytes.fromhex(
                "40 53 48 83 EC 60 F3 41 0F 10 61 04 4C 8D 5C 24 40 F3 0F 58 25 7F 22 77 04 45 33 D2 C7 44 24 40"
            ),
            COORDINATE_TO_GRAPH: bytes.fromhex(
                "40 55 56 57 41 55 41 57 48 81 EC C0 00 00 00 4D 8B E9 41 0F B7 F8 48 8B F2 4C 8B F9 66 45 85 C0"
            ),
            SCAN_EVENT_LOOKUP: bytes.fromhex(
                "48 89 5C 24 08 49 63 D8 4C 8B D1 83 FB 03 75 5B 8B 41 7C 85 C0 74 54 4C 8B 89 80 00 00 00 4D 8D"
            ),
            NEAREST_SYSTEM_RESOLVER: bytes.fromhex(
                "4C 89 4C 24 20 44 88 44 24 18 48 89 54 24 10 55 53 56 57 41 54 41 55 41 56 41 57 48 8D AC 24 48"
            ),
            CANDIDATE_EVALUATOR: bytes.fromhex(
                "44 88 4C 24 20 48 89 54 24 10 48 89 4C 24 08 55 53 56 57 41 54 41 55 48 8D AC 24 B8 C8 FF FF B8"
            ),
            SOLAR_QUERY_CONSTRUCTOR: bytes.fromhex(
                "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 56 48 83 EC 30 45 33 F6 48 C7 41 08 FF FF FF FF 4C 89 71 10 48 8B F9 44 88 71 18 48 83 C1 20 0F 29 74 24 20 E8 52 FC FF FF 44 88"
            ),
            SOLAR_QUERY_RUN: bytes.fromhex(
                "44 88 4C 24 20 53 56 57 41 54 41 55 41 56 41 57 48 83 EC 50 4C 8B F2 44 89 01 49 C1 EE 20 4C 8B"
            ),
            SOLAR_QUERY_DESTRUCTOR: bytes.fromhex(
                "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8B D9 48 81 C1 A0 36 00 00 E8 42 73 EB FF 48 8D"
            ),
            PLANET_DATA_DESTRUCTOR: bytes.fromhex(
                "48 89 5C 24 08 57 48 83 EC 20 48 8B 91 20 34 00 00 48 8B F9 48 85 D2 74 1B 80 B9 2C 34 00 00 00"
            ),
            SYSTEM_NAME_GENERATOR: bytes.fromhex(
                "4C 89 4C 24 20 4C 89 44 24 18 48 89 4C 24 08 55 53 56 57 41 54 41 55 41 56 41 57 48 8D AC 24 B8"
            ),
            OBJECT_LIST_RESOLVER: bytes.fromhex(
                "48 8B C4 55 48 8D 68 B1 48 81 EC C0 00 00 00 48 89 58 F0 48 89 70 E8 48 8B F1 48 89 78 E0 4C 89"
            ),
            OBJECT_LIST_ELEMENT_DESTROY_RANGE: bytes.fromhex(
                "48 3B CA 73 51 48 89 5C 24 10 48 89 6C 24 18 56 48 83 EC 20 49 8B F0 48 89 7C 24 30 48 8B EA 48"
            ),
            ALLOCATOR_DEALLOCATE: bytes.fromhex(
                "48 8D 0D 49 8E 40 02 E9 54 4C 02 00"
            ),
            PUBLISH_SCAN_EVENT: bytes.fromhex(
                "48 8B C4 55 48 8D A8 28 FF FF FF 48 81 EC D0 01 00 00 66 0F 6F 05 16 94 84 03 0F 57 C9 48 89 58"
            ),
            REMOVE_SCAN_EVENT: bytes.fromhex(
                "48 83 EC 58 48 89 5C 24 60 48 89 6C 24 68 48 8B E9 48 89 74 24 70 48 89 7C 24 50 33 FF 4C 89 74"
            ),
        }
        for preferred, expected in expected_prefixes.items():
            actual = ctypes.string_at(resolve(preferred), len(expected))
            if actual != expected:
                raise RuntimeError(
                    f"current executable does not match 0x{preferred:X}: "
                    f"expected {expected.hex()}, found {actual.hex()}"
                )

        lookup_signature = ctypes.WINFUNCTYPE(
            ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int32
        )
        spatial_signature = ctypes.WINFUNCTYPE(
            None, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p
        )
        coordinate_signature = ctypes.WINFUNCTYPE(
            ctypes.c_int32,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_uint16,
            ctypes.c_void_p,
        )
        resolver_signature = ctypes.WINFUNCTYPE(
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_bool,
            ctypes.c_void_p,
            ctypes.c_bool,
            ctypes.c_bool,
        )
        candidate_signature = ctypes.WINFUNCTYPE(
            ctypes.c_bool,
            ctypes.c_void_p,
            ctypes.c_uint64,
            ctypes.c_bool,
            ctypes.c_bool,
            ctypes.c_void_p,
            ctypes.c_bool,
            ctypes.c_bool,
            ctypes.c_bool,
        )
        query_lifecycle_signature = ctypes.WINFUNCTYPE(None, ctypes.c_void_p)
        query_run_signature = ctypes.WINFUNCTYPE(
            None,
            ctypes.c_void_p,
            ctypes.c_uint64,
            ctypes.c_int32,
            ctypes.c_bool,
        )
        name_generator_signature = ctypes.WINFUNCTYPE(
            None,
            ctypes.c_void_p,
            ctypes.c_uint64,
            ctypes.c_void_p,
            ctypes.c_void_p,
        )
        publish_signature = ctypes.WINFUNCTYPE(
            None,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_int32,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_uint64,
        )
        remove_scan_event_signature = ctypes.WINFUNCTYPE(
            ctypes.c_bool,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_void_p,
        )
        object_list_resolver_signature = ctypes.WINFUNCTYPE(
            None,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_void_p,
        )
        destroy_range_signature = ctypes.WINFUNCTYPE(
            None, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p
        )
        deallocate_signature = ctypes.WINFUNCTYPE(
            None, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int32
        )
        self.lookup_event = lookup_signature(resolve(SCAN_EVENT_LOOKUP))
        self.spatial_nearest = spatial_signature(resolve(SPATIAL_NEAREST))
        self.coordinate_to_graph = coordinate_signature(resolve(COORDINATE_TO_GRAPH))
        self.find_nearest = resolver_signature(resolve(NEAREST_SYSTEM_RESOLVER))
        self.evaluate_candidate = candidate_signature(resolve(CANDIDATE_EVALUATOR))
        self.construct_solar_query = query_lifecycle_signature(
            resolve(SOLAR_QUERY_CONSTRUCTOR)
        )
        self.run_solar_query = query_run_signature(resolve(SOLAR_QUERY_RUN))
        self.destroy_solar_query = query_lifecycle_signature(
            resolve(SOLAR_QUERY_DESTRUCTOR)
        )
        self.generate_system_name = name_generator_signature(
            resolve(SYSTEM_NAME_GENERATOR)
        )
        self.resolve_object_lists = object_list_resolver_signature(
            resolve(OBJECT_LIST_RESOLVER)
        )
        self.destroy_object_list_elements = destroy_range_signature(
            resolve(OBJECT_LIST_ELEMENT_DESTROY_RANGE)
        )
        self.deallocate = deallocate_signature(resolve(ALLOCATOR_DEALLOCATE))
        self.publish_scan_event = publish_signature(resolve(PUBLISH_SCAN_EVENT))
        self.remove_scan_event = remove_scan_event_signature(resolve(REMOVE_SCAN_EVENT))
