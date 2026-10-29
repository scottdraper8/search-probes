use crate::criteria::{
    Field, Plan, PlanetFlag, PlanetSelection, Predicate, Requirements, ResourceMode, Section,
    Stage, SystemFlag, WaterPresence,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Facts {
    values: [u32; Field::ALL.len()],
    known_values: u64,
    grass_candidates: Option<u32>,
    terrain_layers: Option<u32>,
    flora_colours_observed: u32,
    flora_colours_known: u32,
    planet_flags: u32,
    known_planet_flags: u32,
    system_flags: u16,
    known_system_flags: u16,
    resources: Option<u32>,
    planet_count: Option<u8>,
    water: Option<WaterPresence>,
    extreme_storm_profile: Option<bool>,
    fauna: Option<std::sync::Arc<[crate::fauna::Species]>>,
    arena: Option<std::sync::Arc<[crate::arena::Creature]>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Match,
    NoMatch,
    Unknown,
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
                combine([biome, subtype], ResourceMode::All)
            }
        }
    }
    pub fn set_planet_count(&mut self, count: u8) {
        self.planet_count = Some(count);
    }
    pub fn planet_count(&self) -> Option<u8> {
        self.planet_count
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
                return combine(
                    [
                        verdict(self.system_flag(SystemFlag::Pirate).map(|pirate| !pirate)),
                        verdict(self.get(Field::Economy).map(|value| choices.accepts(value))),
                    ],
                    ResourceMode::All,
                );
            }
            Predicate::Choice(choices) if choices.field() == Field::WaterColour => {
                return combine(
                    [
                        verdict(self.water.map(|water| water != WaterPresence::Dry)),
                        verdict(
                            self.get(Field::WaterColour)
                                .map(|value| choices.accepts(value)),
                        ),
                    ],
                    ResourceMode::All,
                );
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
fn combine(values: impl IntoIterator<Item = Verdict>, mode: ResourceMode) -> Verdict {
    let mut result = if mode == ResourceMode::All {
        Verdict::Match
    } else {
        Verdict::NoMatch
    };
    let mut count = 0;
    for value in values {
        count += 1;
        if mode == ResourceMode::All && value == Verdict::NoMatch {
            return Verdict::NoMatch;
        }
        if mode == ResourceMode::Any && value == Verdict::Match {
            return Verdict::Match;
        }
        if value == Verdict::Unknown {
            result = Verdict::Unknown;
        }
    }
    if count == 0 { Verdict::Match } else { result }
}

impl Plan {
    pub fn missing_planet_requirements(&self, facts: &Facts) -> Requirements {
        let mut needed = Requirements::default();
        let sections =
            self.section_verdicts(&self.planet, |rule| self.evaluate_planet(rule, facts));
        for &predicate in &self.planet {
            // An acquired Arena observation may have unresolved placement/size-dependent ranks.
            // Regenerating the same roles cannot supply those inputs.
            if matches!(predicate, Predicate::Arena(_)) && facts.arena().is_some() {
                continue;
            }
            if sections[predicate.section() as usize] != Verdict::Unknown
                || self.evaluate_planet(predicate, facts) != Verdict::Unknown
            {
                continue;
            }
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
        needed
    }
    fn section_verdicts(
        &self,
        predicates: &[Predicate],
        evaluate: impl Fn(Predicate) -> Verdict,
    ) -> [Verdict; Section::ALL.len()] {
        let mut results = std::array::from_fn(|index| {
            if self.match_modes[index] == ResourceMode::All {
                Verdict::Match
            } else {
                Verdict::NoMatch
            }
        });
        let mut present = [false; Section::ALL.len()];
        for &rule in predicates {
            let index = rule.section() as usize;
            let mode = self.match_modes[index];
            if present[index]
                && ((mode == ResourceMode::All && results[index] == Verdict::NoMatch)
                    || (mode == ResourceMode::Any && results[index] == Verdict::Match))
            {
                continue;
            }
            present[index] = true;
            let value = evaluate(rule);
            if (mode == ResourceMode::All && value == Verdict::NoMatch)
                || (mode == ResourceMode::Any && value == Verdict::Match)
            {
                results[index] = value;
            } else if value == Verdict::Unknown {
                results[index] = Verdict::Unknown;
            }
        }
        for index in 0..present.len() {
            if !present[index] {
                results[index] = Verdict::Match;
            }
        }
        results
    }
    fn evaluate_planet(&self, predicate: Predicate, facts: &Facts) -> Verdict {
        if matches!(predicate, Predicate::Choice(choice) if matches!(choice.field(), Field::Biome | Field::Subtype))
        {
            // The subtype describes the selected biome on this same planet.
            return combine(self.planet.iter().copied().filter(|rule| matches!(rule, Predicate::Choice(choice) if matches!(choice.field(), Field::Biome | Field::Subtype))).map(|rule| facts.evaluate(rule)), ResourceMode::All);
        }
        facts.evaluate(predicate)
    }
    pub fn system_match(&self, facts: &Facts, planets: &[Facts]) -> Verdict {
        combine(
            self.section_verdicts(&self.system, |rule| {
                if let Predicate::Contains(selection, expected) = rule {
                    let found = if planets.is_empty() {
                        Verdict::Unknown
                    } else {
                        combine(
                            planets
                                .iter()
                                .map(|planet| planet.evaluate_selection(selection)),
                            ResourceMode::Any,
                        )
                    };
                    match (found, expected) {
                        (Verdict::Match, false) => Verdict::NoMatch,
                        (Verdict::NoMatch, false) => Verdict::Match,
                        _ => found,
                    }
                } else {
                    facts.evaluate(rule)
                }
            }),
            ResourceMode::All,
        )
    }
    pub fn planet_match(&self, facts: &Facts) -> Verdict {
        combine(
            self.section_verdicts(&self.planet, |rule| self.evaluate_planet(rule, facts))
                .into_iter(),
            ResourceMode::All,
        )
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
        let sections =
            self.section_verdicts(&self.planet, |rule| self.evaluate_planet(rule, facts));
        sections[Section::XenoArena as usize] == Verdict::Unknown
            && sections.iter().enumerate().all(|(index, &verdict)| {
                index == Section::XenoArena as usize || verdict == Verdict::Match
            })
    }
}

pub fn hue(rgb: [f64; 3]) -> Result<u32, &'static str> {
    if !rgb.iter().all(|value| value.is_finite() && *value >= 0.0) {
        return Err("invalid colour components");
    }
    let [red, green, blue] = rgb;
    let maximum = red.max(green).max(blue);
    let minimum = red.min(green).min(blue);
    let range = maximum - minimum;
    if maximum == 0.0 || range / maximum < 0.12 {
        return Ok(8);
    }
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
    Ok(if !(15.0..345.0).contains(&degrees) {
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
    })
}
