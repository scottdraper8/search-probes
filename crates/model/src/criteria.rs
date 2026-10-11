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
    Rings => ("Rings",Input),
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
    VileBrood => ("Vile Brood",Hints),
    Sandworms => ("Sandworms",Fauna),
    TitanWorms => ("Titan worms",Objects),
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
    Fauna,
    AquaticFauna,
    LandFauna,
    AirborneFauna,
    Features,
    Resources,
    Type,
    Weather,
}
impl Section {
    pub fn parent(self) -> Option<Self> {
        match self {
            Self::AquaticFauna | Self::LandFauna | Self::AirborneFauna => Some(Self::Fauna),
            _ => None,
        }
    }
    pub const ALL: &[Self] = &[
        Self::Composition,
        Self::Profile,
        Self::Environment,
        Self::Colours,
        Self::Conditions,
        Self::Fauna,
        Self::AquaticFauna,
        Self::LandFauna,
        Self::AirborneFauna,
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
            Self::Fauna => Section::Fauna,
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
            _ => Section::Fauna,
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
}

impl Predicate {
    pub fn section(self) -> Section {
        match self {
            Self::Fauna(selection) => selection.domain.section(),
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
            Self::Fauna(_) => Stage::Fauna,
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

#[cfg(test)]
mod tests {
    #[test]
    fn unique_resource_alternatives_and_brood_are_independent() {
        use crate::facts::{Facts, Verdict};
        let mut criteria = Criteria::default();
        criteria.unique_resource_mode = ResourceMode::Any;
        criteria.choices.insert(Field::UniqueResources, 1 | 4);
        criteria.planets.insert(PlanetFlag::VileBrood, false);
        let plan = criteria.compile().unwrap();
        assert_eq!(
            plan.requirements,
            Requirements {
                planet_query: true,
                hints: true,
                ..Default::default()
            }
        );
        let mut facts = Facts::default();
        assert_eq!(plan.planet_match(&facts), Verdict::Unknown);
        for (resources, expected) in [
            (0, Verdict::NoMatch),
            (1, Verdict::Match),
            (2, Verdict::NoMatch),
            (4, Verdict::Match),
            (6, Verdict::Match),
        ] {
            facts.set(Field::UniqueResources, UNIQUE_RESOURCE_SET | resources);
            facts.set_planet_flag(PlanetFlag::VileBrood, false);
            assert_eq!(plan.planet_match(&facts), expected);
            assert!(!plan.missing_planet_requirements(&facts).hints);
        }
        facts.set_planet_flag(PlanetFlag::VileBrood, true);
        assert_eq!(plan.planet_match(&facts), Verdict::NoMatch);
        let mut cached = Facts::default();
        cached.merge(&facts);
        assert_eq!(plan.planet_match(&cached), Verdict::NoMatch);
        for choice in 0..3 {
            assert_eq!(
                Field::UniqueResources.native_value(choice),
                Some(UNIQUE_RESOURCE_SET | 1 << choice)
            );
        }
        criteria.choices.insert(Field::UniqueResources, 8);
        assert!(criteria.compile().is_err());
    }
    #[test]
    fn unique_resource_modes_match_same_planet_and_survive_cached_facts() {
        use crate::facts::{Facts, Verdict};
        let mut criteria = Criteria::default();
        assert_eq!(criteria.unique_resource_mode, ResourceMode::All);
        criteria.choices.insert(Field::UniqueResources, 2 | 4);
        let all = criteria.compile().unwrap();
        criteria.unique_resource_mode = ResourceMode::Any;
        let any = criteria.compile().unwrap();
        // Continuation keys must distinguish conjunctions from alternatives.
        assert_ne!(all.planet, any.planet);
        assert_eq!(all.requirements, any.requirements);
        let mut facts = Facts::default();
        for plan in [&all, &any] {
            assert_eq!(plan.planet_match(&facts), Verdict::Unknown);
            assert!(plan.missing_planet_requirements(&facts).hints);
        }
        for actual in 0..8 {
            facts.set(Field::UniqueResources, UNIQUE_RESOURCE_SET | actual);
            let mut cached = Facts::default();
            cached.merge(&facts);
            // Explicit expected sets include empty, unrelated and compound resources.
            let expected_all = if matches!(actual, 6 | 7) {
                Verdict::Match
            } else {
                Verdict::NoMatch
            };
            let expected_any = if matches!(actual, 2..=7) {
                Verdict::Match
            } else {
                Verdict::NoMatch
            };
            assert_eq!(all.planet_match(&cached), expected_all);
            assert_eq!(any.planet_match(&cached), expected_any);
            assert!(!all.missing_planet_requirements(&cached).hints);
        }
    }

    #[test]
    fn resource_match_controls_are_independent_and_persist() {
        use crate::facts::{Facts, Verdict};
        for unique_mode in [ResourceMode::All, ResourceMode::Any] {
            for ordinary_mode in [ResourceMode::All, ResourceMode::Any] {
                let mut criteria = Criteria {
                    unique_resource_mode: unique_mode,
                    resources: Resources {
                        selected: 1 | 2,
                        mode: ordinary_mode,
                    },
                    ..Default::default()
                };
                criteria.choices.insert(Field::UniqueResources, 2 | 4);
                let saved = serde_json::to_string(&criteria).unwrap();
                let restored: Criteria = serde_json::from_str(&saved).unwrap();
                assert_eq!(restored, criteria);
                let plan = restored.compile().unwrap();
                let mut facts = Facts::default();
                facts.set(Field::UniqueResources, UNIQUE_RESOURCE_SET | 2);
                facts.set_resources(1);
                assert_eq!(
                    plan.planet_match(&facts),
                    if unique_mode == ResourceMode::Any && ordinary_mode == ResourceMode::Any {
                        Verdict::Match
                    } else {
                        Verdict::NoMatch
                    }
                );
            }
        }
        let mut criteria = Criteria::default();
        let all = criteria.compile().unwrap();
        criteria.unique_resource_mode = ResourceMode::Any;
        let any = criteria.compile().unwrap();
        assert_eq!(all.planet, any.planet);
        assert_eq!(any.requirements, Requirements::default());
    }

    use super::*;

    #[test]
    fn player_attacking_fauna_is_selective_and_unknown_until_observed() {
        use crate::facts::{Facts, Verdict};
        assert_eq!(
            Criteria::default().compile().unwrap().requirements,
            Requirements::default()
        );
        for required in [true, false] {
            let mut criteria = Criteria::default();
            criteria
                .planets
                .insert(PlanetFlag::PlayerAttackingFauna, required);
            let plan = criteria.compile().unwrap();
            assert_eq!(
                plan.requirements,
                Requirements {
                    planet_query: true,
                    fauna: true,
                    ..Requirements::default()
                }
            );
            let mut facts = Facts::default();
            assert_eq!(plan.planet_match(&facts), Verdict::Unknown);
            assert!(plan.missing_planet_requirements(&facts).fauna);
            facts.set_planet_flag(PlanetFlag::PlayerAttackingFauna, !required);
            assert_eq!(plan.planet_match(&facts), Verdict::NoMatch);
            facts.set_planet_flag(PlanetFlag::PlayerAttackingFauna, required);
            assert_eq!(plan.planet_match(&facts), Verdict::Match);
            assert_eq!(
                plan.missing_planet_requirements(&facts),
                Requirements::default()
            );
            let encoded = serde_json::to_string(&criteria).unwrap();
            assert_eq!(
                serde_json::from_str::<Criteria>(&encoded).unwrap(),
                criteria
            );
        }
    }

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
    fn subtype_labels_follow_the_selected_biome_and_group_equivalent_formations() {
        let mut criteria = Criteria::default();
        assert!(
            !criteria
                .subtype_options()
                .iter()
                .any(|(mask, _)| mask & (1 << 6) != 0)
        );
        criteria.set_biome_choices(1 << 0);
        let options = criteria.subtype_options();
        assert!(options.contains(&(1 << 6, "Bubble landscape")));
        assert!(options.contains(&(1 << 19, "Fan mushrooms")));
        assert!(options.contains(&((1 << 12) | (1 << 21) | (1 << 22), "Rock formations")));
        criteria.choices.insert(Field::Subtype, 1 << 19);
        criteria.set_biome_choices(1 << 2);
        assert!(!criteria.choices.contains_key(&Field::Subtype));
        assert!(
            criteria
                .subtype_options()
                .contains(&(1 << 19, "Shield trees"))
        );
        criteria.set_biome_choices(1 << 7);
        assert!(
            criteria
                .subtype_options()
                .contains(&(1 << 6, "Bubble formations"))
        );
        criteria.set_biome_choices((1 << 0) | (1 << 7));
        assert!(
            !criteria
                .subtype_options()
                .iter()
                .any(|(mask, _)| mask & (1 << 6) != 0)
        );
    }

    #[test]
    fn contextual_subtypes_match_only_their_native_archetypes() {
        for (biome, _) in Field::Biome.available_options() {
            let mut criteria = Criteria::default();
            criteria.set_biome_choices(1 << biome);
            for (mask, _) in criteria.subtype_options() {
                criteria.choices.insert(Field::Subtype, mask);
                let plan = criteria.compile().unwrap();
                let rule = plan
                    .planet
                    .iter()
                    .find_map(|predicate| match predicate {
                        Predicate::Choice(rule) if rule.field == Field::Subtype => Some(rule),
                        _ => None,
                    })
                    .unwrap();
                for native in 0..=32 {
                    assert_eq!(
                        rule.accepts(native),
                        native > 0 && native <= 31 && mask & (1 << (native - 1)) != 0
                    );
                }
            }
        }
        let mut criteria = Criteria::default();
        criteria.choices.insert(Field::Subtype, 1 << 6);
        assert_eq!(
            criteria.compile().unwrap_err(),
            "unsupported criterion choice"
        );
        criteria.set_biome_choices(1 << 0);
        criteria.choices.insert(Field::Subtype, 1 << 3);
        assert_eq!(
            criteria.compile().unwrap_err(),
            "unsupported criterion choice"
        );
    }

    #[test]
    fn natural_planet_options_preserve_native_values() {
        for (field, expected) in [
            (Field::Flora, vec![0, 2, 3]),
            (Field::Fauna, vec![0, 2, 3]),
            (Field::StormType, (0..9).collect()),
            (Field::StormFrequency, vec![0, 1, 2]),
        ] {
            let options = field.available_options().collect::<Vec<_>>();
            assert_eq!(
                options.iter().map(|&(index, _)| index).collect::<Vec<_>>(),
                expected
            );
            for (choice, _) in options {
                let mut criteria = Criteria::default();
                criteria.choices.insert(field, 1 << choice);
                let Predicate::Choice(rule) = criteria.compile().unwrap().planet[0] else {
                    panic!()
                };
                let native = field.native_value(choice).unwrap();
                for actual in 0..=field.options().len() as u32 {
                    assert_eq!(rule.accepts(actual), actual == native);
                }
            }
        }
    }

    #[test]
    fn unreachable_planet_choices_cannot_start_a_search() {
        for (field, choices) in [
            (Field::Flora, vec![1]),
            (Field::Fauna, vec![1]),
            (Field::Subtype, vec![29, 30]),
        ] {
            for choice in choices {
                for mask in [1 << choice, (1 << choice) | 1] {
                    let mut criteria = Criteria::default();
                    criteria.choices.insert(field, mask);
                    assert_eq!(
                        criteria.compile().unwrap_err(),
                        "unsupported criterion choice"
                    );
                }
            }
        }
    }

    #[test]
    fn appearance_controls_share_classes_and_keep_capture_bindings_distinct() {
        for field in Field::ALL
            .iter()
            .copied()
            .filter(|field| field.colour_index().is_some())
        {
            assert_eq!(field.options(), GRASS_COLOURS);
            assert!(!field.options().contains(&"Neutral"));
            let mut criteria = Criteria::default();
            criteria.choices.insert(field, 1 << 10);
            let plan = criteria.compile().unwrap();
            let mut facts = crate::facts::Facts::default();
            facts.set(field, 10);
            assert_eq!(plan.planet_match(&facts), crate::facts::Verdict::NoMatch);
            facts.set(field, GRASS_COLOUR_SET | (1 << 3));
            assert_eq!(plan.planet_match(&facts), crate::facts::Verdict::NoMatch);
            facts.set(field, GRASS_COLOUR_SET | (1 << 10));
            if field == Field::WaterColour {
                facts.set_water(WaterPresence::Shallow);
            }
            assert_eq!(plan.planet_match(&facts), crate::facts::Verdict::Match);
        }
        assert_eq!(Field::FloraColour.colour_index(), Some(1));
        assert_eq!(Field::Cloud.colour_index(), Some(3));
        assert_eq!(Field::WaterColour.colour_index(), Some(9));
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
            r#"{"choices":{"biome":1},"planets":{"sentinels":false,"extreme_sentinels":true,"rings":true}}"#
        ).unwrap();
        assert_eq!(criteria.sentinel_choices(), None);
        criteria.set_sentinel_choices(1 << 3);
        assert_eq!(criteria.sentinel_choices(), Some(1 << 3));
        assert_eq!(
            criteria.planets,
            BTreeMap::from([(PlanetFlag::Rings, true)])
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
            serde_json::from_str(r#"{"choices":{"biome":1},"planets":{"rgb":true,"rings":true}}"#)
                .unwrap();
        assert_eq!(conflicting.biome_choices(), None);
        conflicting.set_biome_choices(0);
        assert_eq!(conflicting.biome_choices(), Some(0));
        assert_eq!(
            conflicting.planets,
            BTreeMap::from([(PlanetFlag::Rings, true)])
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
        criteria.choices.insert(Field::StormType, 1);
        let plan = criteria.compile().unwrap();
        assert!(plan.requirements.weather);
        assert!(!plan.requirements.metadata);
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
                mask: 1,
                mode: ResourceMode::Any
            }
            .accepts(1)
        );
        assert!(
            !Choices {
                field: Field::Subtype,
                mask: 1,
                mode: ResourceMode::Any
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
        criteria
            .choices
            .insert(Field::Terrain, u64::from(FLOATING_ISLANDS));
        let text = serde_json::to_string(&criteria).unwrap();
        assert_eq!(serde_json::from_str::<Criteria>(&text).unwrap(), criteria);
        assert!(serde_json::from_str::<Criteria>(r#"{"unknown":true}"#).is_err());
    }
}
