use crate::vocabulary::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    System,
    Input,
    Query,
    Metadata,
    Weather,
    ScreenFilter,
    Colour,
    Objects,
}

macro_rules! fields {
    ($($name:ident => ($label:literal,$options:ident,$stage:ident)),+ $(,)?) => {
        #[derive(Debug,Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Hash,Serialize,Deserialize)]
        #[serde(rename_all="snake_case")]
        pub enum Field { $($name),+ }
        impl Field {
            pub const ALL: &[Self]=&[$(Self::$name),+];
            pub fn label(self) -> &'static str { match self { $(Self::$name=>$label),+ } }
            pub fn options(self) -> &'static [&'static str] { match self { $(Self::$name=>$options),+ } }
            pub fn stage(self) -> Stage { match self { $(Self::$name=>Stage::$stage),+ } }
        }
    }
}

fields! {
    Star => ("Star colour",STAR_TYPES,System),
    Wealth => ("Economy strength",WEALTH_CLASSES,System),
    Economy => ("Economy type",TRADING_CLASSES,System),
    Conflict => ("Conflict",CONFLICT_LEVELS,System),
    Population => ("Population",POPULATION_STATES,System),
    Race => ("Dominant lifeform",RACE_ENUM,System),
    Anomaly => ("Space anomaly",ANOMALIES,System),
    Size => ("Size",PLANET_SIZES,Input),
    Biome => ("Biome",BIOMES,Query),
    Subtype => ("Biome subtype",BIOME_SUBTYPES,Query),
    Terrain => ("Terrain",TERRAIN_SETTINGS,Query),
    Sentinels => ("Sentinels",SENTINEL_LEVELS,Query),
    Flora => ("Flora",PLANET_LIFE_LEVELS,Metadata),
    Fauna => ("Fauna",PLANET_LIFE_LEVELS,Metadata),
    Buildings => ("Building density",BUILDING_DENSITY_LEVELS,Metadata),
    ResourceRichness => ("Resource abundance",RESOURCE_LEVELS,Metadata),
    WeatherType => ("Weather type",WEATHER_TYPES,Weather),
    StormFrequency => ("Storm frequency",STORM_FREQUENCIES,Weather),
    WeatherIntensity => ("Weather intensity",WEATHER_INTENSITIES,Weather),
    Grass => ("Grass colour",GRASS_COLOURS,Objects),
    Plant => ("Plant colour",HUE_FAMILIES,Colour),
    Leaf => ("Leaf colour",HUE_FAMILIES,Colour),
    Cloud => ("Cloud colour",HUE_FAMILIES,Colour),
    SkyFog => ("Sky fog colour",HUE_FAMILIES,Colour),
    SkyHeightFog => ("Height fog colour",HUE_FAMILIES,Colour),
    Horizon => ("Horizon colour",HUE_FAMILIES,Colour),
    Sky => ("Daytime sky colour",HUE_FAMILIES,Colour),
    Sunset => ("Sunset colour",HUE_FAMILIES,Colour),
    NightSky => ("Night sky colour",HUE_FAMILIES,Colour),
    WaterColour => ("Water colour",HUE_FAMILIES,Colour),
    WorldColourFilter => ("World colour filter",WORLD_FILTERS,ScreenFilter),
}

impl Field {
    pub fn available_options(self) -> impl Iterator<Item = (usize, &'static str)> + Clone {
        self.options()
            .iter()
            .copied()
            .enumerate()
            .filter(move |(index, _)| match self {
                Self::Race => matches!(index, 0 | 1 | 2 | 7),
                Self::Anomaly => matches!(index, 0 | 2 | 5),
                Self::Biome => !(8..=11).contains(index),
                Self::Terrain => !(10..=20).contains(index),
                _ => true,
            })
    }

    pub fn colour_index(self) -> Option<usize> {
        [
            Self::Grass,
            Self::Plant,
            Self::Leaf,
            Self::Cloud,
            Self::SkyFog,
            Self::SkyHeightFog,
            Self::Horizon,
            Self::Sky,
            Self::Sunset,
            Self::NightSky,
            Self::WaterColour,
        ]
        .iter()
        .position(|field| *field == self)
    }

    pub fn native_value(self, choice: usize) -> Option<u32> {
        if choice >= self.options().len() {
            return None;
        }
        if self == Self::WorldColourFilter {
            return WORLD_FILTER_VALUES.get(choice).copied();
        }
        if self == Self::Biome && choice == 16 {
            return None;
        }
        Some(if self == Self::Anomaly && choice == 5 {
            0
        } else {
            choice as u32 + self.native_origin()
        })
    }

