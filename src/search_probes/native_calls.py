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
                "40 53 48 83 EC 60 F3 41 0F 10 61 04 4C 8D 5C 24 40"
            ),
            COORDINATE_TO_GRAPH: bytes.fromhex(
                "40 55 56 57 41 55 41 57 48 81 EC C0 00 00 00 4D 8B E9 41 0F B7 F8"
            ),
            SCAN_EVENT_LOOKUP: bytes.fromhex(
                "48 89 5C 24 08 49 63 D8 4C 8B D1 83 FB 03"
            ),
            NEAREST_SYSTEM_RESOLVER: bytes.fromhex(
                "4C 89 4C 24 20 44 88 44 24 18 48 89 54 24 10 55 53 56 57 41 54"
            ),
            CANDIDATE_EVALUATOR: bytes.fromhex(
                "44 88 4C 24 20 48 89 54 24 10 48 89 4C 24 08 55 53 56 57 41 54"
            ),
            SOLAR_QUERY_CONSTRUCTOR: bytes.fromhex(
                "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 56"
            ),
            SOLAR_QUERY_RUN: bytes.fromhex("44 88 4C 24 20 53 56 57"),
            SOLAR_QUERY_DESTRUCTOR: bytes.fromhex(
                "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8B D9"
            ),
            PLANET_DATA_DESTRUCTOR: bytes.fromhex(
                "48 89 5C 24 08 57 48 83 EC 20 48 8B 91 20 34 00 00"
            ),
            SYSTEM_NAME_GENERATOR: bytes.fromhex(
                "48 89 5C 24 18 4C 89 4C 24 20 55 56 57 41 54 41 55 41 56 41 57"
            ),
            OBJECT_LIST_RESOLVER: bytes.fromhex(
                "48 8B C4 55 48 8D 68 B1 48 81 EC C0 00 00 00"
            ),
            OBJECT_LIST_ELEMENT_DESTROY_RANGE: bytes.fromhex(
                "48 3B CA 73 51 48 89 5C 24 10 48 89 6C 24 18 56"
            ),
            ALLOCATOR_DEALLOCATE: bytes.fromhex("48 8D 0D 09 B8 40 02 E9 54 4C 02 00"),
            PUBLISH_SCAN_EVENT: bytes.fromhex(
                "48 8B C4 55 48 8D A8 28 FF FF FF 48 81 EC D0 01 00 00"
            ),
            REMOVE_SCAN_EVENT: bytes.fromhex(
                "48 83 EC 58 48 89 5C 24 60 48 89 6C 24 68 48 8B E9"
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
