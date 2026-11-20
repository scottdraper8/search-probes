use crate::{
    terrain::{FEATURE_SET, FLOATING_ISLANDS, TERRAIN_FEATURES},
    vocabulary::*,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    System,
    Input,
    Query,
    Hints,
    Fauna,
    Metadata,
    Weather,
    Terrain,
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
    ConflictLevel => ("Conflict level",CONFLICT_LEVELS,System),
    Population => ("Population",POPULATION_STATES,System),
    Race => ("Dominant lifeform",RACE_ENUM,System),
    Anomaly => ("Space anomaly",ANOMALIES,System),
    Size => ("Size",PLANET_SIZES,Input),
    Biome => ("Biome",BIOMES,Query),
    Subtype => ("Biome subtype",BIOME_SUBTYPES,Query),
    Terrain => ("Terrain",TERRAIN_FEATURES,Terrain),
    Sentinels => ("Sentinels",SENTINEL_LEVELS,Query),
    Flora => ("Biodiversity",PLANET_LIFE_LEVELS,Metadata),
    Fauna => ("Biodiversity",PLANET_LIFE_LEVELS,Metadata),
    ResourceRichness => ("Resource abundance",RESOURCE_LEVELS,Metadata),
    StormType => ("Storm type",STORM_TYPES,Weather),
    StormFrequency => ("Storm frequency",STORM_FREQUENCIES,Query),
    Grass => ("Grass colour",GRASS_COLOURS,Objects),
    FloraColour => ("Non-grass flora colour",GRASS_COLOURS,Objects),
    Horizon => ("Horizon tint",GRASS_COLOURS,Colour),
    Sky => ("Daytime sky colour",GRASS_COLOURS,Colour),
    WaterColour => ("Water colour",GRASS_COLOURS,Colour),
    WorldColourFilter => ("World colour filter",WORLD_FILTERS,ScreenFilter),
    SpecialtyResources => ("Specialty resources",SPECIALTY_RESOURCES,Hints),
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
                Self::Subtype => *index < 29,
                Self::Flora | Self::Fauna => *index != 1,
                Self::Sky | Self::Horizon => matches!(index, 0..=8 | 10),
                Self::WaterColour => matches!(index, 0 | 1 | 3..=7),

                _ => true,
            })
    }

    pub fn colour_index(self) -> Option<usize> {
        match self {
            Self::Grass => Some(0),
            Self::FloraColour => Some(1),
            Self::Horizon => Some(4),
            Self::Sky => Some(5),
            Self::WaterColour => Some(6),
            _ => None,
        }
    }

    pub fn native_value(self, choice: usize) -> Option<u32> {
        if choice >= self.options().len() {
            return None;
        }
        if self == Self::WorldColourFilter {
            return WORLD_FILTER_VALUES.get(choice).copied();
        }
        if self == Self::SpecialtyResources {
            return Some(SPECIALTY_RESOURCE_SET | (1 << choice));
        }
        if self == Self::Terrain {
            return Some(FEATURE_SET | (1 << choice));
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

    pub fn labels_for_native(self, native_value: u32) -> Vec<&'static str> {
        if self.colour_index().is_some() {
            if native_value & GRASS_COLOUR_SET == 0 {
                return Vec::new();
            }
            return self
                .options()
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(index, label)| {
                    (native_value & !GRASS_COLOUR_SET & (1 << index) != 0).then_some(label)
                })
                .collect();
        }
        if matches!(self, Self::Terrain | Self::SpecialtyResources) {
            return self
                .options()
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(index, label)| {
                    self.native_value(index)
                        .is_some_and(|value| value & native_value & !(1 << 31) != 0)
                        .then_some(label)
                })
                .collect();
        }
        self.choice_index(native_value)
            .and_then(|index| self.options().get(index as usize))
            .copied()
            .into_iter()
            .collect()
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
    Rings => ("Has rings",Input),
    Moons => ("Has moons",Input),
    Dissonant => ("Dissonant",Query),
    Sentinels => ("Sentinels",Query),
    ExtremeSentinels => ("Extreme sentinels",Query),
    CorruptSentinels => ("Corrupt sentinels",Query),
    ExtremeStorms => ("Extreme storms",Weather),
    GlowingGrass => ("Glowing grass",Objects),
    GlowingFlora => ("Glowing non-grass flora",Objects),
    PlayerAttackingFauna => ("Player-attacking fauna",Fauna),
    VileBrood => ("Has vile brood",Hints),
    Sandworms => ("Has sandworms",Fauna),
    TitanWorms => ("Has titan worms",Objects),
}}

flags! { SystemFlag {
    Pirate => ("Outlaw system",System),
    Dissonance => ("Has dissonance",Query),
    ExtremeStorms => ("Contains extreme storms",Weather),
}}

impl PlanetFlag {
    pub const SENTINEL_LEVEL_PROJECTIONS: &[Self] = &[
        Self::Sentinels,
        Self::ExtremeSentinels,
        Self::CorruptSentinels,
    ];

