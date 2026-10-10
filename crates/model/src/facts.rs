use crate::criteria::{Field, Plan, PlanetFlag, Predicate, Requirements, Stage, SystemFlag};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
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
    pub fn set_planet_count(&mut self, count: u8) {
        self.planet_count = Some(count);
    }
    pub fn planet_count(&self) -> Option<u8> {
        self.planet_count
    }

    pub fn merge(&mut self, new: &Self) {
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

fn conjunction(predicates: &[Predicate], facts: &Facts) -> Verdict {
    let mut verdict = Verdict::Match;
    for &predicate in predicates {
        match facts.evaluate(predicate) {
            Verdict::NoMatch => return Verdict::NoMatch,
            Verdict::Unknown => verdict = Verdict::Unknown,
            Verdict::Match => {}
        }
    }
    verdict
}

impl Plan {
    pub fn missing_planet_requirements(&self, facts: &Facts) -> Requirements {
        let mut needed = Requirements::default();
        for &predicate in &self.planet {
            if facts.evaluate(predicate) != Verdict::Unknown {
                continue;
            }
            needed.planet_query |= predicate.stage() >= Stage::Query;
            needed.metadata |= predicate.stage() == Stage::Metadata
                || matches!(predicate, Predicate::Planet(PlanetFlag::Paradise, _));
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
    pub fn system_match(&self, facts: &Facts) -> Verdict {
        conjunction(&self.system, facts)
    }
    pub fn planet_match(&self, facts: &Facts) -> Verdict {
        conjunction(&self.planet, facts)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::criteria::Criteria;

    #[test]
    fn missing_data_is_never_a_negative_fact() {
        let mut criteria = Criteria::default();
        criteria.planets.insert(PlanetFlag::GlowingGrass, false);
        let plan = criteria.compile().unwrap();
        let mut facts = Facts::default();
        assert_eq!(plan.planet_match(&facts), Verdict::Unknown);
        facts.set_planet_flag(PlanetFlag::GlowingGrass, false);
        assert_eq!(plan.planet_match(&facts), Verdict::Match);
    }

    #[test]
    fn partial_flora_observations_preserve_unknowns_and_merge_proven_absences() {
        use crate::vocabulary::GRASS_COLOUR_SET;
        let mut criteria = Criteria::default();
        criteria.choices.insert(Field::FloraColour, 1 << 3);
        let green = criteria.compile().unwrap();
        criteria.choices.insert(Field::FloraColour, 1 << 10);
        let white = criteria.compile().unwrap();
        let mut observed = Facts::default();
        observed.observe_flora_colours(GRASS_COLOUR_SET | 1 << 3, 1 << 3);
        let mut cached = Facts::default();
        cached.merge(&observed);
        assert_eq!(green.planet_match(&cached), Verdict::Match);
        assert_eq!(green.missing_planet_requirements(&cached).colours & 2, 0);
        assert_eq!(white.planet_match(&cached), Verdict::Unknown);
        assert_ne!(white.missing_planet_requirements(&cached).colours & 2, 0);
        observed.observe_flora_colours(1 << 10, 1 << 10);
        cached.merge(&observed);
        assert_eq!(white.planet_match(&cached), Verdict::Match);

        criteria.choices.insert(Field::FloraColour, 1 << 5);
        let blue = criteria.compile().unwrap();
        assert_eq!(blue.planet_match(&cached), Verdict::Unknown);
        observed.observe_flora_colours(0, 1 << 5);
        cached.merge(&observed);
        assert_eq!(blue.planet_match(&cached), Verdict::NoMatch);
        assert_eq!(blue.missing_planet_requirements(&cached).colours & 2, 0);
        criteria
            .choices
            .insert(Field::FloraColour, (1 << 5) | (1 << 0));
        let blue_or_red = criteria.compile().unwrap();
        assert_eq!(blue_or_red.planet_match(&cached), Verdict::Unknown);
        observed.observe_flora_colours(0, 1 << 0);
        cached.merge(&observed);
        assert_eq!(blue_or_red.planet_match(&cached), Verdict::NoMatch);
        cached.set(Field::FloraColour, GRASS_COLOUR_SET | 1 << 3 | 1 << 10);
        assert_eq!(blue.planet_match(&cached), Verdict::NoMatch);
    }

    #[test]
    fn terrain_layers_match_features_and_defer_unknown_floating_objects() {
        use crate::terrain::{FEATURE_SET, FLOATING_ISLANDS};
        let mut criteria = Criteria::default();
        let mountain = 1 << 1;
        let cave = 1 << 6;
        criteria.choices.insert(Field::Terrain, mountain | cave);
        let layers = criteria.compile().unwrap();
        assert!(layers.requirements.terrain);
        assert!(!layers.requirements.objects);
        let mut facts = Facts::default();
        assert_eq!(layers.planet_match(&facts), Verdict::Unknown);
        facts.observe_terrain_layers(mountain as u32);
        assert_eq!(layers.planet_match(&facts), Verdict::Match);
        assert!(!layers.missing_planet_requirements(&facts).terrain);
        facts.observe_terrain_layers(1 << 4);
        assert_eq!(layers.planet_match(&facts), Verdict::NoMatch);

        criteria
            .choices
            .insert(Field::Terrain, u64::from(FLOATING_ISLANDS) | mountain);
        let either = criteria.compile().unwrap();
        assert!(either.requirements.objects);
        assert_eq!(either.planet_match(&facts), Verdict::Unknown);
        assert!(either.missing_planet_requirements(&facts).objects);
        facts.observe_terrain_layers(mountain as u32);
        assert_eq!(either.planet_match(&facts), Verdict::Match);
        assert!(!either.missing_planet_requirements(&facts).objects);

        criteria
            .choices
            .insert(Field::Terrain, u64::from(FLOATING_ISLANDS));
        let floating = criteria.compile().unwrap();
        let mut retained = Facts::default();
        retained.merge(&facts);
        assert_eq!(floating.planet_match(&retained), Verdict::Unknown);
        retained.set(Field::Terrain, FEATURE_SET | mountain as u32);
        assert_eq!(floating.planet_match(&retained), Verdict::NoMatch);
        retained.set(
            Field::Terrain,
            FEATURE_SET | mountain as u32 | FLOATING_ISLANDS,
        );
        assert_eq!(floating.planet_match(&retained), Verdict::Match);
        assert!(!floating.missing_planet_requirements(&retained).objects);
    }

    #[test]
    fn choices_are_or_within_each_field_and_and_across_fields() {
        let mut criteria = Criteria::default();
        criteria.choices.insert(Field::Size, 0b10111);
        criteria.choices.insert(Field::Biome, 1);
        let plan = criteria.compile().unwrap();
        let mut facts = Facts::default();
        facts.set(Field::Size, 3);
        assert_eq!(plan.planet_match(&facts), Verdict::NoMatch);
        facts.set(Field::Size, 0);
        assert_eq!(plan.planet_match(&facts), Verdict::Unknown);
        facts.set(Field::Biome, 0);
        assert_eq!(plan.planet_match(&facts), Verdict::Match);
        facts.set(Field::Biome, 1);
        assert_eq!(plan.planet_match(&facts), Verdict::NoMatch);
    }

    #[test]
    fn dominant_lifeform_distinguishes_empty_abandoned_and_unknown_systems() {
        for (mask, population, race, expected) in [
            (128, Some(1), Some(0), Verdict::Match),
            (128, Some(1), None, Verdict::Match),
            (1, Some(1), Some(0), Verdict::NoMatch),
            (1, Some(0), Some(0), Verdict::Match),
            (2, Some(2), Some(1), Verdict::Match),
            (2, Some(2), Some(2), Verdict::NoMatch),
            (128, Some(2), Some(1), Verdict::NoMatch),
            (128, Some(0), Some(7), Verdict::NoMatch),
            (129, Some(1), Some(2), Verdict::Match),
            (129, Some(0), Some(0), Verdict::Match),
            (129, Some(0), Some(2), Verdict::NoMatch),
            (1, None, Some(0), Verdict::Unknown),
            (1, Some(0), None, Verdict::Unknown),
        ] {
            let mut criteria = Criteria::default();
            criteria.choices.insert(Field::Race, mask);
            let plan = criteria.compile().unwrap();
            let mut facts = Facts::default();
            if let Some(population) = population {
                facts.set(Field::Population, population);
            }
            if let Some(race) = race {
                facts.set(Field::Race, race);
            }
            assert_eq!(
                plan.system_match(&facts),
                expected,
                "mask={mask}, population={population:?}, race={race:?}"
            );
        }
    }

    #[test]
    fn merging_new_observations_preserves_known_fields() {
        let mut before = Facts::default();
        before.set(Field::Sky, 5);
        before.set_planet_flag(PlanetFlag::Water, true);
        let mut new = Facts::default();
        new.set(Field::Biome, 0);
        new.set_planet_flag(PlanetFlag::Water, false);
        before.merge(&new);
        assert_eq!(before.get(Field::Sky), Some(5));
        assert_eq!(before.get(Field::Biome), Some(0));
        assert_eq!(before.planet_flag(PlanetFlag::Water), Some(false));
        assert!(std::mem::size_of::<Facts>() <= 192);
    }

    #[test]
    fn hue_families_preserve_yellow_green_boundary_and_hdr_values() {
        for (colour, expected) in [
            ([1.0, 0.0, 0.0], 0),
            ([1.0, 0.5, 0.0], 1),
            ([1.0, 1.0, 0.0], 2),
            ([0.99, 1.0, 0.0], 3),
            ([0.0, 1.0, 1.0], 4),
            ([0.0, 0.0, 4.0], 5),
            ([0.5, 0.0, 1.0], 6),
            ([1.0, 0.0, 1.0], 7),
            ([1.0, 0.99, 0.99], 8),
            ([0.0; 3], 8),
        ] {
            assert_eq!(hue(colour).unwrap(), expected);
        }
        assert!(hue([f64::NAN, 0.0, 0.0]).is_err());
        assert!(hue([-1.0, 0.0, 0.0]).is_err());
    }
}