    fn choice_index(self, native_value: u32) -> Option<u32> {
        if self == Self::WorldColourFilter {
            WORLD_FILTER_VALUES
                .iter()
                .position(|&value| value == native_value)
                .map(|index| index as u32)
        } else if self == Self::Anomaly {
            match native_value {
                0 => Some(5),
                1..=5 => Some(native_value - 1),
                _ => None,
            }
        } else {
            native_value.checked_sub(self.native_origin())
        }
    }

    fn native_origin(self) -> u32 {
        u32::from(matches!(self, Self::Subtype | Self::Anomaly))
    }
}

macro_rules! flags {
    ($kind:ident {$($name:ident => ($label:literal,$stage:ident)),+ $(,)?}) => {
        #[derive(Debug,Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Hash,Serialize,Deserialize)]
        #[serde(rename_all="snake_case")]
        pub enum $kind { $($name),+ }
        impl $kind {
            pub const ALL: &[Self]=&[$(Self::$name),+];
            pub fn label(self) -> &'static str { match self { $(Self::$name=>$label),+ } }
            pub fn stage(self) -> Stage { match self { $(Self::$name=>Stage::$stage),+ } }
        }
    }
}

flags! { PlanetFlag {
    Rings => ("Rings",Input),
    Moons => ("Has moons",Input),
    Water => ("Water",Query),
    DeepWater => ("Deep water",Query),
    NonGasGiant => ("Giant rocky planet",Query),
    Sentinels => ("Sentinels",Query),
    ExtremeSentinels => ("Extreme sentinels",Query),
    CorruptSentinels => ("Corrupt sentinels",Query),
    ExtremeWeather => ("Extreme weather",Query),
    Infested => ("Infested",Query),
    Relic => ("Relic",Query),
    Rgb => ("Mega-exotic biome",Query),
    CreatureDiscovery => ("Suitable for creature discovery",Query),
    WeirdCreatureDiscovery => ("Suitable for unusual creature discovery",Query),
    CreatureTaming => ("Suitable for creature taming",Query),
    RobotCreatureDiscovery => ("Suitable for robotic creature discovery",Query),
    Scrap => ("Salvageable scrap",Metadata),
    Paradise => ("Paradise conditions",Weather),
    FloatingIslands => ("Floating islands",Objects),
    Dead => ("Dead",Query),
    Weird => ("Weird / Exotic",Query),
    GlowingGrass => ("Glowing grass",Objects),
    GlowingFlora => ("Glowing flora",Objects),
}}

flags! { SystemFlag {
    Pirate => ("Outlaw system",System),
    Giant => ("Contains a giant planet",Input),
    GasGiant => ("Contains a gas giant",Query),
    NonGasGiant => ("Contains a giant rocky planet",Query),
    Waterworld => ("Contains a waterworld",Query),
    Water => ("Contains water",Query),
    DeepWater => ("Contains deep water",Query),
    Weird => ("Contains a weird / exotic planet",Query),
    Infested => ("Contains an infested planet",Query),
    Relic => ("Contains a relic planet",Query),
    Rgb => ("Contains a mega-exotic biome",Query),
    CorruptSentinels => ("Contains corrupt sentinels",Query),
    ExtremeStorms => ("Contains extreme storms",Query),
    Dead => ("Contains a dead planet",Query),
}}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiantFilter {
    Any,
    AnyType,
    Gas,
    NonGas,
    Exclude,
}

