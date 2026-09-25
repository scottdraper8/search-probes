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
    SystemClass => ("System class",SYSTEM_CLASSES,System),
    Anomaly => ("Space anomaly",ANOMALIES,System),
    Size => ("Planet size",PLANET_SIZES,Input),
    Biome => ("Biome",BIOMES,Query),
    Subtype => ("Biome subtype",BIOME_SUBTYPES,Query),
    Terrain => ("Terrain",TERRAIN_SETTINGS,Query),
    Flora => ("Flora",PLANET_LIFE_LEVELS,Metadata),
    Fauna => ("Fauna",PLANET_LIFE_LEVELS,Metadata),
    Buildings => ("Building density",BUILDING_DENSITY_LEVELS,Metadata),
    ResourceRichness => ("Resource abundance",RESOURCE_LEVELS,Metadata),
    WeatherConditions => ("Storm conditions",WEATHER_CONDITIONS,Weather),
    WeatherType => ("Weather type",WEATHER_TYPES,Weather),
    StormFrequency => ("Storm frequency",STORM_FREQUENCIES,Weather),
    WeatherIntensity => ("Weather intensity",WEATHER_INTENSITIES,Weather),
    Grass => ("Grass colour",HUE_FAMILIES,Colour),
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
}

impl Field {
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
        (choice < self.options().len()).then_some(choice as u32 + self.native_origin())
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
    ExtremeHazard => ("Extreme environmental hazard",Query),
    Prime => ("Prime planet",Query),
    Infested => ("Infested",Query),
    Normal => ("Normal planet",Query),
    Relic => ("Relic planet",Query),
    Rgb => ("Chromatic biome",Query),
    CreatureDiscovery => ("Suitable for creature discovery",Query),
    WeirdCreatureDiscovery => ("Suitable for unusual creature discovery",Query),
    CreatureTaming => ("Suitable for creature taming",Query),
    RobotCreatureDiscovery => ("Suitable for robotic creature discovery",Query),
    Scrap => ("Salvageable scrap",Metadata),
    Paradise => ("Paradise conditions",Weather),
    FloatingIslands => ("Floating islands",Objects),
}}

flags! { SystemFlag {
    Pirate => ("Outlaw system",System),
    Giant => ("Contains a giant planet",Input),
    GasGiant => ("Contains a gas giant",Query),
    NonGasGiant => ("Contains a giant rocky planet",Query),
    Waterworld => ("Contains a waterworld",Query),
    Water => ("Contains water",Query),
    DeepWater => ("Contains deep water",Query),
    Weird => ("Contains an exotic biome",Query),
    Infested => ("Contains an infested planet",Query),
    Normal => ("Contains a normal planet",Query),
    Relic => ("Contains a relic planet",Query),
    Rgb => ("Contains a chromatic biome",Query),
    CorruptSentinels => ("Contains corrupt sentinels",Query),
    ExtremeStorms => ("Contains extreme storms",Query),
}}

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
    pub choices: BTreeMap<Field, u64>,
    pub planets: BTreeMap<PlanetFlag, bool>,
    pub systems: BTreeMap<SystemFlag, bool>,
    pub resources: Resources,
    pub minimum_planets: Option<u8>,
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
        let Some(index) = native_value.checked_sub(self.field.native_origin()) else {
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
