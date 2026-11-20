use crate::criteria::{
    Field, Plan, PlanetFlag, PlanetSelection, Predicate, Requirements, Section, Stage, SystemFlag,
    WaterPresence,
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Facts {
    values: [u32; Field::ALL.len()],
    known_values: u64,
    grass_candidates: Option<u32>,
    terrain_layers: Option<u32>,
    flora_colours_observed: u32,
    flora_colours_known: u32,
    #[serde(skip)]
    colour_evidence: BTreeMap<Field, crate::appearance::GeneratedColourAppearance>,
    #[serde(skip)]
    colour_components: BTreeMap<Field, BTreeMap<ColourComponent, ColourComponentObservation>>,
    #[serde(skip)]
    sky_reference: Option<SkyReferenceObservation>,
    planet_flags: u32,
    known_planet_flags: u32,
    system_flags: u16,
    known_system_flags: u16,
    resources: Option<u32>,
    planet_count: Option<u8>,
    star_count: Option<u8>,
    water: Option<WaterPresence>,
    extreme_storm_profile: Option<bool>,
    fauna: Option<std::sync::Arc<[crate::fauna::Species]>>,
    arena: Option<std::sync::Arc<[crate::arena::Creature]>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Match,
    NoMatch,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedValue {
    pub native: u32,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourProvenance {
    GeneratedReferenceAppearance,
    GrassMaterialAppearance,
    FloraModelAppearance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourComponent {
    SkyColour,
    SkySolarColour,
    SkyUpperColour,
    SunColour,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ColourComponentObservation {
    pub classification: u32,
    pub appearance: crate::appearance::GeneratedColourAppearance,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkyReferenceObservation {
    pub gradient_progress: f64,
    pub gradient_speed: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ColourObservation {
    pub classification: u32,
    pub labels: Vec<String>,
    pub evaluated_labels: Vec<String>,
    pub complete: bool,
    pub provenance: ColourProvenance,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appearance: Option<crate::appearance::GeneratedColourAppearance>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<ColourComponent, ColourComponentObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky_reference: Option<SkyReferenceObservation>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FactSnapshot {
    pub fields: BTreeMap<Field, ObservedValue>,
    pub colour_observations: BTreeMap<Field, ColourObservation>,
    pub planet_flags: BTreeMap<PlanetFlag, bool>,
    pub system_flags: BTreeMap<SystemFlag, bool>,
    pub resources: Option<Vec<String>>,
    pub planet_count: Option<u8>,
    pub star_count: Option<u8>,
    pub water: Option<WaterPresence>,
    pub fauna_count: Option<usize>,
    pub arena_count: Option<usize>,
}

impl From<&Facts> for FactSnapshot {
    fn from(facts: &Facts) -> Self {
        let fields = Field::ALL
            .iter()
            .filter_map(|&field| {
                facts.get(field).map(|native| {
                    (
                        field,
                        ObservedValue {
                            native,
                            labels: field
                                .labels_for_native(native)
                                .into_iter()
                                .map(str::to_owned)
                                .collect(),
                        },
                    )
                })
            })
            .collect();
        let colour_observations = Field::ALL
            .iter()
            .copied()
            .filter_map(|field| {
                field.colour_index()?;
                let (classification, known, complete, provenance) = match field {
                    Field::FloraColour => {
                        let classification = facts.get(field).unwrap_or(
                            crate::vocabulary::GRASS_COLOUR_SET | facts.flora_colours_observed,
                        );
                        if classification == crate::vocabulary::GRASS_COLOUR_SET
                            && facts.flora_colours_known == 0
                        {
                            return None;
                        }
                        (
                            classification,
                            if facts.get(field).is_some() {
                                (1 << field.options().len()) - 1
                            } else {
                                facts.flora_colours_known
                            },
                            facts.get(field).is_some(),
                            ColourProvenance::FloraModelAppearance,
                        )
                    }
                    Field::Grass => (
                        facts.get(field)?,
                        (1 << field.options().len()) - 1,
                        true,
                        ColourProvenance::GrassMaterialAppearance,
                    ),
                    _ => (
                        facts.get(field)?,
                        (1 << field.options().len()) - 1,
                        true,
                        ColourProvenance::GeneratedReferenceAppearance,
                    ),
                };
                let labels = field
                    .labels_for_native(classification)
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                let evaluated_labels = field
                    .options()
                    .iter()
                    .copied()
                    .enumerate()
                    .filter_map(|(index, label)| {
                        (known & (1 << index) != 0).then_some(label.into())
                    })
                    .collect();
                Some((
                    field,
                    ColourObservation {
                        classification,
                        labels,
                        evaluated_labels,
                        complete,
                        provenance,
                        appearance: facts.colour_evidence.get(&field).copied(),
                        components: facts
                            .colour_components
                            .get(&field)
                            .cloned()
                            .unwrap_or_default(),
                        sky_reference: (field == Field::Sky)
                            .then_some(facts.sky_reference)
                            .flatten(),
                    },
                ))
            })
            .collect();
        let planet_flags = PlanetFlag::ALL
            .iter()
            .filter_map(|&flag| facts.planet_flag(flag).map(|value| (flag, value)))
            .collect();
        let system_flags = SystemFlag::ALL
            .iter()
            .filter_map(|&flag| facts.system_flag(flag).map(|value| (flag, value)))
            .collect();
        let resources = facts.resources().map(|bits| {
            crate::vocabulary::RESOURCE_LABELS
                .iter()
                .copied()
                .enumerate()
                .filter(|(index, _)| bits & (1 << index) != 0)
                .map(|(_, label)| label.to_owned())
                .collect()
        });
        Self {
            fields,
            colour_observations,
            planet_flags,
            system_flags,
            resources,
            planet_count: facts.planet_count(),
            star_count: facts.star_count(),
            water: facts.water(),
            fauna_count: facts.fauna().map(<[_]>::len),
            arena_count: facts.arena().map(<[_]>::len),
        }
    }
}

impl Facts {
    pub fn set(&mut self, field: Field, value: u32) {
        self.values[field as usize] = value;
        self.known_values |= 1u64 << (field as usize);
    }
    pub fn restrict_grass_colours(&mut self, candidates: u32) {
        self.grass_candidates = Some(
            self.grass_candidates
                .map_or(candidates, |old| old & candidates),
        );
    }
    pub fn get(&self, field: Field) -> Option<u32> {
        (self.known_values & (1u64 << (field as usize)) != 0).then_some(self.values[field as usize])
    }
    pub fn clear(&mut self, field: Field) {
        self.known_values &= !(1u64 << (field as usize));
        self.values[field as usize] = 0;
    }
    pub fn observe_terrain_layers(&mut self, features: u32) {
        self.terrain_layers = Some(features);
    }
    pub fn terrain_layers(&self) -> Option<u32> {
        self.terrain_layers
    }
    pub fn observe_flora_colours(&mut self, colours: u32, known: u32) {
        let colours = colours & !crate::vocabulary::GRASS_COLOUR_SET;
        self.flora_colours_observed |= colours;
        self.flora_colours_known |= (known & !crate::vocabulary::GRASS_COLOUR_SET) | colours;
    }
    pub fn set_colour_evidence(
        &mut self,
        field: Field,
        evidence: crate::appearance::GeneratedColourAppearance,
    ) {
        self.colour_evidence.insert(field, evidence);
    }
    pub fn set_colour_component(
        &mut self,
        field: Field,
        component: ColourComponent,
        observation: ColourComponentObservation,
    ) {
        self.colour_components
            .entry(field)
            .or_default()
            .insert(component, observation);
    }
    pub fn set_sky_reference(&mut self, observation: SkyReferenceObservation) {
        self.sky_reference = Some(observation);
    }
    pub fn set_extreme_storm_profile(&mut self, value: bool) {
        self.extreme_storm_profile = Some(value);
    }
    pub fn extreme_storm_profile(&self) -> Option<bool> {
        self.extreme_storm_profile
    }
    pub fn clear_planet_flag(&mut self, flag: PlanetFlag) {
        let bit = 1u32 << (flag as usize);
        self.known_planet_flags &= !bit;
        self.planet_flags &= !bit;
    }
    pub fn set_planet_flag(&mut self, flag: PlanetFlag, value: bool) {
        let bit = 1u32 << (flag as usize);
        self.known_planet_flags |= bit;
        self.planet_flags = (self.planet_flags & !bit) | if value { bit } else { 0 };
    }
    pub fn planet_flag(&self, flag: PlanetFlag) -> Option<bool> {
        let bit = 1u32 << (flag as usize);
        (self.known_planet_flags & bit != 0).then_some(self.planet_flags & bit != 0)
    }
    pub fn set_system_flag(&mut self, flag: SystemFlag, value: bool) {
        let bit = 1u16 << (flag as usize);
        self.known_system_flags |= bit;
        self.system_flags = (self.system_flags & !bit) | if value { bit } else { 0 };
    }
    pub fn system_flag(&self, flag: SystemFlag) -> Option<bool> {
        let bit = 1u16 << (flag as usize);
        (self.known_system_flags & bit != 0).then_some(self.system_flags & bit != 0)
    }
    pub fn set_resources(&mut self, resources: u32) {
        self.resources = Some(resources);
    }
    pub fn resources(&self) -> Option<u32> {
        self.resources
    }
    pub fn set_water(&mut self, water: WaterPresence) {
        self.water = Some(water);
    }
    pub fn water(&self) -> Option<WaterPresence> {
        self.water
    }
    pub fn evaluate_selection(&self, selection: PlanetSelection) -> Verdict {
        match selection {
            PlanetSelection::Water(rule) => verdict(self.water.map(|water| rule.accepts(water))),
            PlanetSelection::Size(mask) => verdict(
                self.get(Field::Size)
                    .map(|size| size < 64 && mask & (1 << size) != 0),
            ),
            PlanetSelection::Biome { biomes, subtypes } => {
                let biome = if biomes == 0 {
                    Verdict::Match
                } else {
                    verdict(self.get(Field::Biome).map(|biome| {
                        biome < 16
                            && (biomes & (1 << biome) != 0
                                || ((8..=10).contains(&biome) && biomes & (1 << 16) != 0))
                    }))
                };
                let subtype = if subtypes == 0 {
                    Verdict::Match
                } else {
                    verdict(self.get(Field::Subtype).map(|value| {
                        (1..=64).contains(&value) && subtypes & (1 << (value - 1)) != 0
                    }))
                };
                all([biome, subtype])
            }
        }
    }
    pub fn set_planet_count(&mut self, count: u8) {
        self.planet_count = Some(count);
    }
    pub fn planet_count(&self) -> Option<u8> {
        self.planet_count
    }
    pub fn set_star_count(&mut self, count: u8) {
        self.star_count = Some(count);
    }
    pub fn star_count(&self) -> Option<u8> {
        self.star_count
    }

    pub fn set_fauna(&mut self, species: Vec<crate::fauna::Species>) {
        self.fauna = Some(species.into());
    }
    pub fn fauna(&self) -> Option<&[crate::fauna::Species]> {
        self.fauna.as_deref()
    }
    pub fn set_arena(&mut self, creatures: Vec<crate::arena::Creature>) {
        self.arena = Some(creatures.into());
    }
    pub fn arena(&self) -> Option<&[crate::arena::Creature]> {
        self.arena.as_deref()
    }
    pub fn merge(&mut self, new: &Self) {
        if let Some(arena) = &new.arena {
            self.arena = Some(arena.clone());
        }
        if let Some(fauna) = &new.fauna {
            self.fauna = Some(fauna.clone());
        }
        if let Some(profile) = new.extreme_storm_profile {
            self.extreme_storm_profile = Some(profile);
        }
        if let Some(water) = new.water {
            self.water = Some(water);
        }
        self.observe_flora_colours(new.flora_colours_observed, new.flora_colours_known);
        self.colour_evidence.extend(
            new.colour_evidence
                .iter()
                .map(|(&field, &value)| (field, value)),
        );
        for (&field, components) in &new.colour_components {
            self.colour_components
                .entry(field)
                .or_default()
                .extend(components);
        }
        if let Some(reference) = new.sky_reference {
            self.sky_reference = Some(reference);
        }
        if let Some(features) = new.terrain_layers {
            self.observe_terrain_layers(features);
        }
        if let Some(candidates) = new.grass_candidates {
            self.restrict_grass_colours(candidates);
        }
        for &field in Field::ALL {
            if let Some(value) = new.get(field) {
                self.set(field, value);
            }
        }
        for &flag in PlanetFlag::ALL {
            if let Some(value) = new.planet_flag(flag) {
                self.set_planet_flag(flag, value);
            }
        }
        for &flag in SystemFlag::ALL {
            if let Some(value) = new.system_flag(flag) {
                self.set_system_flag(flag, value);
            }
        }
        if let Some(value) = new.resources {
            self.resources = Some(value);
        }
        if let Some(value) = new.planet_count {
            self.planet_count = Some(value);
        }
        if let Some(value) = new.star_count {
            self.star_count = Some(value);
        }
    }

    pub fn evaluate(&self, predicate: Predicate) -> Verdict {
        let accepted = match predicate {
            Predicate::Arena(selection) => {
                return self
                    .arena()
                    .map_or(Verdict::Unknown, |creatures| selection.evaluate(creatures));
            }
            Predicate::Fauna(selection) => self
                .fauna
                .as_deref()
                .map(|species| selection.evaluate(species) == Verdict::Match),
            Predicate::Water(rule) => self.water.map(|water| rule.accepts(water)),
            Predicate::Contains(_, _) => None,
            Predicate::Choice(choices) if choices.field() == Field::Economy => {
                return all([
                    verdict(
                        self.get(Field::Population)
                            .map(|population| population == 0),
                    ),
                    verdict(self.system_flag(SystemFlag::Pirate).map(|pirate| !pirate)),
                    verdict(self.get(Field::Economy).map(|value| choices.accepts(value))),
                ]);
            }
            Predicate::Choice(choices)
                if matches!(choices.field(), Field::Wealth | Field::ConflictLevel) =>
            {
                return all([
                    verdict(
                        self.get(Field::Population)
                            .map(|population| population == 0),
                    ),
                    verdict(
                        self.get(choices.field())
                            .map(|value| choices.accepts(value)),
                    ),
                ]);
            }
            Predicate::Choice(choices) if choices.field() == Field::WaterColour => {
                return all([
                    verdict(self.water.map(|water| water != WaterPresence::Dry)),
                    verdict(
                        self.get(Field::WaterColour)
                            .map(|value| choices.accepts(value)),
                    ),
                ]);
            }
            Predicate::Choice(choices) if choices.field() == Field::Terrain => self
                .get(Field::Terrain)
                .map(|value| choices.accepts(value))
                .or_else(|| {
                    self.terrain_layers.and_then(|features| {
                        if choices.accepts(crate::terrain::FEATURE_SET | features) {
                            Some(true)
                        } else if choices.mask() & u64::from(crate::terrain::FLOATING_ISLANDS) == 0
                        {
                            Some(false)
                        } else {
                            None
                        }
                    })
                }),
            Predicate::Choice(choices) if choices.field() == Field::Race => self
                .get(Field::Population)
                .and_then(|population| match population {
                    1 => Some(choices.accepts(7)),
                    0 | 2 => self
                        .get(Field::Race)
                        .map(|race| race != 7 && choices.accepts(race)),
                    _ => None,
                }),
            Predicate::Choice(choices) if choices.field() == Field::Grass => self
                .get(Field::Grass)
                .map(|value| choices.accepts(value))
                .or_else(|| {
                    self.grass_candidates
                        .and_then(|candidates| (!choices.accepts(candidates)).then_some(false))
                }),
            Predicate::Choice(choices) if choices.field() == Field::FloraColour => self
                .get(Field::FloraColour)
                .map(|value| choices.accepts(value))
                .or_else(|| {
                    if choices
                        .accepts(crate::vocabulary::GRASS_COLOUR_SET | self.flora_colours_observed)
                    {
                        Some(true)
                    } else if choices.mask() & !u64::from(self.flora_colours_known) == 0 {
                        Some(false)
                    } else {
                        None
                    }
                }),
            Predicate::Choice(choices) => self
                .get(choices.field())
                .map(|value| choices.accepts(value)),
            Predicate::Planet(flag, expected) => {
                self.planet_flag(flag).map(|value| value == expected)
            }
            Predicate::System(flag, expected) => {
                self.system_flag(flag).map(|value| value == expected)
            }
            Predicate::Resources(rule) => self.resources.map(|value| rule.accepts(value)),
            Predicate::MinimumPlanets(minimum) => self.planet_count.map(|count| count >= minimum),
            Predicate::MinimumStars(minimum) => self.star_count.map(|count| count >= minimum),
        };
        match accepted {
            Some(true) => Verdict::Match,
            Some(false) => Verdict::NoMatch,
            None => Verdict::Unknown,
        }
    }
}

fn verdict(accepted: Option<bool>) -> Verdict {
    match accepted {
        Some(true) => Verdict::Match,
        Some(false) => Verdict::NoMatch,
        None => Verdict::Unknown,
    }
}
pub(crate) fn all(values: impl IntoIterator<Item = Verdict>) -> Verdict {
    let mut result = Verdict::Match;
    for value in values {
        if value == Verdict::NoMatch {
            return Verdict::NoMatch;
        }
        if value == Verdict::Unknown {
            result = Verdict::Unknown;
        }
    }
    result
}

pub(crate) fn any(values: impl IntoIterator<Item = Verdict>) -> Verdict {
    let mut result = Verdict::NoMatch;
    for value in values {
        if value == Verdict::Match {
            return Verdict::Match;
        }
        if value == Verdict::Unknown {
            result = Verdict::Unknown;
        }
    }
    result
}

impl Plan {
    pub fn targets_animals(&self) -> bool {
        self.planet.iter().any(|predicate| match predicate {
            Predicate::Fauna(selection) => selection.targets_animals(),
            Predicate::Arena(selection) => selection.targets_animals(),
            _ => false,
        })
    }

    /// Concrete generated animals that satisfy the selected fauna sections on an otherwise
    /// matching planet. Negative-only fauna rules intentionally yield no destination animal.
    pub fn fauna_witnesses<'a>(&self, facts: &'a Facts) -> Vec<&'a crate::fauna::Species> {
        let Some(species) = facts.fauna() else {
            return Vec::new();
        };
        let mut witnesses = Vec::new();
        for selection in self.planet.iter().filter_map(|predicate| match predicate {
            Predicate::Fauna(selection) => Some(*selection),
            _ => None,
        }) {
            for animal in selection.witnesses(species) {
                if !witnesses
                    .iter()
                    .any(|previous: &&crate::fauna::Species| previous.role == animal.role)
                {
                    witnesses.push(animal);
                }
            }
        }
        witnesses.sort_by_key(|animal| animal.role);
        witnesses
    }

    /// Concrete generated animals that satisfy every positive Arena selection. Arena facts retain
    /// role indices so their ranks can be joined to the discovery names captured for the planet.
    pub fn arena_witnesses<'a>(&self, facts: &'a Facts) -> Vec<&'a crate::fauna::Species> {
        let (Some(creatures), Some(species)) = (facts.arena(), facts.fauna()) else {
            return Vec::new();
        };
        let mut roles: Option<Vec<u8>> = None;
        for selection in self.planet.iter().filter_map(|predicate| match predicate {
            Predicate::Arena(selection) if selection.targets_animals() => Some(*selection),
            _ => None,
        }) {
            let current: Vec<_> = selection
                .witnesses(creatures)
                .into_iter()
                .map(|creature| creature.role)
                .collect();
            if let Some(roles) = &mut roles {
                roles.retain(|role| current.contains(role));
            } else {
                roles = Some(current);
            }
        }
        let Some(roles) = roles else {
            return Vec::new();
        };
        species
            .iter()
            .filter(|animal| roles.contains(&animal.role))
            .collect()
    }

    pub fn animal_witnesses<'a>(&self, facts: &'a Facts) -> Vec<&'a crate::fauna::Species> {
        let fauna_selected = self.planet.iter().any(|predicate| {
            matches!(predicate, Predicate::Fauna(selection) if selection.targets_animals())
        });
        let arena_selected = self.planet.iter().any(|predicate| {
            matches!(predicate, Predicate::Arena(selection) if selection.targets_animals())
        });
        let mut fauna = self.fauna_witnesses(facts);
        let arena = self.arena_witnesses(facts);
        match (fauna_selected, arena_selected) {
            (true, true) => {
                fauna.retain(|animal| arena.iter().any(|other| other.role == animal.role));
                fauna
            }
            (true, false) => fauna,
            (false, true) => arena,
            (false, false) => Vec::new(),
        }
    }

    pub fn missing_planet_requirements(&self, facts: &Facts) -> Requirements {
        let mut needed = Requirements::default();
        let sections = self.section_verdicts(&self.planet, |rule| facts.evaluate(rule));
        for &predicate in &self.planet {
            // An acquired Arena observation may have unresolved placement/size-dependent ranks.
            // Regenerating the same roles cannot supply those inputs.
            if matches!(predicate, Predicate::Arena(_)) && facts.arena().is_some() {
                continue;
            }
            if sections[predicate.section() as usize] != Verdict::Unknown
                || facts.evaluate(predicate) != Verdict::Unknown
            {
                continue;
            }
            Self::include_requirements(predicate, &mut needed);
        }
        needed
    }

    pub fn inspection_requirements(&self, facts: &Facts) -> Requirements {
        let mut needed = Requirements {
            screen_filter: facts.get(Field::WorldColourFilter).is_none(),
            ..Requirements::default()
        };
        for &predicate in &self.planet {
            if matches!(predicate, Predicate::Arena(_)) && facts.arena().is_some() {
                continue;
            }
            if facts.evaluate(predicate) == Verdict::Unknown {
                Self::include_requirements(predicate, &mut needed);
            }
        }
        needed
    }

    fn include_requirements(predicate: Predicate, needed: &mut Requirements) {
        needed.planet_query |= predicate.stage() >= Stage::Query;
        needed.metadata |= predicate.stage() == Stage::Metadata;
        needed.hints |= predicate.stage() == Stage::Hints;
        needed.fauna |= predicate.stage() == Stage::Fauna;
        needed.weather |= predicate.stage() == Stage::Weather;
        needed.terrain |= predicate.stage() == Stage::Terrain;
        needed.screen_filter |= predicate.stage() == Stage::ScreenFilter;
        needed.objects |= predicate.stage() == Stage::Objects;
        if let Predicate::Choice(choices) = predicate {
            needed.objects |= choices.field() == Field::Terrain
                && choices.mask() & u64::from(crate::terrain::FLOATING_ISLANDS) != 0;
            if let Some(index) = choices.field().colour_index() {
                needed.colours |= 1 << index;
            }
        }
    }
    fn section_verdicts(
        &self,
        predicates: &[Predicate],
        evaluate: impl Fn(Predicate) -> Verdict,
    ) -> [Verdict; Section::ALL.len()] {
        let mut results = [Verdict::Match; Section::ALL.len()];
        for &rule in predicates {
            let index = rule.section() as usize;
            if results[index] != Verdict::NoMatch {
                results[index] = all([results[index], evaluate(rule)]);
            }
        }
        results
    }
    pub fn planet_predicate_verdict(&self, predicate: Predicate, facts: &Facts) -> Verdict {
        facts.evaluate(predicate)
    }
    pub fn system_predicate_verdict(
        &self,
        predicate: Predicate,
        facts: &Facts,
        planets: &[Facts],
    ) -> Verdict {
        if let Predicate::Contains(selection, expected) = predicate {
            let found = if planets.is_empty() {
                Verdict::Unknown
            } else {
                any(planets
                    .iter()
                    .map(|planet| planet.evaluate_selection(selection)))
            };
            match (found, expected) {
                (Verdict::Match, false) => Verdict::NoMatch,
                (Verdict::NoMatch, false) => Verdict::Match,
                _ => found,
            }
        } else {
            facts.evaluate(predicate)
        }
    }
    pub fn planet_section_verdicts(&self, facts: &Facts) -> BTreeMap<Section, Verdict> {
        Section::ALL
            .iter()
            .copied()
            .zip(self.section_verdicts(&self.planet, |rule| facts.evaluate(rule)))
            .collect()
    }
    pub fn system_section_verdicts(
        &self,
        facts: &Facts,
        planets: &[Facts],
    ) -> BTreeMap<Section, Verdict> {
        Section::ALL
            .iter()
            .copied()
            .zip(self.section_verdicts(&self.system, |rule| {
                self.system_predicate_verdict(rule, facts, planets)
            }))
            .collect()
    }
    pub fn system_match(&self, facts: &Facts, planets: &[Facts]) -> Verdict {
        all(self.section_verdicts(&self.system, |rule| {
            self.system_predicate_verdict(rule, facts, planets)
        }))
    }
    pub fn planet_match(&self, facts: &Facts) -> Verdict {
        all(self.section_verdicts(&self.planet, |rule| facts.evaluate(rule)))
    }

    /// Identify an undecided Arena condition after acquisition, with every other section matched.
    /// This does not establish a match or exclusion, or authorize skipping the unresolved planet.
    pub fn unresolved_arena_ranks(&self, facts: &Facts) -> bool {
        if !facts
            .arena()
            .is_some_and(|creatures| creatures.iter().any(|c| !c.ranks.is_complete()))
        {
            return false;
        }
        let sections = self.section_verdicts(&self.planet, |rule| facts.evaluate(rule));
        sections[Section::XenoArena as usize] == Verdict::Unknown
            && sections.iter().enumerate().all(|(index, &verdict)| {
                index == Section::XenoArena as usize || verdict == Verdict::Match
            })
    }
}

pub fn hue(rgb: [f64; 3]) -> Result<u32, &'static str> {
    let (maximum, range) = colour_range(rgb)?;
    if maximum == 0.0 || range / maximum < 0.12 {
        return Ok(8);
    }
    Ok(hue_sector(rgb, maximum, range))
}

pub fn chromatic_hue(rgb: [f64; 3]) -> Result<u32, &'static str> {
    let (maximum, range) = colour_range(rgb)?;
    if range <= f64::EPSILON {
        return Ok(8);
    }
    Ok(hue_sector(rgb, maximum, range))
}

fn colour_range(rgb: [f64; 3]) -> Result<(f64, f64), &'static str> {
    if !rgb.iter().all(|value| value.is_finite() && *value >= 0.0) {
        return Err("invalid colour components");
    }
    let [red, green, blue] = rgb;
    let maximum = red.max(green).max(blue);
    let minimum = red.min(green).min(blue);
    Ok((maximum, maximum - minimum))
}

fn hue_sector(rgb: [f64; 3], maximum: f64, range: f64) -> u32 {
    let [red, green, blue] = rgb;
    let rc = (maximum - red) / range;
    let gc = (maximum - green) / range;
    let bc = (maximum - blue) / range;
    let h = if red == maximum {
        bc - gc
    } else if green == maximum {
        2.0 + rc - bc
    } else {
        4.0 + gc - rc
    };
    let degrees = (h / 6.0).rem_euclid(1.0) * 360.0;
    if !(15.0..345.0).contains(&degrees) {
        0
    } else if degrees < 45.0 {
        1
    } else if degrees <= 60.0 {
        2
    } else if degrees < 165.0 {
        3
    } else if degrees < 195.0 {
        4
    } else if degrees < 255.0 {
        5
    } else if degrees < 285.0 {
        6
    } else {
        7
    }
}