impl GiantFilter {
    pub const ALL: &[Self] = &[
        Self::Any,
        Self::AnyType,
        Self::Gas,
        Self::NonGas,
        Self::Exclude,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::AnyType => "Require a giant",
            Self::Gas => "Require a gas giant",
            Self::NonGas => "Require a non-gas giant",
            Self::Exclude => "Exclude all giants",
        }
    }

    fn rule(self) -> Option<(SystemFlag, bool)> {
        match self {
            Self::Any => None,
            Self::AnyType => Some((SystemFlag::Giant, true)),
            Self::Gas => Some((SystemFlag::GasGiant, true)),
            Self::NonGas => Some((SystemFlag::NonGasGiant, true)),
            Self::Exclude => Some((SystemFlag::Giant, false)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceMode {
    #[default]
    All,
    Any,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    pub selected: u32,
    pub mode: ResourceMode,
}

impl Resources {
    pub fn accepts(self, actual: u32) -> bool {
        match self.mode {
            ResourceMode::All => actual & self.selected == self.selected,
            ResourceMode::Any => self.selected == 0 || actual & self.selected != 0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Criteria {
    #[serde(deserialize_with = "deserialize_choices")]
    pub choices: BTreeMap<Field, u64>,
    #[serde(deserialize_with = "deserialize_flags")]
    pub planets: BTreeMap<PlanetFlag, bool>,
    #[serde(deserialize_with = "deserialize_flags")]
    pub systems: BTreeMap<SystemFlag, bool>,
    pub resources: Resources,
    pub minimum_planets: Option<u8>,
}

fn deserialize_choices<'de, D>(deserializer: D) -> Result<BTreeMap<Field, u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut choices = BTreeMap::<String, u64>::deserialize(deserializer)?
        .into_iter()
        .filter(|(name, _)| !matches!(name.as_str(), "weather_conditions" | "system_class"))
        .map(|(name, value)| {
            Field::deserialize(serde::de::value::StringDeserializer::<D::Error>::new(name))
                .map(|field| (field, value))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    if let Some(mask) = choices.get_mut(&Field::Terrain) {
        *mask &= !(((1u64 << 11) - 1) << 10);
        if *mask == 0 {
            choices.remove(&Field::Terrain);
        }
    }
    Ok(choices)
}

fn deserialize_flags<'de, D, F>(deserializer: D) -> Result<BTreeMap<F, bool>, D::Error>
where
    D: serde::Deserializer<'de>,
    F: Deserialize<'de> + Ord,
{
    BTreeMap::<String, bool>::deserialize(deserializer)?
        .into_iter()
        .filter(|(name, _)| !matches!(name.as_str(), "normal" | "prime" | "extreme_hazard"))
        .map(|(name, value)| {
            F::deserialize(serde::de::value::StringDeserializer::<D::Error>::new(name))
                .map(|flag| (flag, value))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Choices {
    field: Field,
    mask: u64,
}

impl Choices {
    pub fn field(self) -> Field {
        self.field
    }
    pub fn mask(self) -> u64 {
        self.mask
    }
    pub fn accepts(self, native_value: u32) -> bool {
        if self.field == Field::Grass && native_value & GRASS_COLOUR_SET != 0 {
            let mask = self.mask as u32;
            return native_value & mask & !GRASS_COLOUR_SET != 0;
        }
        if self.field == Field::Biome && native_value >= 16 {
            return false;
        }
        if self.field == Field::Biome
            && (8..=10).contains(&native_value)
            && self.mask & (1 << 16) != 0
        {
            return true;
        }
        let Some(index) = self.field.choice_index(native_value) else {
            return false;
        };
        index < 64 && self.mask & (1u64 << index) != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Predicate {
    Choice(Choices),
    Planet(PlanetFlag, bool),
    System(SystemFlag, bool),
    Resources(Resources),
    MinimumPlanets(u8),
}

impl Predicate {
    pub fn stage(self) -> Stage {
        match self {
            Self::Choice(choices) => choices.field.stage(),
            Self::Planet(flag, _) => flag.stage(),
            Self::System(flag, _) => flag.stage(),
            Self::Resources(_) => Stage::Query,
            Self::MinimumPlanets(_) => Stage::System,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Requirements {
    pub planet_query: bool,
    pub metadata: bool,
    pub weather: bool,
    pub screen_filter: bool,
    pub colours: u16,
    pub objects: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub system: Vec<Predicate>,
    pub planet: Vec<Predicate>,
    pub requirements: Requirements,
}

impl Criteria {
    pub fn biome_choices(&self) -> Option<u64> {
        let selected = self.choices.get(&Field::Biome).copied().unwrap_or(0);
        if selected >> BIOMES.len() != 0 {
            return None;
        }
        let mega = 0b111 << 8;
        let mut accepted = selected & !(1 << 16);
        if selected & (1 << 16) != 0 {
            accepted |= mega;
        }
        if let Some(&required) = self.planets.get(&PlanetFlag::Rgb) {
            if selected == 0 {
                accepted = (1 << 16) - 1;
            }
            accepted &= if required { mega } else { !mega };
            if accepted == 0 {
                return None;
            }
        }
        if accepted & mega == mega {
            Some((accepted & !mega) | (1 << 16))
        } else {
            Some(accepted)
        }
    }

    pub fn set_biome_choices(&mut self, mask: u64) {
        self.planets.remove(&PlanetFlag::Rgb);
        if mask == 0 {
            self.choices.remove(&Field::Biome);
        } else {
            self.choices.insert(Field::Biome, mask);
        }
    }

    pub fn sentinel_choices(&self) -> Option<u64> {
        let selected = self.choices.get(&Field::Sentinels).copied().unwrap_or(0);
        if selected >> SENTINEL_LEVELS.len() != 0 {
            return None;
        }
        let mut accepted = if selected == 0 { 0b1111 } else { selected };
        let mut has_rule = selected != 0;
        for (flag, matching) in [
            (PlanetFlag::Sentinels, 0b1110),
            (PlanetFlag::ExtremeSentinels, 0b0100),
            (PlanetFlag::CorruptSentinels, 0b1000),
        ] {
            if let Some(required) = self.planets.get(&flag) {
                has_rule = true;
                accepted &= if *required {
                    matching
                } else {
                    0b1111 ^ matching
                };
            }
        }
        if accepted == 0 {
            None
        } else {
            Some(if has_rule { accepted } else { 0 })
        }
    }

    pub fn set_sentinel_choices(&mut self, mask: u64) {
        for flag in [
            PlanetFlag::Sentinels,
            PlanetFlag::ExtremeSentinels,
            PlanetFlag::CorruptSentinels,
        ] {
            self.planets.remove(&flag);
        }
        if mask == 0 {
            self.choices.remove(&Field::Sentinels);
        } else {
            self.choices.insert(Field::Sentinels, mask);
        }
    }

    pub fn giant_filter(&self) -> Option<GiantFilter> {
        let mut rules = self.systems.iter().filter_map(|(&flag, &value)| {
            matches!(
                flag,
                SystemFlag::Giant | SystemFlag::GasGiant | SystemFlag::NonGasGiant
            )
            .then_some((flag, value))
        });
        let rule = rules.next();
        if rules.next().is_some() {
            return None;
        }
        GiantFilter::ALL
            .iter()
            .copied()
            .find(|filter| filter.rule() == rule)
    }

    pub fn set_giant_filter(&mut self, filter: GiantFilter) {
        for flag in [
            SystemFlag::Giant,
            SystemFlag::GasGiant,
            SystemFlag::NonGasGiant,
        ] {
            self.systems.remove(&flag);
        }
        if let Some((flag, value)) = filter.rule() {
            self.systems.insert(flag, value);
        }
    }

    pub fn compile(&self) -> Result<Plan, &'static str> {
        let mut plan = Plan {
            system: Vec::new(),
            planet: Vec::new(),
            requirements: Requirements::default(),
        };
        for (&field, &mask) in &self.choices {
            if mask == 0 {
                continue;
            }
            if mask >> field.options().len() != 0 {
                return Err("unknown criterion choice");
            }
            let predicate = Predicate::Choice(Choices { field, mask });
            if field.stage() == Stage::System {
                plan.system.push(predicate);
            } else {
                plan.planet.push(predicate);
            }
            if let Some(index) = field.colour_index() {
                plan.requirements.colours |= 1 << index;
            }
            if field == Field::Economy {
                plan.system
                    .push(Predicate::System(SystemFlag::Pirate, false));
            }
            if field == Field::WaterColour {
                plan.planet.push(Predicate::Planet(PlanetFlag::Water, true));
            }
        }
        for (&flag, &value) in &self.planets {
            plan.planet.push(Predicate::Planet(flag, value));
        }
        for (&flag, &value) in &self.systems {
            plan.system.push(Predicate::System(flag, value));
        }
        if let Some(minimum) = self.minimum_planets {
            if !(1..=8).contains(&minimum) {
                return Err("planet count must be between one and eight");
            }
            plan.system.push(Predicate::MinimumPlanets(minimum));
        }
        if self.resources.selected >> RESOURCE_IDS.len() != 0 {
            return Err("unknown resource choice");
        }
        if self.resources.selected != 0 {
            if self.resources.mode == ResourceMode::All && self.resources.selected.count_ones() > 3
            {
                return Err(
                    "a planet has at most three listed resources; select up to three or use Any",
                );
            }
            plan.planet.push(Predicate::Resources(self.resources));
        }
        for predicate in plan.system.iter().chain(&plan.planet) {
            plan.requirements.planet_query |= predicate.stage() >= Stage::Query;
            plan.requirements.metadata |= predicate.stage() == Stage::Metadata
                || *predicate == Predicate::Planet(PlanetFlag::Paradise, true)
                || *predicate == Predicate::Planet(PlanetFlag::Paradise, false);
            plan.requirements.weather |= predicate.stage() == Stage::Weather;
            plan.requirements.screen_filter |= predicate.stage() == Stage::ScreenFilter;
            plan.requirements.objects |= predicate.stage() == Stage::Objects;
        }
        plan.system
            .sort_by_key(|predicate| (predicate.stage(), *predicate));
        plan.planet
            .sort_by_key(|predicate| (predicate.stage(), *predicate));
        plan.system.dedup();
        plan.planet.dedup();
        Ok(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glow_filters_are_independent_and_require_objects_without_colours() {
        use crate::facts::{Facts, Verdict};
        for flag in [PlanetFlag::GlowingGrass, PlanetFlag::GlowingFlora] {
            for required in [false, true] {
                let mut criteria = Criteria::default();
                criteria.planets.insert(flag, required);
                let plan = criteria.compile().unwrap();
                assert!(plan.requirements.objects);
                assert_eq!(plan.requirements.colours, 0);
                assert!(!plan.requirements.weather && !plan.requirements.metadata);
                let mut facts = Facts::default();
                assert_eq!(plan.planet_match(&facts), Verdict::Unknown);
                facts.set_planet_flag(flag, !required);
                assert_eq!(plan.planet_match(&facts), Verdict::NoMatch);
                facts.set_planet_flag(flag, required);
                assert_eq!(plan.planet_match(&facts), Verdict::Match);
                let encoded = serde_json::to_string(&criteria).unwrap();
                assert_eq!(
                    serde_json::from_str::<Criteria>(&encoded).unwrap(),
                    criteria
                );
            }
        }
        let mut criteria = Criteria::default();
        criteria.planets.insert(PlanetFlag::GlowingGrass, true);
        criteria.planets.insert(PlanetFlag::GlowingFlora, false);
        let plan = criteria.compile().unwrap();
        let mut facts = Facts::default();
        facts.set_planet_flag(PlanetFlag::GlowingGrass, true);
        assert_eq!(plan.planet_match(&facts), Verdict::Unknown);
        facts.set_planet_flag(PlanetFlag::GlowingFlora, false);
        assert_eq!(plan.planet_match(&facts), Verdict::Match);
    }

    #[test]
    fn grey_matches_only_grey_and_grassless_is_a_known_nonmatch() {
        let criteria: Criteria = serde_json::from_str(r#"{"choices":{"grass":256}}"#).unwrap();
        assert_eq!(criteria.choices[&Field::Grass], 1 << 8);
        let plan = criteria.compile().unwrap();
        assert!(plan.requirements.objects);
        assert_eq!(plan.requirements.colours, 1);
        for (colours, expected) in [
            (0, false),
            (1 << 3, false),
            (1 << 8, true),
            (1 << 9, false),
            (1 << 10, false),
        ] {
            let mut facts = crate::facts::Facts::default();
            facts.set(Field::Grass, GRASS_COLOUR_SET | colours);
            assert_eq!(
                plan.planet_match(&facts) == crate::facts::Verdict::Match,
                expected
            );
            assert!(plan.missing_planet_requirements(&facts) == Requirements::default());
        }
        assert_eq!(
            Field::Grass.available_options().skip(8).collect::<Vec<_>>(),
            [(8, "Grey"), (9, "Black"), (10, "White")]
        );
    }
    #[test]
    fn filtered_menus_keep_native_selection_indices() {
        assert_eq!(
            Field::Race.available_options().collect::<Vec<_>>(),
            [(0, "Gek"), (1, "Vy'keen"), (2, "Korvax"), (7, "None")]
        );
        assert_eq!(
            Field::Anomaly.available_options().collect::<Vec<_>>(),
            [(0, "Atlas station"), (2, "Black hole"), (5, "None")]
        );
        assert_eq!(
            Field::Biome.available_options().skip(8).collect::<Vec<_>>(),
            [
                (12, "Swamp"),
                (13, "Lava"),
                (14, "Waterworld"),
                (15, "GasGiant"),
                (16, "Mega-exotic")
            ]
        );
        assert_eq!(
            Field::Terrain
                .available_options()
                .map(|(index, _)| index)
                .collect::<Vec<_>>(),
            (0..10).chain(21..31).collect::<Vec<_>>()
        );
        for (mask, native) in [(1, 1), (2, 2), (4, 3), (8, 4), (16, 5), (32, 0)] {
            let criteria: Criteria =
                serde_json::from_str(&format!(r#"{{"choices":{{"anomaly":{mask}}}}}"#)).unwrap();
            let Predicate::Choice(rule) = criteria.compile().unwrap().system[0] else {
                panic!()
            };
            for actual in 0..7 {
                assert_eq!(rule.accepts(actual), actual == native);
            }
        }
    }

    #[test]
    fn sentinel_menu_represents_saved_combinations_without_broadening_them() {
        use crate::facts::{Facts, Verdict};
        for combination in 0..27 {
            for selected in 0..16 {
                let mut criteria = Criteria::default();
                criteria.choices.insert(Field::Sentinels, selected);
                let mut code = combination;
                for flag in [
                    PlanetFlag::Sentinels,
                    PlanetFlag::ExtremeSentinels,
                    PlanetFlag::CorruptSentinels,
                ] {
                    if code % 3 != 0 {
                        criteria.planets.insert(flag, code % 3 == 1);
                    }
                    code /= 3;
                }
                let plan = criteria.compile().unwrap();
                let displayed = criteria.sentinel_choices();
                let mut any_match = false;
                for level in 0..4 {
                    let mut facts = Facts::default();
                    facts.set(Field::Sentinels, level);
                    facts.set_planet_flag(PlanetFlag::Sentinels, level != 0);
                    facts.set_planet_flag(PlanetFlag::ExtremeSentinels, level == 2);
                    facts.set_planet_flag(PlanetFlag::CorruptSentinels, level == 3);
                    let matched = plan.planet_match(&facts) == Verdict::Match;
                    any_match |= matched;
                    assert_eq!(
                        matched,
                        displayed.is_some_and(|mask| mask == 0 || mask & (1 << level) != 0)
                    );
                }
                assert_eq!(displayed.is_some(), any_match);
            }
        }
    }

    #[test]
    fn editing_sentinels_replaces_legacy_rules_and_preserves_other_filters() {
        let mut criteria: Criteria = serde_json::from_str(
            r#"{"choices":{"biome":1},"planets":{"sentinels":false,"extreme_sentinels":true,"water":true}}"#
        ).unwrap();
        assert_eq!(criteria.sentinel_choices(), None);
        criteria.set_sentinel_choices(1 << 3);
        assert_eq!(criteria.sentinel_choices(), Some(1 << 3));
        assert_eq!(
            criteria.planets,
            BTreeMap::from([(PlanetFlag::Water, true)])
        );
        assert_eq!(criteria.choices.get(&Field::Biome), Some(&1));
        let plan = criteria.compile().unwrap();
        assert!(plan.requirements.planet_query);
        assert!(!plan.requirements.metadata);
        criteria.set_sentinel_choices(0);
        assert_eq!(criteria.sentinel_choices(), Some(0));
        assert!(!criteria.choices.contains_key(&Field::Sentinels));
    }

    #[test]
    fn retired_filters_are_removed_without_dropping_supported_saved_criteria() {
        let criteria: Criteria = serde_json::from_str(
            r#"{
                "choices":{"weather_conditions":7,"system_class":2,"terrain":1075838976,"biome":1,"storm_frequency":4,"weather_intensity":2,"weather_type":8},
                "planets":{"normal":true,"prime":false,"extreme_hazard":true,"infested":false,"extreme_weather":true},
                "systems":{"normal":false,"water":true}
            }"#
        ).unwrap();
        assert_eq!(
            criteria.choices,
            BTreeMap::from([
                (Field::Terrain, 1075838976),
                (Field::Biome, 1),
                (Field::StormFrequency, 4),
                (Field::WeatherIntensity, 2),
                (Field::WeatherType, 8),
            ])
        );
        assert_eq!(
            criteria.planets,
            BTreeMap::from([
                (PlanetFlag::Infested, false),
                (PlanetFlag::ExtremeWeather, true)
            ])
        );
        assert_eq!(
            criteria.systems,
            BTreeMap::from([(SystemFlag::Water, true)])
        );
        criteria.compile().unwrap();
        for (mask, expected) in [
            (1024, None),
            (1048576, None),
            (1073743872, Some(1073741824)),
            (2097664, Some(2097664)),
        ] {
            let criteria: Criteria =
                serde_json::from_str(&format!(r#"{{"choices":{{"terrain":{mask}}}}}"#)).unwrap();
            assert_eq!(criteria.choices.get(&Field::Terrain).copied(), expected);
        }
        for invalid in [
            r#"{"choices":{"unknown":1}}"#,
            r#"{"planets":{"unknown":true}}"#,
            r#"{"systems":{"unknown":true}}"#,
        ] {
            assert!(serde_json::from_str::<Criteria>(invalid).is_err());
        }
    }

    #[test]
    fn giant_dropdown_replaces_all_giant_rules_and_preserves_other_criteria() {
        use crate::facts::{Facts, Verdict};

        let mut criteria = Criteria::default();
        criteria.systems.insert(SystemFlag::Water, true);
        criteria.planets.insert(PlanetFlag::NonGasGiant, false);
        criteria.systems.insert(SystemFlag::GasGiant, true);
        criteria.systems.insert(SystemFlag::NonGasGiant, true);
        assert_eq!(criteria.giant_filter(), None);
        for (filter, expected) in [
            (GiantFilter::Any, [true, true, true, true]),
            (GiantFilter::AnyType, [false, true, true, true]),
            (GiantFilter::Gas, [false, true, false, true]),
            (GiantFilter::NonGas, [false, false, true, true]),
            (GiantFilter::Exclude, [true, false, false, false]),
        ] {
            criteria.set_giant_filter(filter);
            assert_eq!(criteria.giant_filter(), Some(filter));
            assert_eq!(criteria.systems.get(&SystemFlag::Water), Some(&true));
            assert_eq!(criteria.planets.get(&PlanetFlag::NonGasGiant), Some(&false));
            let plan = criteria.compile().unwrap();
            for ((gas, non_gas), accepted) in
                [(false, false), (true, false), (false, true), (true, true)]
                    .into_iter()
                    .zip(expected)
            {
                let mut facts = Facts::default();
                facts.set_system_flag(SystemFlag::Water, true);
                facts.set_system_flag(SystemFlag::Giant, gas || non_gas);
                facts.set_system_flag(SystemFlag::GasGiant, gas);
                facts.set_system_flag(SystemFlag::NonGasGiant, non_gas);
                assert_eq!(
                    plan.system_match(&facts),
                    if accepted {
                        Verdict::Match
                    } else {
                        Verdict::NoMatch
                    }
                );
            }
        }
        assert_eq!(criteria.systems.len(), 2);
        criteria.set_giant_filter(GiantFilter::Any);
        assert_eq!(criteria.systems.len(), 1);
    }

    #[test]
    fn any_produces_no_predicate_or_acquisition() {
        let mut criteria = Criteria::default();
        for &field in Field::ALL {
            criteria.choices.insert(field, 0);
        }
        let plan = criteria.compile().unwrap();
        assert!(plan.system.is_empty() && plan.planet.is_empty());
        assert_eq!(plan.requirements, Requirements::default());
    }

    #[test]
    fn sizes_allow_every_size_except_moon() {
        let mut criteria = Criteria::default();
        criteria.choices.insert(Field::Size, 0b10111);
        let plan = criteria.compile().unwrap();
        let Predicate::Choice(choices) = plan.planet[0] else {
            panic!()
        };
        for size in 0..5 {
            assert_eq!(choices.accepts(size), size != 3);
        }
        assert!(!choices.accepts(5));
        assert!(!plan.requirements.planet_query);
    }

    #[test]
    fn mega_exotic_groups_native_biomes_and_preserves_saved_restrictions() {
        for (json, accepted) in [
            (r#"{"choices":{"biome":65536}}"#, vec![8, 9, 10]),
            (r#"{"choices":{"biome":1792}}"#, vec![8, 9, 10]),
            (r#"{"planets":{"rgb":true}}"#, vec![8, 9, 10]),
            (
                r#"{"choices":{"biome":512},"planets":{"rgb":true}}"#,
                vec![9],
            ),
            (
                r#"{"planets":{"rgb":false}}"#,
                vec![0, 1, 2, 3, 4, 5, 6, 7, 11, 12, 13, 14, 15],
            ),
        ] {
            let criteria: Criteria = serde_json::from_str(json).unwrap();
            let displayed = criteria.biome_choices().unwrap();
            if accepted == [8, 9, 10] {
                assert_eq!(displayed, 1 << 16);
            }
            let plan = criteria.compile().unwrap();
            let mut edited = criteria.clone();
            edited.set_biome_choices(displayed);
            let edited_plan = edited.compile().unwrap();
            assert!(!edited.planets.contains_key(&PlanetFlag::Rgb));
            for native in 0..16 {
                let mut facts = crate::facts::Facts::default();
                facts.set(Field::Biome, native);
                facts.set_planet_flag(PlanetFlag::Rgb, (8..=10).contains(&native));
                assert_eq!(
                    plan.planet_match(&facts) == crate::facts::Verdict::Match,
                    accepted.contains(&native)
                );
                assert_eq!(edited_plan.planet_match(&facts), plan.planet_match(&facts));
            }
        }
        let mut conflicting: Criteria =
            serde_json::from_str(r#"{"choices":{"biome":1},"planets":{"rgb":true,"water":true}}"#)
                .unwrap();
        assert_eq!(conflicting.biome_choices(), None);
        conflicting.set_biome_choices(0);
        assert_eq!(conflicting.biome_choices(), Some(0));
        assert_eq!(
            conflicting.planets,
            BTreeMap::from([(PlanetFlag::Water, true)])
        );
    }

    #[test]
    fn world_filter_masks_use_generated_values_and_acquire_only_selected_data() {
        let mut criteria = Criteria::default();
        criteria
            .choices
            .insert(Field::WorldColourFilter, (1 << 8) | (1 << 36) | (1 << 41));
        let plan = criteria.compile().unwrap();
        assert!(plan.requirements.screen_filter);
        assert!(!plan.requirements.weather);
        assert_eq!(plan.requirements.colours, 0);
        let mut facts = crate::facts::Facts::default();
        assert!(plan.missing_planet_requirements(&facts).screen_filter);
        for native in 0..85 {
            facts.set(Field::WorldColourFilter, native);
            assert_eq!(
                plan.planet_match(&facts) == crate::facts::Verdict::Match,
                [13, 78, 83].contains(&native)
            );
            assert!(!plan.missing_planet_requirements(&facts).screen_filter);
        }
        criteria
            .choices
            .insert(Field::WorldColourFilter, 1 << WORLD_FILTERS.len());
        assert!(criteria.compile().is_err());
        criteria.choices.insert(Field::WorldColourFilter, 0);
        assert!(!criteria.compile().unwrap().requirements.screen_filter);
        assert_eq!(Field::WorldColourFilter.native_value(36), Some(78));
        assert_eq!(WORLD_FILTER_VALUES.len(), WORLD_FILTERS.len());
    }

    #[test]
    fn acquisitions_follow_selected_fields() {
        let mut criteria = Criteria::default();
        criteria.choices.insert(Field::Sky, 1 << 5);
        let plan = criteria.compile().unwrap();
        assert_eq!(
            plan.requirements,
            Requirements {
                planet_query: true,
                colours: 1 << 7,
                ..Requirements::default()
            }
        );
        criteria.planets.insert(PlanetFlag::Paradise, false);
        let plan = criteria.compile().unwrap();
        assert!(plan.requirements.metadata && plan.requirements.weather);
        assert!(!plan.requirements.objects);
    }

    #[test]
    fn resources_share_one_control_with_explicit_conjunction() {
        assert!(
            Resources {
                selected: 0b11,
                mode: ResourceMode::All
            }
            .accepts(0b111)
        );
        assert!(
            !Resources {
                selected: 0b11,
                mode: ResourceMode::All
            }
            .accepts(0b10)
        );
        assert!(
            Resources {
                selected: 0b11,
                mode: ResourceMode::Any
            }
            .accepts(0b10)
        );
        let criteria = Criteria {
            resources: Resources {
                selected: 15,
                mode: ResourceMode::All,
            },
            ..Criteria::default()
        };
        assert!(criteria.compile().is_err());
    }

    #[test]
    fn native_subtypes_are_one_based_and_races_keep_native_indices() {
        assert!(
            Choices {
                field: Field::Subtype,
                mask: 1
            }
            .accepts(1)
        );
        assert!(
            !Choices {
                field: Field::Subtype,
                mask: 1
            }
            .accepts(0)
        );
        assert_eq!(Field::Race.options()[8], "Builders");
        assert_eq!(Field::Race.options()[7], "None");
    }

    #[test]
    fn criteria_round_trip_and_reject_unknown_fields() {
        let mut criteria = Criteria::default();
        criteria.choices.insert(Field::Biome, 1);
        criteria.planets.insert(PlanetFlag::FloatingIslands, true);
        let text = serde_json::to_string(&criteria).unwrap();
        assert_eq!(serde_json::from_str::<Criteria>(&text).unwrap(), criteria);
        assert!(serde_json::from_str::<Criteria>(r#"{"unknown":true}"#).is_err());
    }
}
