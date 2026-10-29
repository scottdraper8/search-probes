use crate::{
    terrain::{FEATURE_SET, FLOATING_ISLANDS, TERRAIN_FEATURES},
    vocabulary::*,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    Conflict => ("Conflict",CONFLICT_LEVELS,System),
    Population => ("Population",POPULATION_STATES,System),
    Race => ("Dominant lifeform",RACE_ENUM,System),
    Anomaly => ("Space anomaly",ANOMALIES,System),
    Size => ("Size",PLANET_SIZES,Input),
    Biome => ("Biome",BIOMES,Query),
    Subtype => ("Biome subtype",BIOME_SUBTYPES,Query),
    Terrain => ("Terrain",TERRAIN_FEATURES,Terrain),
    Sentinels => ("Sentinels",SENTINEL_LEVELS,Query),
    Flora => ("Flora",PLANET_LIFE_LEVELS,Metadata),
    Fauna => ("Quantity",PLANET_LIFE_LEVELS,Metadata),
    ResourceRichness => ("Resource abundance",RESOURCE_LEVELS,Metadata),
    StormType => ("Storm type",STORM_TYPES,Weather),
    StormFrequency => ("Storm frequency",STORM_FREQUENCIES,Query),
    Grass => ("Grass colour",GRASS_COLOURS,Objects),
    FloraColour => ("Flora colour",GRASS_COLOURS,Objects),
    Cloud => ("Cloud colour",GRASS_COLOURS,Colour),
    SkyFog => ("Sky fog colour",GRASS_COLOURS,Colour),
    SkyHeightFog => ("Height fog colour",GRASS_COLOURS,Colour),
    Horizon => ("Horizon colour",GRASS_COLOURS,Colour),
    Sky => ("Daytime sky colour",GRASS_COLOURS,Colour),
    NightSky => ("Night sky colour",GRASS_COLOURS,Colour),
    WaterColour => ("Water colour",GRASS_COLOURS,Colour),
    WorldColourFilter => ("World colour filter",WORLD_FILTERS,ScreenFilter),
    UniqueResources => ("Unique resources",UNIQUE_RESOURCES,Hints),
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
                Self::Sky | Self::Horizon | Self::SkyFog => *index != 9,
                Self::SkyHeightFog => !matches!(index, 6 | 8 | 9),

                _ => true,
            })
    }

    pub fn colour_index(self) -> Option<usize> {
        [
            Self::Grass,
            Self::FloraColour,
            Self::Cloud,
            Self::SkyFog,
            Self::SkyHeightFog,
            Self::Horizon,
            Self::Sky,
            Self::NightSky,
            Self::WaterColour,
        ]
        .iter()
        .position(|field| *field == self)
        .map(|index| if index >= 2 { index + 1 } else { index })
    }

    pub fn native_value(self, choice: usize) -> Option<u32> {
        if choice >= self.options().len() {
            return None;
        }
        if self == Self::WorldColourFilter {
            return WORLD_FILTER_VALUES.get(choice).copied();
        }
        if self == Self::UniqueResources {
            return Some(UNIQUE_RESOURCE_SET | (1 << choice));
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
    Infested => ("Infested",Query),
    Relic => ("Relic",Query),
    Rgb => ("Mega-exotic biome",Query),
    Dead => ("Dead",Query),
    Weird => ("Weird / Exotic",Query),
    GlowingGrass => ("Glowing grass",Objects),
    GlowingFlora => ("Glowing flora",Objects),
    PlayerAttackingFauna => ("Player-attacking fauna",Fauna),
    VileBrood => ("Has vile brood",Hints),
    Sandworms => ("Has sandworms",Fauna),
    TitanWorms => ("Has titan worms",Objects),
}}

