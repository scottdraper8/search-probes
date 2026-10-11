use crate::{
    capture, colour_appearance, grass_appearance,
    object_cache::ObjectTableCache,
    object_flags::ObjectTables,
    planet_query_context::PlanetQueryContext,
    query::{Bindings, Query},
    snapshot::{Snapshot, apply_capture},
    solar::SolarContext,
};
use search_probes_model::{
    address::Address,
    cache::{MAX_NAME_BYTES, SystemRecord},
    criteria::{Field, Plan, PlanetFlag, Predicate, SystemFlag},
    facts::Verdict,
    weather,
};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, Default)]
pub struct Work {
    pub systems: u64,
    pub planets: u64,
    pub metadata_captures: u64,
    pub colour_captures: u64,
    pub object_scans: u64,
    pub names: u64,
}

impl std::ops::AddAssign for Work {
    fn add_assign(&mut self, rhs: Self) {
        self.systems = self.systems.saturating_add(rhs.systems);
        self.planets = self.planets.saturating_add(rhs.planets);
        self.metadata_captures = self.metadata_captures.saturating_add(rhs.metadata_captures);
        self.colour_captures = self.colour_captures.saturating_add(rhs.colour_captures);
        self.object_scans = self.object_scans.saturating_add(rhs.object_scans);
        self.names = self.names.saturating_add(rhs.names);
    }
}

pub struct Match {
    pub planet: usize,
    pub name: String,
}

pub struct Outcome {
    pub snapshot: Snapshot,
    pub matches: Vec<Match>,
    pub work: Work,
    pub cancelled: bool,
}

impl Outcome {
    pub fn into_record(self) -> Result<SystemRecord, &'static str> {
        let mut record = SystemRecord::new(self.snapshot.system, self.snapshot.planets)?;
        for result in self.matches {
            let mut end = result.name.len().min(MAX_NAME_BYTES);
            while !result.name.is_char_boundary(end) {
                end -= 1;
            }
            record.set_name(result.planet, &result.name[..end])?;
        }
        Ok(record)
    }
}

pub struct Generator<'cache> {
    object_summaries: &'cache ObjectTableCache,
    bindings: Bindings,
    solar: SolarContext,
    planet: PlanetQueryContext,
    objects: Option<ObjectTables>,
    base: usize,
}