    pub const fn is_sentinel_level_projection(self) -> bool {
        matches!(
            self,
            Self::Sentinels | Self::ExtremeSentinels | Self::CorruptSentinels
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaterFilter {
    #[default]
    Any,
    RequireAny,
    RequireDeep,
    ExcludeDeep,
    ExcludeAll,
}
impl WaterFilter {
    pub const ALL: &[Self] = &[
        Self::Any,
        Self::RequireAny,
        Self::RequireDeep,
        Self::ExcludeDeep,
        Self::ExcludeAll,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::RequireAny => "Require any water type",
            Self::RequireDeep => "Require deep water",
            Self::ExcludeDeep => "Exclude deep water",
            Self::ExcludeAll => "Exclude all water",
        }
    }
    pub fn accepts(self, water: WaterPresence) -> bool {
        match self {
            Self::Any => true,
            Self::RequireAny => water != WaterPresence::Dry,
            Self::RequireDeep => water == WaterPresence::Deep,
            Self::ExcludeDeep => water != WaterPresence::Deep,
            Self::ExcludeAll => water == WaterPresence::Dry,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WaterPresence {
    Dry,
    Shallow,
    Deep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    SystemFeatures,
    SystemDemographics,
    SystemComposition,
    PlanetType,
    Flora,
    SkyAtmospherics,
    PlanetFeatures,
    GeneralFauna,
    AquaticFauna,
    LandFauna,
    AirborneFauna,
    XenoArena,
    Resources,
    Weather,
}
impl Section {
    pub const ALL: &[Self] = &[
        Self::SystemFeatures,
        Self::SystemDemographics,
        Self::SystemComposition,
        Self::PlanetType,
        Self::Flora,
        Self::SkyAtmospherics,
        Self::PlanetFeatures,
        Self::GeneralFauna,
        Self::AquaticFauna,
        Self::LandFauna,
        Self::AirborneFauna,
        Self::XenoArena,
        Self::Resources,
        Self::Weather,
    ];
}
impl Field {
    pub fn section(self) -> Section {
        match self {
            Self::Star | Self::Anomaly => Section::SystemFeatures,
            Self::Wealth | Self::Economy | Self::ConflictLevel | Self::Population | Self::Race => {
                Section::SystemDemographics
            }
            Self::Biome | Self::Subtype | Self::Size | Self::Terrain => Section::PlanetType,
            Self::Flora | Self::Grass | Self::FloraColour => Section::Flora,
            Self::Sentinels | Self::WaterColour => Section::PlanetFeatures,
            Self::Fauna => Section::GeneralFauna,
            Self::ResourceRichness | Self::SpecialtyResources => Section::Resources,
            Self::StormType | Self::StormFrequency => Section::Weather,
            _ => Section::SkyAtmospherics,
        }
    }
}
impl PlanetFlag {
    pub fn section(self) -> Section {
        match self {
            Self::Rings | Self::Moons => Section::PlanetFeatures,
            Self::Sentinels | Self::ExtremeSentinels | Self::CorruptSentinels | Self::Dissonant => {
                Section::PlanetFeatures
            }
            Self::ExtremeStorms => Section::Weather,
            Self::GlowingGrass | Self::GlowingFlora => Section::Flora,
            _ => Section::GeneralFauna,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionSide {
    Include,
    Exclude,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionField {
    Biomes,
    Subtypes,
    Sizes,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExclusiveSelections {
    pub include: u64,
    pub exclude: u64,
}
impl ExclusiveSelections {
    pub fn get(self, side: SelectionSide) -> u64 {
        match side {
            SelectionSide::Include => self.include,
            SelectionSide::Exclude => self.exclude,
        }
    }
    pub fn set(&mut self, side: SelectionSide, mask: u64) {
        match side {
            SelectionSide::Include => {
                self.include = mask;
                self.exclude &= !mask;
            }
            SelectionSide::Exclude => {
                self.exclude = mask;
                self.include &= !mask;
            }
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composition {
    pub biomes: ExclusiveSelections,
    pub subtypes: ExclusiveSelections,
    pub sizes: ExclusiveSelections,
}
impl Composition {
    pub fn selections(&self, field: CompositionField) -> ExclusiveSelections {
        match field {
            CompositionField::Biomes => self.biomes,
            CompositionField::Subtypes => self.subtypes,
            CompositionField::Sizes => self.sizes,
        }
    }
    fn biome_scopes_overlap(&self) -> bool {
        self.biomes.include == 0
            || self.biomes.exclude == 0
            || self.biomes.include & self.biomes.exclude != 0
    }
    pub fn set(&mut self, field: CompositionField, side: SelectionSide, mask: u64) {
        match field {
            CompositionField::Biomes => {
                let old = self.biomes;
                self.biomes.set(side, mask);
                if old.include != self.biomes.include {
                    self.subtypes.include = 0;
                }
                if old.exclude != self.biomes.exclude {
                    self.subtypes.exclude = 0;
                }
            }
            CompositionField::Subtypes => {
                if self.biome_scopes_overlap() {
                    self.subtypes.set(side, mask);
                } else {
                    match side {
                        SelectionSide::Include => self.subtypes.include = mask,
                        SelectionSide::Exclude => self.subtypes.exclude = mask,
                    }
                }
            }
            CompositionField::Sizes => self.sizes.set(side, mask),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    pub selected: u32,
}

impl Resources {
    pub fn accepts(self, actual: u32) -> bool {
        self.selected == 0 || actual & self.selected != 0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criteria {
    pub choices: BTreeMap<Field, u64>,
    pub planets: BTreeMap<PlanetFlag, bool>,
    pub systems: BTreeMap<SystemFlag, bool>,
    pub resources: Resources,
    pub minimum_planets: Option<u8>,
    pub minimum_stars: Option<u8>,
    pub composition: Composition,
    pub system_water: WaterFilter,
    pub planet_water: WaterFilter,
    pub fauna: BTreeMap<crate::fauna::Domain, crate::fauna::FaunaCriteria>,
    pub arena: crate::arena::ArenaCriteria,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(deny_unknown_fields)]
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
        if self.field == Field::SpecialtyResources {
            return native_value & SPECIALTY_RESOURCE_SET != 0
                && Resources {
                    selected: self.mask as u32,
                }
                .accepts(native_value & !SPECIALTY_RESOURCE_SET);
        }
        if self.field == Field::Terrain {
            return native_value & FEATURE_SET != 0
                && native_value & self.mask as u32 & !FEATURE_SET != 0;
        }
        if self.field.colour_index().is_some() {
            let mask = self.mask as u32;
            return native_value & GRASS_COLOUR_SET != 0
                && native_value & mask & !GRASS_COLOUR_SET != 0;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PlanetSelection {
    Biome { biomes: u64, subtypes: u64 },
    Size(u64),
    Water(WaterFilter),
}
impl PlanetSelection {
    pub fn stage(self) -> Stage {
        match self {
            Self::Size(_) => Stage::Input,
            _ => Stage::Query,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Predicate {
    Choice(Choices),
    Planet(PlanetFlag, bool),
    System(SystemFlag, bool),
    Resources(Resources),
    MinimumPlanets(u8),
    MinimumStars(u8),
    Water(WaterFilter),
    Contains(PlanetSelection, bool),
    Fauna(crate::fauna::Selection),
    Arena(crate::arena::Selection),
}

impl Predicate {
    pub fn section(self) -> Section {
        match self {
            Self::Fauna(selection) => selection.domain.section(),
            Self::Arena(selection) => selection.section(),
            Self::Choice(choices) => choices.field.section(),
            Self::Planet(flag, _) => flag.section(),
            Self::System(SystemFlag::ExtremeStorms, _) | Self::MinimumPlanets(_) => {
                Section::SystemComposition
            }
            Self::MinimumStars(_)
            | Self::System(SystemFlag::Dissonance, _)
            | Self::Contains(PlanetSelection::Water(_), _) => Section::SystemFeatures,
            Self::System(SystemFlag::Pirate, _) => Section::SystemDemographics,
            Self::Resources(_) => Section::Resources,
            Self::Water(_) => Section::PlanetFeatures,
            Self::Contains(_, _) => Section::SystemComposition,
        }
    }
    pub fn stage(self) -> Stage {
        match self {
            Self::Fauna(_) | Self::Arena(_) => Stage::Fauna,
            Self::Choice(choices) => choices.field.stage(),
            Self::Planet(flag, _) => flag.stage(),
            Self::System(flag, _) => flag.stage(),
            Self::Resources(_) => Stage::Query,
            Self::MinimumPlanets(_) | Self::MinimumStars(_) => Stage::System,
            Self::Water(_) => Stage::Query,
            Self::Contains(selection, _) => selection.stage(),
        }
    }

    pub fn description(self) -> String {
        fn selected(mask: u64, labels: impl IntoIterator<Item = (usize, &'static str)>) -> String {
            labels
                .into_iter()
                .filter_map(|(index, label)| (mask & (1u64 << index) != 0).then_some(label))
                .collect::<Vec<_>>()
                .join(" | ")
        }
        fn expected(label: &str, value: bool) -> String {
            if value {
                label.to_owned()
            } else {
                format!("Not {label}")
            }
        }
        match self {
            Self::Choice(choices) => format!(
                "{}: {}",
                choices.field.label(),
                selected(
                    choices.mask,
                    choices.field.options().iter().copied().enumerate()
                )
            ),
            Self::Planet(flag, value) => expected(flag.label(), value),
            Self::System(flag, value) => expected(flag.label(), value),
            Self::Resources(resources) => format!(
                "Resources: {}",
                selected(
                    u64::from(resources.selected),
                    crate::vocabulary::RESOURCE_LABELS
                        .iter()
                        .copied()
                        .enumerate()
                )
            ),
            Self::MinimumPlanets(minimum) => format!("At least {minimum} planets"),
            Self::MinimumStars(minimum) => format!("At least {minimum} stars"),
            Self::Water(rule) => format!("Water: {}", rule.label()),
            Self::Contains(selection, value) => {
                let detail = match selection {
                    PlanetSelection::Biome { biomes, subtypes } => format!(
                        "biome [{}], subtype [{}]",
                        selected(biomes, Field::Biome.options().iter().copied().enumerate()),
                        selected(
                            subtypes,
                            Field::Subtype.options().iter().copied().enumerate()
                        )
                    ),
                    PlanetSelection::Size(mask) => format!(
                        "size [{}]",
                        selected(mask, Field::Size.options().iter().copied().enumerate())
                    ),
                    PlanetSelection::Water(rule) => format!("water [{}]", rule.label()),
                };
                if value {
                    format!("System contains a planet with {detail}")
                } else {
                    format!("System contains no planet with {detail}")
                }
            }
            Self::Fauna(selection) => {
                let mut details = Vec::new();
                if let Some(presence) = selection.presence {
                    details.push(expected(selection.domain.presence_label(), presence));
                }
                for (index, field) in crate::fauna::FaunaField::ALL.iter().copied().enumerate() {
                    let mask = selection.masks[index];
                    if mask != 0 {
                        details.push(format!(
                            "{}: {}",
                            field.label(),
                            selected(mask, field.options(selection.domain))
                        ));
                    }
                }
                if let Some(minimum) = selection.height.minimum_cm {
                    details.push(format!("Minimum height: {minimum} cm"));
                }
                if let Some(maximum) = selection.height.maximum_cm {
                    details.push(format!("Maximum height: {maximum} cm"));
                }
                format!("{}: {}", selection.domain.label(), details.join("; "))
            }
            Self::Arena(selection) => {
                let mut details = Vec::new();
                if let Some(presence) = selection.presence {
                    details.push(expected("Has Arena-eligible fauna", presence));
                }
                for (index, field) in crate::arena::ArenaField::ALL.iter().copied().enumerate() {
                    let mask = selection.masks[index];
                    if mask != 0 {
                        details.push(format!(
                            "{}: {}",
                            field.label(),
                            selected(mask, field.options())
                        ));
                    }
                }
                format!("Arena: {}", details.join("; "))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Requirements {
    pub planet_query: bool,
    pub hints: bool,
    pub fauna: bool,
    pub metadata: bool,
    pub weather: bool,
    pub terrain: bool,
    pub screen_filter: bool,
    pub colours: u16,
    pub objects: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Each predicate is required. Multi-value alternatives belong inside a predicate's mask;
/// sections group presentation and never change the conjunction between filters.
pub struct Plan {
    pub system: Vec<Predicate>,
    pub planet: Vec<Predicate>,
    pub requirements: Requirements,
}

impl Criteria {
    const INHABITED: u64 = 1 << 0;
    const EMPTY: u64 = 1 << 1;
    const ABANDONED: u64 = 1 << 2;
    const GAS_GIANT: u64 = 1 << 15;
    const GIANT: u64 = 1 << 4;
    const CORRUPTED_SENTINELS: u64 = 1 << 3;
    const NORMAL_ECONOMY_LEVELS: u64 = 0b111;
    const PIRATE_ECONOMY_LEVEL: u64 = 1 << 3;

    fn option_mask(field: Field) -> u64 {
        field
            .available_options()
            .fold(0, |mask, (index, _)| mask | 1 << index)
    }

    fn selected_or_all(&self, field: Field) -> u64 {
        self.choices
            .get(&field)
            .copied()
            .filter(|mask| *mask != 0)
            .unwrap_or_else(|| Self::option_mask(field))
    }

    pub fn storm_frequency_is_only_none(&self) -> bool {
        self.choices
            .get(&Field::StormFrequency)
            .is_some_and(|mask| *mask == 1)
    }

    fn choice_is_only(&self, field: Field, option: usize) -> bool {
        self.choices.get(&field) == Some(&(1 << option))
    }

    pub fn flora_details_available(&self) -> bool {
        !self.choice_is_only(Field::Flora, 0)
    }

    pub fn fauna_details_available(&self) -> bool {
        !self.choice_is_only(Field::Fauna, 0)
    }

    pub fn fauna_domain_available(&self, domain: crate::fauna::Domain) -> bool {
        self.fauna_details_available()
            && (domain != crate::fauna::Domain::Aquatic || !self.target_planet_must_be_dry())
    }

    pub fn arena_available(&self) -> bool {
        self.fauna_details_available()
    }

    pub fn enforce_weather_dependencies(&mut self) {
        if self.storm_frequency_is_only_none() {
            self.choices.remove(&Field::StormType);
            self.planets.remove(&PlanetFlag::ExtremeStorms);
        }
        if self.systems.get(&SystemFlag::ExtremeStorms) == Some(&false) {
            self.planets.remove(&PlanetFlag::ExtremeStorms);
        }
    }

    fn population_candidates(&self) -> u64 {
        let mut candidates = self.selected_or_all(Field::Population);
        if let Some(races) = self.choices.get(&Field::Race).copied() {
            let mut supported = 0;
            if races & 0b111 != 0 {
                supported |= Self::INHABITED | Self::ABANDONED;
            }
            if races & (1 << 7) != 0 {
                supported |= Self::EMPTY;
            }
            candidates &= supported;
        }
        candidates
    }

    pub fn population_allows_inhabited(&self) -> bool {
        self.population_candidates() & Self::INHABITED != 0
    }

    pub fn race_available_mask(&self) -> u64 {
        let populations = self.selected_or_all(Field::Population);
        if populations == Self::EMPTY {
            // Empty already implies the sole available value, None, so a second
            // filter cannot narrow the result.
            return 0;
        }
        let mut available = 0;
        if populations & (Self::INHABITED | Self::ABANDONED) != 0 {
            available |= 0b111;
        }
        if populations & Self::EMPTY != 0 {
            available |= 1 << 7;
        }
        available
    }

    pub fn outlaw_filter_available(&self) -> bool {
        self.population_allows_inhabited()
    }

    fn demographic_choice_mask(&self, field: Field) -> u64 {
        if !self.population_allows_inhabited() {
            return 0;
        }
        let outlaw = self.systems.get(&SystemFlag::Pirate).copied();
        let wealth = self.choices.get(&Field::Wealth).copied().unwrap_or(0);
        match field {
            Field::Wealth => match outlaw {
                Some(true) => Self::PIRATE_ECONOMY_LEVEL,
                Some(false) => Self::NORMAL_ECONOMY_LEVELS,
                None => Self::NORMAL_ECONOMY_LEVELS | Self::PIRATE_ECONOMY_LEVEL,
            },
            Field::Economy => {
                let wealth_allows_normal = wealth == 0 || wealth & Self::NORMAL_ECONOMY_LEVELS != 0;
                if outlaw == Some(true) || !wealth_allows_normal {
                    0
                } else {
                    Self::option_mask(Field::Economy)
                }
            }
            Field::ConflictLevel => {
                let wealth_allows_normal = wealth == 0 || wealth & Self::NORMAL_ECONOMY_LEVELS != 0;
                let wealth_allows_pirate = wealth == 0 || wealth & Self::PIRATE_ECONOMY_LEVEL != 0;
                let economy_selected = self
                    .choices
                    .get(&Field::Economy)
                    .is_some_and(|mask| *mask != 0);
                let normal = outlaw != Some(true) && wealth_allows_normal;
                let pirate = outlaw != Some(false) && !economy_selected && wealth_allows_pirate;
                (if normal {
                    Self::NORMAL_ECONOMY_LEVELS
                } else {
                    0
                }) | (if pirate {
                    Self::PIRATE_ECONOMY_LEVEL
                } else {
                    0
                })
            }
            _ => Self::option_mask(field),
        }
    }

    pub fn enforce_population_dependencies(&mut self) {
        let allowed_races = self.race_available_mask();
        if let Some(mask) = self.choices.get_mut(&Field::Race) {
            *mask &= allowed_races;
        }
        if self.choices.get(&Field::Race) == Some(&0) {
            self.choices.remove(&Field::Race);
        }
        if !self.outlaw_filter_available() {
            self.systems.remove(&SystemFlag::Pirate);
        }
        for field in [Field::Wealth, Field::Economy, Field::ConflictLevel] {
            let allowed = self.demographic_choice_mask(field);
            if let Some(mask) = self.choices.get_mut(&field) {
                *mask &= allowed;
            }
            if self.choices.get(&field) == Some(&0) {
                self.choices.remove(&field);
            }
        }
    }

    pub fn planet_biome_is_only_gas_giant(&self) -> bool {
        self.biome_choices() == Some(Self::GAS_GIANT)
    }

    pub fn derived_planet_size(&self) -> Option<usize> {
        self.planet_biome_is_only_gas_giant().then_some(4)
    }

    fn biome_option_contains(option: usize, actual: usize) -> bool {
        option == actual || (option == 16 && (8..=10).contains(&actual))
    }

    fn target_biome_contains(selected: u64, actual: usize) -> bool {
        selected == 0
            || selected & (1 << actual) != 0
            || ((8..=10).contains(&actual) && selected & (1 << 16) != 0)
    }

    fn system_excludes_biome_subtype_pair(&self, biome: usize, subtype: usize) -> bool {
        let biomes = self.composition.biomes.exclude;
        let subtypes = self.composition.subtypes.exclude;
        if biomes == 0 && subtypes == 0 {
            return false;
        }
        let biome_matches = biomes == 0 || Self::target_biome_contains(biomes, biome);
        let subtype_matches = subtypes == 0 || subtypes & (1 << subtype) != 0;
        biome_matches && subtype_matches
    }

    fn available_target_biome_mask(&self) -> u64 {
        let selected_subtypes = self.choices.get(&Field::Subtype).copied().unwrap_or(0);
        Field::Biome
            .available_options()
            .filter(|(option, _)| {
                if *option == 15 && self.composition.sizes.exclude & Self::GIANT != 0 {
                    return false;
                }
                (0..16).any(|actual| {
                    Self::biome_option_contains(*option, actual)
                        && (0..29).any(|subtype| {
                            biome_subtype_present(actual, subtype)
                                && (selected_subtypes == 0
                                    || selected_subtypes & (1 << subtype) != 0)
                                && !self.system_excludes_biome_subtype_pair(actual, subtype)
                        })
                })
            })
            .fold(0, |mask, (option, _)| mask | 1 << option)
    }

    fn available_target_subtype_mask(&self) -> u64 {
        let selected_biomes = self.biome_choices().unwrap_or(0);
        (0..29)
            .filter(|subtype| {
                Field::Biome.available_options().any(|(option, _)| {
                    (selected_biomes == 0 || selected_biomes & (1 << option) != 0)
                        && (0..16).any(|actual| {
                            Self::biome_option_contains(option, actual)
                                && biome_subtype_present(actual, *subtype)
                                && !self.system_excludes_biome_subtype_pair(actual, *subtype)
                        })
                })
            })
            .fold(0, |mask, subtype| mask | 1 << subtype)
    }

    pub fn available_choice_mask(&self, field: Field) -> u64 {
        let mut available = Self::option_mask(field);
        match field {
            Field::Race => return self.race_available_mask(),
            Field::Wealth | Field::Economy | Field::ConflictLevel => {
                return self.demographic_choice_mask(field);
            }
            Field::Biome => return self.available_target_biome_mask(),
            Field::Subtype => return self.available_target_subtype_mask(),
            Field::Size => available &= !self.composition.sizes.exclude,
            Field::Sentinels => {
                if self.systems.get(&SystemFlag::Dissonance) == Some(&false) {
                    available &= !Self::CORRUPTED_SENTINELS;
                }
            }
            Field::StormType if self.storm_frequency_is_only_none() => return 0,
            Field::WaterColour if self.target_planet_must_be_dry() => return 0,
            Field::Grass | Field::FloraColour if !self.flora_details_available() => return 0,
            _ => {}
        }
        available
    }

    pub fn choice_options(&self, field: Field) -> Vec<(usize, &'static str)> {
        let available = self.available_choice_mask(field);
        field
            .available_options()
            .filter(|(index, _)| available & (1 << index) != 0)
            .collect()
    }

    pub fn planet_requirement_available(&self, flag: PlanetFlag) -> bool {
        match flag {
            PlanetFlag::ExtremeStorms => {
                !self.storm_frequency_is_only_none()
                    && self.systems.get(&SystemFlag::ExtremeStorms) != Some(&false)
            }
            PlanetFlag::Dissonant | PlanetFlag::CorruptSentinels => {
                if self.systems.get(&SystemFlag::Dissonance) == Some(&false) {
                    return false;
                }
                if flag == PlanetFlag::CorruptSentinels {
                    return self.sentinel_requirement_can_narrow(flag);
                }
                true
            }
            PlanetFlag::Sentinels | PlanetFlag::ExtremeSentinels => {
                self.sentinel_requirement_can_narrow(flag)
            }
            PlanetFlag::GlowingGrass | PlanetFlag::GlowingFlora => self.flora_details_available(),
            PlanetFlag::PlayerAttackingFauna => self.fauna_details_available(),
            _ => true,
        }
    }

    fn sentinel_requirement_can_narrow(&self, flag: PlanetFlag) -> bool {
        let levels =
            self.selected_or_all(Field::Sentinels) & self.available_choice_mask(Field::Sentinels);
        let true_levels = match flag {
            PlanetFlag::Sentinels => 0b1110,
            PlanetFlag::ExtremeSentinels => 0b0100,
            PlanetFlag::CorruptSentinels => 0b1000,
            _ => return true,
        };
        levels & true_levels != 0 && levels & !true_levels != 0
    }

    pub fn target_planet_must_be_dry(&self) -> bool {
        self.planet_water == WaterFilter::ExcludeAll || self.system_water == WaterFilter::ExcludeAll
    }

    pub fn planet_water_option_available(&self, value: WaterFilter) -> bool {
        match value {
            WaterFilter::RequireAny => self.system_water != WaterFilter::ExcludeAll,
            WaterFilter::RequireDeep => !matches!(
                self.system_water,
                WaterFilter::ExcludeDeep | WaterFilter::ExcludeAll
            ),
            WaterFilter::ExcludeAll => {
                !self.choices.contains_key(&Field::WaterColour)
                    && !self
                        .fauna
                        .get(&crate::fauna::Domain::Aquatic)
                        .is_some_and(|fauna| {
                            fauna.presence == Some(true) || fauna.has_detail_filters()
                        })
            }
            _ => true,
        }
    }

    pub fn system_water_option_available(&self, value: WaterFilter) -> bool {
        match value {
            WaterFilter::ExcludeDeep => self.planet_water != WaterFilter::RequireDeep,
            WaterFilter::ExcludeAll => {
                !matches!(
                    self.planet_water,
                    WaterFilter::RequireAny | WaterFilter::RequireDeep
                ) && !self.choices.contains_key(&Field::WaterColour)
                    && !self
                        .fauna
                        .get(&crate::fauna::Domain::Aquatic)
                        .is_some_and(|fauna| {
                            fauna.presence == Some(true) || fauna.has_detail_filters()
                        })
            }
            _ => true,
        }
    }

    fn enforce_cross_scope_dependencies(&mut self) {
        for field in [
            Field::Biome,
            Field::Subtype,
            Field::Size,
            Field::Sentinels,
            Field::WaterColour,
        ] {
            let allowed = self.available_choice_mask(field);
            if let Some(mask) = self.choices.get_mut(&field) {
                *mask &= allowed;
            }
            if self.choices.get(&field) == Some(&0) {
                self.choices.remove(&field);
            }
        }
        if self.planet_biome_is_only_gas_giant() {
            self.choices.remove(&Field::Size);
        }
        if self.systems.get(&SystemFlag::Dissonance) == Some(&false) {
            for flag in [PlanetFlag::Dissonant, PlanetFlag::CorruptSentinels] {
                self.planets.remove(&flag);
            }
        }
        for flag in [
            PlanetFlag::Sentinels,
            PlanetFlag::ExtremeSentinels,
            PlanetFlag::CorruptSentinels,
            PlanetFlag::GlowingGrass,
            PlanetFlag::GlowingFlora,
            PlanetFlag::PlayerAttackingFauna,
        ] {
            if !self.planet_requirement_available(flag) {
                self.planets.remove(&flag);
            }
        }
        if !self.flora_details_available() {
            for field in [Field::Grass, Field::FloraColour] {
                self.choices.remove(&field);
            }
            for flag in [PlanetFlag::GlowingGrass, PlanetFlag::GlowingFlora] {
                self.planets.remove(&flag);
            }
        }
        if !self.fauna_details_available() {
            self.fauna.clear();
            self.arena = crate::arena::ArenaCriteria::default();
            self.planets.remove(&PlanetFlag::PlayerAttackingFauna);
        }
        match self.system_water {
            WaterFilter::ExcludeAll => {
                self.planet_water = WaterFilter::Any;
            }
            WaterFilter::ExcludeDeep
                if matches!(
                    self.planet_water,
                    WaterFilter::RequireDeep | WaterFilter::ExcludeDeep
                ) =>
            {
                self.planet_water = WaterFilter::Any;
            }
            _ => {}
        }
        if self.target_planet_must_be_dry() {
            self.fauna.remove(&crate::fauna::Domain::Aquatic);
        }
    }

    pub fn enforce_dependencies(&mut self) {
        self.arena.enforce_dependencies();
        for (&domain, fauna) in &mut self.fauna {
            fauna.enforce_domain_dependencies(domain);
        }
        self.enforce_sentinel_dependencies();
        self.enforce_cross_scope_dependencies();
        self.enforce_weather_dependencies();
        self.enforce_population_dependencies();
        self.enforce_cross_scope_dependencies();
    }

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
        if accepted & mega == mega {
            Some((accepted & !mega) | (1 << 16))
        } else {
            Some(accepted)
        }
    }

    pub fn set_biome_choices(&mut self, mask: u64) {
        if self.biome_choices() != Some(mask) {
            self.choices.remove(&Field::Subtype);
        }
        if mask == Self::GAS_GIANT {
            self.choices.remove(&Field::Size);
        }
        if mask == 0 {
            self.choices.remove(&Field::Biome);
        } else {
            self.choices.insert(Field::Biome, mask);
        }
    }

    pub fn subtype_options(&self) -> Vec<(u64, &'static str)> {
        let Some(selected) = self.biome_choices() else {
            return Vec::new();
        };
        let available = self.available_target_subtype_mask();
        subtype_options_for(selected)
            .into_iter()
            .filter_map(|(mask, label)| {
                let mask = mask & available;
                (mask != 0).then_some((mask, label))
            })
            .collect()
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
        for &flag in PlanetFlag::SENTINEL_LEVEL_PROJECTIONS {
            self.planets.remove(&flag);
        }
        if mask == 0 {
            self.choices.remove(&Field::Sentinels);
        } else {
            self.choices.insert(Field::Sentinels, mask);
        }
    }

    fn enforce_sentinel_dependencies(&mut self) {
        let has_direct_flags = PlanetFlag::SENTINEL_LEVEL_PROJECTIONS
            .iter()
            .any(|flag| self.planets.contains_key(flag));
        if has_direct_flags && let Some(mask) = self.sentinel_choices() {
            self.set_sentinel_choices(mask);
        }
    }

    fn compile_composition(&self, plan: &mut Plan) -> Result<(), &'static str> {
        for (field, pair) in [
            (Field::Biome, self.composition.biomes),
            (Field::Size, self.composition.sizes),
        ] {
            let allowed = field
                .available_options()
                .fold(0u64, |mask, (index, _)| mask | 1 << index);
            if (pair.include | pair.exclude) & !allowed != 0 {
                return Err("unsupported system composition choice");
            }
            if pair.include & pair.exclude != 0 {
                return Err("conflicting system composition choices");
            }
        }
        if self.composition.biome_scopes_overlap()
            && self.composition.subtypes.include & self.composition.subtypes.exclude != 0
        {
            return Err("conflicting system subtype choices");
        }
        for side in [SelectionSide::Include, SelectionSide::Exclude] {
            let biomes = self.composition.biomes.get(side);
            let subtypes = self.composition.subtypes.get(side);
            let options = subtype_options_for(biomes);
            let allowed = options.iter().fold(0, |mask, (option, _)| mask | option);
            if subtypes & !allowed != 0 {
                return Err("unsupported system biome subtype");
            }
            let expected = side == SelectionSide::Include;
            if biomes != 0 || subtypes != 0 {
                plan.system.push(Predicate::Contains(
                    PlanetSelection::Biome { biomes, subtypes },
                    expected,
                ));
            }
            let sizes = self.composition.sizes.get(side);
            if sizes != 0 {
                plan.system
                    .push(Predicate::Contains(PlanetSelection::Size(sizes), expected));
            }
        }
        Ok(())
    }

    pub fn compile(&self) -> Result<Plan, &'static str> {
        let has_sentinel_projections = PlanetFlag::SENTINEL_LEVEL_PROJECTIONS
            .iter()
            .any(|flag| self.planets.contains_key(flag));
        if has_sentinel_projections && self.sentinel_choices().is_none() {
            return Err("sentinel filters have no possible shared level");
        }
        if has_sentinel_projections {
            return Err("use the unified Sentinels filter instead of sentinel level projections");
        }
        if self.storm_frequency_is_only_none()
            && (self.choices.contains_key(&Field::StormType)
                || self.planets.contains_key(&PlanetFlag::ExtremeStorms))
        {
            return Err("storm type and extreme storms require a storm frequency other than None");
        }
        if self
            .choices
            .get(&Field::Race)
            .is_some_and(|mask| mask & !self.race_available_mask() != 0)
        {
            return Err("dominant lifeform is incompatible with the population selection");
        }
        if self.systems.contains_key(&SystemFlag::Pirate) && !self.outlaw_filter_available() {
            return Err("outlaw system cannot narrow the selected population and lifeform");
        }
        for field in [Field::Wealth, Field::Economy, Field::ConflictLevel] {
            if self
                .choices
                .get(&field)
                .is_some_and(|mask| mask & !self.demographic_choice_mask(field) != 0)
            {
                return Err(
                    "economy and conflict filters are incompatible with the selected demographics",
                );
            }
        }
        if !self.flora_details_available()
            && (self.choices.contains_key(&Field::Grass)
                || self.choices.contains_key(&Field::FloraColour)
                || [PlanetFlag::GlowingGrass, PlanetFlag::GlowingFlora]
                    .into_iter()
                    .any(|flag| self.planets.contains_key(&flag)))
        {
            return Err("flora appearance filters require planet flora");
        }
        if !self.fauna_details_available()
            && (!self.fauna.is_empty()
                || self.arena.presence.is_some()
                || self.arena.has_detail_filters()
                || self.planets.contains_key(&PlanetFlag::PlayerAttackingFauna))
        {
            return Err("fauna detail filters require planet fauna");
        }
        for flag in [
            PlanetFlag::Sentinels,
            PlanetFlag::ExtremeSentinels,
            PlanetFlag::CorruptSentinels,
            PlanetFlag::GlowingGrass,
            PlanetFlag::GlowingFlora,
            PlanetFlag::PlayerAttackingFauna,
        ] {
            if self.planets.contains_key(&flag) && !self.planet_requirement_available(flag) {
                return Err("planet requirement cannot narrow the current filter values");
            }
        }
        if self.systems.get(&SystemFlag::ExtremeStorms) == Some(&false)
            && self.planets.get(&PlanetFlag::ExtremeStorms) == Some(&true)
        {
            return Err(
                "a target planet cannot require extreme storms in a system that excludes them",
            );
        }
        if self.systems.get(&SystemFlag::Dissonance) == Some(&false)
            && (self.planets.get(&PlanetFlag::Dissonant) == Some(&true)
                || self.planets.get(&PlanetFlag::CorruptSentinels) == Some(&true)
                || self
                    .choices
                    .get(&Field::Sentinels)
                    .is_some_and(|mask| mask & Self::CORRUPTED_SENTINELS != 0))
        {
            return Err(
                "a target planet cannot require dissonance or corrupted sentinels in a system that excludes dissonance",
            );
        }
        if self.planet_biome_is_only_gas_giant() && self.choices.contains_key(&Field::Size) {
            return Err("a Gas Giant target already has the derived Giant size");
        }
        for field in [
            Field::Biome,
            Field::Subtype,
            Field::Size,
            Field::WaterColour,
        ] {
            if self
                .choices
                .get(&field)
                .is_some_and(|mask| mask & !self.available_choice_mask(field) != 0)
            {
                return Err("target planet choice conflicts with a system-wide exclusion");
            }
        }
        if !self.planet_water_option_available(self.planet_water) {
            return Err("target planet water filter conflicts with another target constraint");
        }
        if !self.system_water_option_available(self.system_water) {
            return Err("system water exclusion conflicts with the target planet filters");
        }
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
            if field.colour_index().is_some()
                || matches!(
                    field,
                    Field::Subtype
                        | Field::Flora
                        | Field::Fauna
                        | Field::StormFrequency
                        | Field::WaterColour
                )
            {
                let allowed = if field == Field::Subtype {
                    self.subtype_options()
                        .iter()
                        .fold(0u64, |allowed, (mask, _)| allowed | mask)
                } else {
                    field
                        .available_options()
                        .fold(0u64, |allowed, (index, _)| allowed | (1 << index))
                };
                if mask & !allowed != 0 {
                    return Err("unsupported criterion choice");
                }
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
        }
        if self.planet_water != WaterFilter::Any {
            plan.planet.push(Predicate::Water(self.planet_water));
        }
        if self.system_water != WaterFilter::Any {
            let (rule, expected) = match self.system_water {
                WaterFilter::ExcludeDeep => (WaterFilter::RequireDeep, false),
                WaterFilter::ExcludeAll => (WaterFilter::RequireAny, false),
                rule => (rule, true),
            };
            plan.system
                .push(Predicate::Contains(PlanetSelection::Water(rule), expected));
        }
        self.compile_composition(&mut plan)?;
        for selection in self.arena.compile()? {
            plan.planet.push(Predicate::Arena(selection));
        }
        for (&domain, criteria) in &self.fauna {
            if let Some(selection) = criteria.compile(domain)? {
                plan.planet.push(Predicate::Fauna(selection));
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
        if let Some(minimum) = self.minimum_stars {
            if !(1..=3).contains(&minimum) {
                return Err("star count must be between one and three");
            }
            plan.system.push(Predicate::MinimumStars(minimum));
        }
        if self.resources.selected >> RESOURCE_IDS.len() != 0 {
            return Err("unknown resource choice");
        }
        if self.resources.selected != 0 {
            plan.planet.push(Predicate::Resources(self.resources));
        }
        for predicate in plan.system.iter().chain(&plan.planet) {
            plan.requirements.planet_query |= predicate.stage() >= Stage::Query;
            plan.requirements.metadata |= predicate.stage() == Stage::Metadata;
            plan.requirements.hints |= predicate.stage() == Stage::Hints;
            plan.requirements.fauna |= predicate.stage() == Stage::Fauna;
            plan.requirements.weather |= predicate.stage() == Stage::Weather;
            plan.requirements.terrain |= predicate.stage() == Stage::Terrain;
            plan.requirements.screen_filter |= predicate.stage() == Stage::ScreenFilter;
            plan.requirements.objects |= predicate.stage() == Stage::Objects;
            if let Predicate::Choice(choices) = predicate {
                plan.requirements.objects |= choices.field() == Field::Terrain
                    && choices.mask() & u64::from(FLOATING_ISLANDS) != 0;
            }
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

pub fn subtype_options_for(selected: u64) -> Vec<(u64, &'static str)> {
    let mut options: Vec<(u64, &'static str)> = Vec::new();
    for choice in 0..29 {
        let mut label = None;
        let mut ambiguous = false;
        for (biome, _) in Field::Biome.available_options() {
            if (selected != 0 && selected & (1 << biome) == 0)
                || !biome_subtype_present(biome, choice)
            {
                continue;
            }
            match biome_subtype_label(biome, choice) {
                Some(current) if label.is_none() || label == Some(current) => {
                    label = Some(current);
                }
                _ => ambiguous = true,
            }
        }
        if !ambiguous && let Some(label) = label {
            if let Some((mask, _)) = options.iter_mut().find(|(_, name)| *name == label) {
                *mask |= 1 << choice;
            } else {
                options.push((1 << choice, label));
            }
        }
    }
    options
}

#[cfg(test)]
mod tests {
    use super::Field;

    fn available_labels(field: Field) -> Vec<&'static str> {
        field.available_options().map(|(_, label)| label).collect()
    }

    #[test]
    fn atmospheric_colour_options_match_authored_game_tables() {
        let chromatic_and_light_neutrals = vec![
            "Red", "Orange", "Yellow", "Green", "Cyan", "Blue", "Purple", "Magenta", "Grey",
            "White",
        ];
        assert_eq!(
            available_labels(Field::Horizon),
            chromatic_and_light_neutrals
        );
        assert_eq!(available_labels(Field::Sky), chromatic_and_light_neutrals);
        assert_eq!(
            available_labels(Field::WaterColour),
            vec![
                "Red", "Orange", "Green", "Cyan", "Blue", "Purple", "Magenta"
            ]
        );
    }
}