flags! { SystemFlag {
    Pirate => ("Outlaw system",System),
    Dissonance => ("Dissonance",Query),
    ExtremeStorms => ("Contains extreme storms",Weather),
}}

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
pub enum WaterPresence {
    Dry,
    Shallow,
    Deep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Composition,
    Profile,
    Environment,
    Colours,
    Conditions,
    GeneralFauna,
    AquaticFauna,
    LandFauna,
    AirborneFauna,
    XenoArena,
    Features,
    Resources,
    Type,
    Weather,
}
impl Section {
    pub const ALL: &[Self] = &[
        Self::Composition,
        Self::Profile,
        Self::Environment,
        Self::Colours,
        Self::Conditions,
        Self::GeneralFauna,
        Self::AquaticFauna,
        Self::LandFauna,
        Self::AirborneFauna,
        Self::XenoArena,
        Self::Features,
        Self::Resources,
        Self::Type,
        Self::Weather,
    ];
}
impl Field {
    pub fn section(self) -> Section {
        match self {
            Self::Star
            | Self::Wealth
            | Self::Economy
            | Self::Conflict
            | Self::Population
            | Self::Race
            | Self::Anomaly => Section::Profile,
            Self::Biome | Self::Subtype | Self::Flora => Section::Environment,
            Self::Size => Section::Type,
            Self::Sentinels => Section::Conditions,
            Self::Fauna => Section::GeneralFauna,
            Self::Terrain => Section::Features,
            Self::ResourceRichness | Self::UniqueResources => Section::Resources,
            Self::StormType | Self::StormFrequency => Section::Weather,
            _ => Section::Colours,
        }
    }
}
impl PlanetFlag {
    pub fn section(self) -> Section {
        match self {
            Self::Rings | Self::Moons => Section::Features,
            Self::Sentinels | Self::ExtremeSentinels | Self::CorruptSentinels | Self::Dissonant => {
                Section::Conditions
            }
            Self::ExtremeStorms => Section::Weather,
            Self::GlowingGrass | Self::GlowingFlora | Self::Rgb => Section::Environment,
            Self::Dead | Self::Weird | Self::Infested | Self::Relic => Section::Type,
            _ => Section::GeneralFauna,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectionSide {
    Include,
    Exclude,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompositionField {
    Biomes,
    Subtypes,
    Sizes,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
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
    pub unique_resource_mode: ResourceMode,
    pub minimum_planets: Option<u8>,
    pub composition: Composition,
    pub system_water: WaterFilter,
    pub planet_water: WaterFilter,
    pub match_modes: BTreeMap<Section, ResourceMode>,
    pub fauna: BTreeMap<crate::fauna::Domain, crate::fauna::FaunaCriteria>,
    pub arena: crate::arena::ArenaCriteria,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Choices {
    field: Field,
    mask: u64,
    mode: ResourceMode,
}

impl Choices {
    pub fn field(self) -> Field {
        self.field
    }
    pub fn mask(self) -> u64 {
        self.mask
    }
    pub fn accepts(self, native_value: u32) -> bool {
        if self.field == Field::UniqueResources {
            return native_value & UNIQUE_RESOURCE_SET != 0
                && Resources {
                    selected: self.mask as u32,
                    mode: self.mode,
                }
                .accepts(native_value & !UNIQUE_RESOURCE_SET);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Predicate {
    Choice(Choices),
    Planet(PlanetFlag, bool),
    System(SystemFlag, bool),
    Resources(Resources),
    MinimumPlanets(u8),
    Water(WaterFilter),
    Contains(PlanetSelection, bool),
    Fauna(crate::fauna::Selection),
    Arena(crate::arena::Selection),
}

impl Predicate {
    pub fn section(self) -> Section {
        match self {
            Self::Fauna(selection) => selection.domain.section(),
            Self::Arena(_) => Section::XenoArena,
            Self::Choice(choices) => choices.field.section(),
            Self::Planet(flag, _) => flag.section(),
            Self::System(SystemFlag::ExtremeStorms, _) | Self::MinimumPlanets(_) => {
                Section::Composition
            }
            Self::System(_, _) | Self::Contains(PlanetSelection::Water(_), _) => Section::Profile,
            Self::Resources(_) => Section::Resources,
            Self::Water(_) => Section::Conditions,
            Self::Contains(_, _) => Section::Composition,
        }
    }
    pub fn stage(self) -> Stage {
        match self {
            Self::Fauna(_) | Self::Arena(_) => Stage::Fauna,
            Self::Choice(choices) => choices.field.stage(),
            Self::Planet(flag, _) => flag.stage(),
            Self::System(flag, _) => flag.stage(),
            Self::Resources(_) => Stage::Query,
            Self::MinimumPlanets(_) => Stage::System,
            Self::Water(_) => Stage::Query,
            Self::Contains(selection, _) => selection.stage(),
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
pub struct Plan {
    pub system: Vec<Predicate>,
    pub planet: Vec<Predicate>,
    pub requirements: Requirements,
    pub match_modes: [ResourceMode; Section::ALL.len()],
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
        if self.biome_choices() != Some(mask) {
            self.choices.remove(&Field::Subtype);
        }
        self.planets.remove(&PlanetFlag::Rgb);
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
        subtype_options_for(selected)
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

    pub fn match_mode(&self, section: Section) -> ResourceMode {
        self.match_modes.get(&section).copied().unwrap_or_default()
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
            let all = expected && self.match_mode(Section::Composition) == ResourceMode::All;
            if biomes != 0 || subtypes != 0 {
                if all {
                    for (index, _) in Field::Biome.available_options() {
                        if biomes & (1 << index) != 0 {
                            plan.system.push(Predicate::Contains(
                                PlanetSelection::Biome {
                                    biomes: 1 << index,
                                    subtypes,
                                },
                                true,
                            ));
                        }
                    }
                    for (mask, _) in options {
                        if subtypes & mask != 0 {
                            plan.system.push(Predicate::Contains(
                                PlanetSelection::Biome {
                                    biomes,
                                    subtypes: subtypes & mask,
                                },
                                true,
                            ));
                        }
                    }
                } else {
                    plan.system.push(Predicate::Contains(
                        PlanetSelection::Biome { biomes, subtypes },
                        expected,
                    ));
                }
            }
            let sizes = self.composition.sizes.get(side);
            if sizes != 0 {
                if all {
                    for (index, _) in Field::Size.available_options() {
                        if sizes & (1 << index) != 0 {
                            plan.system
                                .push(Predicate::Contains(PlanetSelection::Size(1 << index), true));
                        }
                    }
                } else {
                    plan.system
                        .push(Predicate::Contains(PlanetSelection::Size(sizes), expected));
                }
            }
        }
        Ok(())
    }

    pub fn compile(&self) -> Result<Plan, &'static str> {
        let mut plan = Plan {
            system: Vec::new(),
            planet: Vec::new(),
            requirements: Requirements::default(),
            match_modes: std::array::from_fn(|index| self.match_mode(Section::ALL[index])),
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
                        | Field::SkyHeightFog
                        | Field::NightSky
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
            let predicate = Predicate::Choice(Choices {
                field,
                mask,
                mode: if field == Field::UniqueResources {
                    self.unique_resource_mode
                } else {
                    ResourceMode::Any
                },
            });
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
        if let Some(selection) = self.arena.compile(self.match_mode(Section::XenoArena))? {
            plan.planet.push(Predicate::Arena(selection));
        }
        for (&domain, criteria) in &self.fauna {
            if let Some(selection) = criteria.compile(domain, self.match_mode(domain.section()))? {
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
        if !ambiguous {
            if let Some(label) = label {
                if let Some((mask, _)) = options.iter_mut().find(|(_, name)| *name == label) {
                    *mask |= 1 << choice;
                } else {
                    options.push((1 << choice, label));
                }
            }
        }
    }
    options
}