impl<'cache> Generator<'cache> {
    /// # Safety
    /// Requires the exact verified image, enabled query hooks and a waiting gameplay callback that
    /// holds the live configuration graph fixed until this worker-owned generator is dropped.
    pub unsafe fn new(
        base: usize,
        live: *const u8,
        object_summaries: &'cache ObjectTableCache,
    ) -> Result<Self, &'static str> {
        unsafe {
            Ok(Self {
                bindings: Bindings::from_verified_image(base)?,
                solar: SolarContext::new(base)?,
                planet: PlanetQueryContext::borrow_query_configuration(base, live)?,
                objects: None,
                object_summaries,
                base,
            })
        }
    }

    /// # Safety
    /// The game-resource lifetime and quiescence contract from construction remains in force.
    pub unsafe fn search(
        &mut self,
        address: Address,
        plan: &Plan,
        difficulty: usize,
        cancel: &AtomicBool,
        cached: Option<&SystemRecord>,
    ) -> Result<Outcome, &'static str> {
        if difficulty > 3 {
            return Err("invalid combat difficulty");
        }
        let mut query = unsafe { Query::begin(&self.bindings, self.solar.as_mut_ptr(), address) }?;
        let mut outcome = Outcome {
            snapshot: Snapshot::solar(query.bytes())?,
            matches: Vec::new(),
            work: Work {
                systems: 1,
                ..Work::default()
            },
            cancelled: false,
        };
        if let Some(cached) = cached {
            if cached.count() != outcome.snapshot.count {
                return Err("cached system count changed");
            }
            let mut system = cached.system.clone();
            system.merge(&outcome.snapshot.system);
            outcome.snapshot.system = system;
            for index in 0..cached.count() {
                let mut planet = cached.planets[index].clone();
                planet.merge(&outcome.snapshot.planets[index]);
                outcome.snapshot.planets[index] = planet;
            }
            outcome.snapshot.aggregate();
        }
        if plan.system_match(
            &outcome.snapshot.system,
            &outcome.snapshot.planets[..outcome.snapshot.count],
        ) == Verdict::NoMatch
        {
            return Ok(outcome);
        }
        if plan.requirements.objects && self.objects.is_none() {
            self.objects = Some(unsafe { ObjectTables::from_verified_image(self.base) }?);
        }
        let generator = self.planet.as_mut_ptr();
        let planet_storms = plan
            .planet
            .iter()
            .any(|rule| matches!(rule, Predicate::Planet(PlanetFlag::ExtremeStorms, _)));
        let system_storms = plan
            .system
            .iter()
            .any(|rule| matches!(rule, Predicate::System(SystemFlag::ExtremeStorms, _)));
        for index in 0..outcome.snapshot.count {
            if cancel.load(Ordering::Relaxed) {
                outcome.cancelled = true;
                break;
            }
            let system_verdict = plan.system_match(
                &outcome.snapshot.system,
                &outcome.snapshot.planets[..outcome.snapshot.count],
            );
            if system_verdict == Verdict::NoMatch
                || (system_verdict == Verdict::Match && plan.planet.is_empty())
            {
                break;
            }
            if system_verdict == Verdict::Match
                && plan.planet_match(&outcome.snapshot.planets[index]) == Verdict::NoMatch
            {
                continue;
            }
            if system_verdict == Verdict::Match
                && plan.planet_match(&outcome.snapshot.planets[index]) == Verdict::Match
            {
                if let Some(name) = cached.and_then(|record| record.name(index)) {
                    outcome.matches.push(Match {
                        planet: index,
                        name: name.into(),
                    });
                    continue;
                }
            }
            outcome.work.planets += 1;
            unsafe {
                query.visit_planet(generator, index, |live, bytes| {
                    outcome.snapshot.read_planet(bytes, index, difficulty)?;
                    let facts = &mut outcome.snapshot.planets[index];
                    // System storm criteria need the same per-planet hazard proof as
                    // planet storm criteria, including when there is no planet target.
                    if system_verdict == Verdict::Unknown
                        && system_storms
                        && facts.planet_flag(PlanetFlag::ExtremeStorms).is_none()
                    {
                        let details = capture::capture(live, generator, capture::WEATHER)?;
                        let profile = capture::storm_profile(live, generator)?;
                        facts.set_extreme_storm_profile(weather::extreme_storm_profile(&profile)?);
                        apply_capture(facts, &details, difficulty)?;
                        outcome.work.metadata_captures += 1;
                    }
                    if plan.planet.is_empty() || plan.planet_match(facts) == Verdict::NoMatch {
                        return Ok(());
                    }
                    let needed = plan.missing_planet_requirements(facts);
                    if needed.hints {
                        let hints = crate::resource_hints::capture(live)?;
                        facts.set(Field::UniqueResources, hints.unique_resources);
                        facts.set_planet_flag(PlanetFlag::VileBrood, hints.vile_brood);
                        if plan.planet_match(facts) == Verdict::NoMatch {
                            return Ok(());
                        }
                    }
                    let needed = plan.missing_planet_requirements(facts);
                    if needed.fauna {
                        if plan.planet.iter().any(|rule| matches!(rule, search_probes_model::criteria::Predicate::Fauna(_))) && facts.fauna().is_none() {
                            let observed = crate::fauna::capture(live)?;
                            facts.set_planet_flag(PlanetFlag::PlayerAttackingFauna, observed.player_attacking);
                            facts.set_planet_flag(PlanetFlag::Sandworms, observed.sandworms);
                            facts.set_fauna(observed.species);
                        } else {
                            facts.set_planet_flag(PlanetFlag::PlayerAttackingFauna, crate::fauna::player_attacking(live)?);
                            facts.set_planet_flag(PlanetFlag::Sandworms, crate::fauna::sandworms(live)?);
                        }
                        if plan.planet_match(facts) == Verdict::NoMatch { return Ok(()); }
                    }
                    let needed = plan.missing_planet_requirements(facts);
                    let metadata = if needed.metadata {
                        capture::METADATA
                    } else {
                        0
                    } | if needed.weather { capture::WEATHER } else { 0 }
                        | if needed.screen_filter {
                            capture::SCREEN_FILTER
                        } else {
                            0
                        };
                    if metadata != 0 {
                        let details = capture::capture(live, generator, metadata)?;
                        if needed.weather && planet_storms && facts.planet_flag(PlanetFlag::ExtremeStorms).is_none() {
                            let profile = capture::storm_profile(live, generator)?;
                            facts.set_extreme_storm_profile(weather::extreme_storm_profile(&profile)?);
                        }
                        apply_capture(facts, &details, difficulty)?;
                        outcome.work.metadata_captures += 1;
                        if plan.planet_match(facts) == Verdict::NoMatch {
                            return Ok(());
                        }
                    }
                    let needed = plan.missing_planet_requirements(facts);
                    if needed.terrain {
                        let layers = crate::terrain::features(live)?;
                        facts.observe_terrain_layers(layers);
                        if plan.planet_match(facts) == Verdict::NoMatch { return Ok(()); }
                    }
                    let needed = plan.missing_planet_requirements(facts);
                    let palette_colours = needed.colours & !3;
                    if palette_colours != 0 {
                        let details =
                            capture::capture(live, generator, u32::from(palette_colours) << 8)?;
                        colour_appearance::apply(facts, &details)?;
                        outcome.work.colour_captures += 1;
                        if plan.planet_match(facts) == Verdict::NoMatch {
                            return Ok(());
                        }
                    }
                    let flora_needed = needed.colours & 2 != 0;
                    let grass = if needed.colours & 1 != 0 {
                        let captured = capture::capture(live, generator, capture::colour(0))?;
                        outcome.work.colour_captures += 1;
                        let prepared = grass_appearance::prepare(
                            captured.grass_terrain[0],
                            captured.screen_filter,
                        );
                        if let Ok(prepared) = &prepared {
                            if let Some(candidates) = prepared.fixed_colour() {
                                facts.restrict_grass_colours(candidates);
                                if plan.planet_match(facts) == Verdict::NoMatch {
                                    return Ok(());
                                }
                            }
                        }
                        Some(prepared)
                    } else {
                        None
                    };
                    if let Some(objects) = self.objects.as_ref().filter(|_| needed.objects) {
                        let need_glow = [PlanetFlag::GlowingGrass, PlanetFlag::GlowingFlora]
                            .into_iter().enumerate().fold(0, |mask, (bit, flag)| {
                                mask | if plan.planet.iter().any(|predicate|
                                    matches!(predicate, search_probes_model::criteria::Predicate::Planet(selected, _) if *selected == flag))
                                    && facts.planet_flag(flag).is_none() { 1 << bit } else { 0 }
                            });
                        let mut flora_models = Vec::new();
                        let need_glow = need_glow | if flora_needed { 4 } else { 0 };
                        let flags = objects.flags(live, self.object_summaries, need_glow, Some(&mut flora_models))?;
                        if flags.missing_tables != 0 {
                            return Err("an object table could not be loaded");
                        }
                        if let Some(layers) = facts.terrain_layers() {
                            facts.set(Field::Terrain, search_probes_model::terrain::FEATURE_SET | layers
                                | if flags.floating_islands != 0 { search_probes_model::terrain::FLOATING_ISLANDS } else { 0 });
                        }
                        if need_glow & 1 != 0 { facts.set_planet_flag(PlanetFlag::GlowingGrass, flags.glowing_grass); }
                        if need_glow & 2 != 0 { facts.set_planet_flag(PlanetFlag::GlowingFlora, flags.glowing_flora); }
                        facts.set_planet_flag(PlanetFlag::TitanWorms, flags.titan_worms);
                        outcome.work.object_scans += 1;
                        if plan.planet_match(facts) == Verdict::NoMatch {
                            return Ok(());
                        }
                        if let Some(grass) = grass {
                            let colours = if flags.ground_grass == 0 {
                                search_probes_model::vocabulary::GRASS_COLOUR_SET
                            } else {
                                grass_appearance::get(&grass?, flags.ground_grass)?
                            };
                            facts.set(Field::Grass, colours);
                        }
                        if plan.planet_match(facts)==Verdict::NoMatch {return Ok(());}
                        if flora_needed {
                            let filter=capture::capture(live,generator,capture::SCREEN_FILTER)?.screen_filter;
                            let context=crate::flora_appearance::Context {
                                palettes:capture::flora_palettes(live),
                                ground_colours:capture::flora_ground_colours(live),
                                palette_mode:((self.base+0x5261a94) as *const u32).read_unaligned(),
                            };
                            let wanted = plan.planet.iter().find_map(|predicate| match predicate {
                                search_probes_model::criteria::Predicate::Choice(choices)
                                    if choices.field() == Field::FloraColour => Some(choices.mask() as u32),
                                _ => None,
                            }).ok_or("flora acquisition has no colour criterion")?;
                            let observation=crate::flora_appearance::colours(&flora_models,&context,filter,wanted)?;
                            facts.observe_flora_colours(observation.colours, observation.known);
                            if observation.complete {
                                facts.set(Field::FloraColour,observation.colours);
                            }
                            outcome.work.colour_captures+=1;
                        }
                    }
                    match plan.planet_match(facts) {
                        Verdict::Unknown => {
                            return Err("selected planet criterion has no acquired value");
                        }
                        Verdict::NoMatch => return Ok(()),
                        Verdict::Match => {}
                    }
                    let name = capture::capture(live, generator, capture::NAME)?.name;
                    let len = name
                        .iter()
                        .position(|byte| *byte == 0)
                        .unwrap_or(name.len());
                    outcome.matches.push(Match {
                        planet: index,
                        name: String::from_utf8_lossy(&name[..len]).into_owned(),
                    });
                    outcome.work.names += 1;
                    Ok(())
                })?;
            }
            outcome.snapshot.aggregate();
        }
        if !self.planet.borrowed_configuration_unchanged() {
            return Err("native query changed borrowed configuration");
        }
        if outcome.cancelled {
            outcome.matches.clear();
            return Ok(outcome);
        }
        match plan.system_match(
            &outcome.snapshot.system,
            &outcome.snapshot.planets[..outcome.snapshot.count],
        ) {
            Verdict::NoMatch => outcome.matches.clear(),
            Verdict::Unknown => return Err("selected system criterion has no acquired value"),
            Verdict::Match => {}
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use search_probes_model::facts::Facts;

    #[test]
    fn captured_names_stay_inside_the_cache_bound_at_utf8_boundaries() {
        for name in [
            "a".repeat(200),
            "水".repeat(100),
            String::from_utf8_lossy(&[0xff; 128]).into_owned(),
        ] {
            let mut system = Facts::default();
            system.set_planet_count(1);
            let outcome = Outcome {
                snapshot: Snapshot {
                    system,
                    planets: std::array::from_fn(|_| Facts::default()),
                    count: 1,
                },
                matches: vec![Match {
                    planet: 0,
                    name: name.clone(),
                }],
                work: Work::default(),
                cancelled: false,
            };
            let record = outcome.into_record().unwrap();
            let retained = record.name(0).unwrap();
            assert!(retained.len() <= MAX_NAME_BYTES);
            assert!(name.starts_with(retained));
            assert!(retained.len() >= MAX_NAME_BYTES - 3);
        }
    }
}
