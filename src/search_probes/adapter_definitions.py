from __future__ import annotations

SOURCE_MISSION_ID = "WIKI_TRADE5"
REFERENCE_SCAN_EVENT_ID = "SE_PHOTO_BIOME_LUSH"
MISSION_ID = "SQN_SS_LUSH"
SCAN_EVENT_ID = "SE_SQN_SS_LUSH"
MAX_PROBE_TAG_LENGTH = 12
MAX_GUIDE_PRESET_SLOTS = 9
# Slot identities are versioned independently from user-facing preset names. Persisted mission
# progress from an older navigation contract must not resume midway through a replacement mission.
GUIDE_MISSION_PREFIX = "SQN_SS9_P"
GUIDE_REQUEST_EVENT_PREFIX = "SE_SQN_SS9_P"
GUIDE_READY_EVENT_PREFIX = "SE_SQN_SS9_R"
GUIDE_TARGET_EVENT_PREFIX = "SE_SQN_SS9_T"
FORM_NAVIGATION_TARGET_MISSION_ID = "SQN_SS11_NAV"
FORM_NAVIGATION_TARGET_EVENT_ID = "SE_SQN_SS11_TGT"
FROM_LIST_PLACEHOLDER = "0000000000000000"
CATEGORY_ICON_ON = "TEXTURES/UI/FRONTEND/ICONS/MISSIONS/MISSION.BLACKHOLE.ON.DDS"
CATEGORY_ICON_OFF = "TEXTURES/UI/FRONTEND/ICONS/MISSIONS/MISSION.BLACKHOLE.OFF.DDS"
PRESET_TOPIC_ICON = "TEXTURES/UI/HUD/ICONS/WIKI/EXPLORATION4.DDS"
PRESET_NOTIFY_ICON = "TEXTURES/UI/HUD/ICONS/WIKI/EXPLORATION2.DDS"

LANGUAGES = (
    "BrazilianPortuguese",
    "Dutch",
    "English",
    "French",
    "German",
    "Italian",
    "Japanese",
    "Korean",
    "LatinAmericanSpanish",
    "Polish",
    "Portuguese",
    "Russian",
    "SimplifiedChinese",
    "Spanish",
    "TencentChinese",
    "TraditionalChinese",
    "USEnglish",
)

LOOKUP_BOOLEAN_FIELDS = (
    "UseStarType",
    "UseWealth",
    "UseTrading",
    "AllowUnsafeMatches",
    "NeverAllowEmpty",
    "NeverAllowAbandoned",
    "AllowedToBePurpleWithoutAccess",
    "RequireUndiscovered",
    "AlwaysAvailableInPirateStations",
    "NeedsWaterPlanet",
    "NeedsDeepWaterPlanet",
    "NeedsPrimePlanet",
    "NeedsSentinels",
    "NeedsCorruptSentinelPlanet",
    "NeedsExtremeSentinelPlanet",
    "NeverAllowExtremeSentinelPlanet",
    "NeedsExtremeWeatherPlanet",
    "NeedsExtremeHazardPlanet",
    "NeverAllowExtremeWeatherPlanet",
    "AnyBiomeNotWeirdOrDead",
    "AnyRGBBiome",
    "AnyInfestedBiome",
    "NeedsBiome",
    "NeedsEmptySystem",
    "NeedsAbandonedSystem",
    "SuitableForCreatureDiscovery",
    "SuitableForWeirdCreatureDiscovery",
    "SuitableForRobotCreatureDiscovery",
    "SuitableForCreatureTaming",
    "SystemNeedsWater",
    "SystemNeedsWeirdPlanet",
    "SystemNeedsInfestedPlanet",
    "SystemNeedsRelicPlanet",
    "SystemNeedsCorruptSentinelPlanet",
    "SystemNeedsExtremeStormPlanet",
    "SystemNeedsGasGiant",
    "SystemNeedsNonGasGiant",
    "NeverAllowGasGiantSystem",
)

LOCALISATION = {
    "SQN_SS_CATEGORY": "Search Probes",
    "SQN_SS_CATEGORY_U": "SEARCH PROBES",
    "SQN_SS_TOPIC": "Launch Saved Search Probe",
    "SQN_SS_SHORT": "Launch this saved search probe.",
    "SQN_SS_BODY": (
        "Launch a probe into nearby generated systems using this preset's conditions. The closest "
        "match will become an ordinary Galaxy Map destination."
    ),
    "SQN_SS_BUTTON": "Launch Search Probe",
    "SQN_SS_LUSH_MISSION_TITLE": "Search Probe Target",
    "SQN_SS_LUSH_MISSION_DESC": "A search probe has identified a navigation target.",
    "SQN_SS_LUSH_MISSION_FLAVOUR": "Navigate to the system identified by the search probe.",
    "SQN_SS_LUSH_MISSION_OBJ": "Approach the search probe target planet",
    "SQN_SS_LUSH_MISSION_OBJ_TIP": (
        "Follow Current Mission in the Galaxy Map and approach the marked planet."
    ),
    "SQN_SS_LUSH_MISSION_MSG": "Search probe navigation active",
    "SQN_SS_LUSH_SYSTEM_MSG": "Travel to the search probe target system",
    "SQN_SS_LUSH_PLANET_MSG": "Approach the search probe target planet",
    "SQN_SS_LUSH_OSD": "Search probe target selected",
    "SQN_SS_LUSH_MARKER": "Search probe target",
}
