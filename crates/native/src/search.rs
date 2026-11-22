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
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

/// Cooperatively stops native work only at boundaries where owned native temporaries can drop.
pub struct Stop<'a> {
    cancel: &'a AtomicBool,
    deadline: Option<Instant>,
}

impl<'a> Stop<'a> {
    pub fn new(cancel: &'a AtomicBool, deadline: Option<Instant>) -> Self {
        Self { cancel, deadline }
    }
    pub fn requested(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
            || self
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Work {
    pub systems: u64,
    pub planets: u64,
    pub fauna: u64,
    pub metadata_captures: u64,
    pub colour_captures: u64,
    pub object_scans: u64,
    pub names: u64,
}

impl std::ops::AddAssign for Work {
    fn add_assign(&mut self, rhs: Self) {
        self.systems = self.systems.saturating_add(rhs.systems);
        self.planets = self.planets.saturating_add(rhs.planets);
        self.fauna = self.fauna.saturating_add(rhs.fauna);
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
    pub planet_observations: Option<search_probes_model::observations::PlanetObservations>,
    pub pending_flora: Option<PendingFlora>,
    /// Exact-query scalar inputs for Arena predicates still needing model/placement refinement.
    /// Native temporary planet/role objects are already gone when this result reaches the scheduler.
    pub pending_arena: Vec<crate::arena_inputs::Planet>,
    pub work: Work,
    pub cancelled: bool,
}

pub struct PendingFlora {
    pub address: Address,
    pub planet: usize,
    pub models: Vec<crate::flora_appearance::Spawn>,
    pub context: crate::flora_appearance::Context,
    pub filter: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Search,
    Inspect(Option<usize>),
}

impl Operation {
    fn validate_plan(self, plan: &Plan) -> Result<(), &'static str> {
        match self {
            Self::Search => search_probes_model::settings::validate_search_plan(plan),
            Self::Inspect(_) => Ok(()),
        }
    }

    fn needs_flora_inventory(self, colour_requirements: u16) -> bool {
        matches!(self, Self::Inspect(Some(_))) && colour_requirements & 2 != 0
    }
}

fn planet_order(count: usize, operation: Operation) -> Vec<usize> {
    if let Operation::Inspect(Some(target)) = operation {
        return std::iter::once(target)
            .chain((0..count).filter(|&index| index != target))
            .collect();
    }
    (0..count).collect()
}

fn needs_star_count(plan: &Plan) -> bool {
    plan.system
        .iter()
        .any(|predicate| matches!(predicate, Predicate::StarCounts(_)))
}

impl Outcome {
    pub fn complete_flora(&mut self, cancel: &Stop<'_>) -> Result<(), &'static str> {
        let Some(pending) = self.pending_flora.take() else {
            return Ok(());
        };
        let inventory = crate::flora_appearance::inventory(
            &pending.models,
            &pending.context,
            pending.filter,
            false,
            cancel,
        )?;
        self.snapshot.planets[pending.planet]
            .observe_flora_colours(0, inventory.predicate_exclusions());
        self.planet_observations = Some(
            search_probes_model::observations::PlanetObservations {
                address: pending.address,
                acquisition: search_probes_model::observations::ObservationAcquisition::CurrentWorldGeneratedQuery,
                surface_models: inventory,
            },
        );
        Ok(())
    }

    pub fn into_record(
        self,
    ) -> Result<(SystemRecord, Vec<crate::arena_inputs::Planet>), &'static str> {
        let mut record = SystemRecord::new(self.snapshot.system, self.snapshot.planets)?;
        for result in self.matches {
            let mut end = result.name.len().min(MAX_NAME_BYTES);
            while !result.name.is_char_boundary(end) {
                end -= 1;
            }
            record.set_name(result.planet, &result.name[..end])?;
        }
        Ok((record, self.pending_arena))
    }
}

pub struct Generator<'cache> {
    object_summaries: &'cache ObjectTableCache,
    bindings: Bindings,
    solar: SolarContext,
    planet: PlanetQueryContext,
    objects: Option<ObjectTables>,
    arena: Option<crate::arena::Context>,
    fauna: Option<crate::fauna::Context>,
    base: usize,
}

impl<'cache> Generator<'cache> {
    /// # Safety
    /// Query hooks must be enabled while a waiting gameplay callback holds the live configuration
    /// graph fixed until this worker-owned generator is dropped.
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
                arena: None,
                fauna: None,
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
        cancel: &Stop<'_>,
        cached: Option<&SystemRecord>,
    ) -> Result<Outcome, &'static str> {
        unsafe { self.execute(address, plan, difficulty, cancel, cached, Operation::Search) }
    }

    /// Inspect the loaded address without search pruning or cache reuse.
    ///
    /// # Safety
    /// The game-resource lifetime and quiescence contract from construction remains in force.
    pub unsafe fn inspect_current(
        &mut self,
        address: Address,
        plan: &Plan,
        difficulty: usize,
        cancel: &Stop<'_>,
    ) -> Result<Outcome, &'static str> {
        let target = match address.coordinates().planet {
            0 => None,
            value => Some(usize::from(value - 1)),
        };
        unsafe {
            self.execute(
                address.system(),
                plan,
                difficulty,
                cancel,
                None,
                Operation::Inspect(target),
            )
        }
    }

    unsafe fn execute(
        &mut self,
        address: Address,
        plan: &Plan,
        difficulty: usize,
        cancel: &Stop<'_>,
        cached: Option<&SystemRecord>,
        operation: Operation,
    ) -> Result<Outcome, &'static str> {
        operation.validate_plan(plan)?;
        if cancel.requested() {
            return Err("search stopped before system generation");
        }
        if difficulty > 3 {
            return Err("invalid combat difficulty");
        }
        let mut query = unsafe {
            Query::begin(
                &self.bindings,
                self.solar.as_mut_ptr(),
                address,
                needs_star_count(plan),
            )
        }?;
        let mut outcome = Outcome {
            snapshot: Snapshot::solar(query.bytes(), query.star_count())?,
            matches: Vec::new(),
            planet_observations: None,
            pending_flora: None,
            pending_arena: Vec::new(),
            work: Work {
                systems: 1,
                ..Work::default()
            },
            cancelled: false,
        };
        if let Operation::Inspect(Some(index)) = operation
            && index >= outcome.snapshot.count
        {
            return Err("current planet is outside the generated system");
        }
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
            && operation == Operation::Search
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
        for index in planet_order(outcome.snapshot.count, operation) {
            if cancel.requested() {
                outcome.cancelled = true;
                break;
            }
            let inspect_target =
                matches!(operation, Operation::Inspect(Some(target)) if target == index);
            let system_verdict = plan.system_match(
                &outcome.snapshot.system,
                &outcome.snapshot.planets[..outcome.snapshot.count],
            );
            if matches!(operation, Operation::Inspect(Some(_)))
                && !inspect_target
                && system_verdict != Verdict::Unknown
            {
                continue;
            }
            if operation == Operation::Search {
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
                    && let Some(name) = cached.and_then(|record| record.name(index))
                {
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
                    let mut arena_inputs = None;
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
                    if matches!(operation, Operation::Inspect(_)) && !inspect_target {
                        return Ok(());
                    }
                    if !inspect_target
                        && (plan.planet.is_empty()
                            || plan.planet_match(facts) == Verdict::NoMatch)
                    {
                        return Ok(());
                    }
                    let needed = if inspect_target {
                        plan.inspection_requirements(facts)
                    } else {
                        plan.missing_planet_requirements(facts)
                    };
                    if needed.hints {
                        let hints = crate::resource_hints::capture(live)?;
                        facts.set(Field::SpecialtyResources, hints.specialty_resources);
                        facts.set_planet_flag(PlanetFlag::VileBrood, hints.vile_brood);
                        if !inspect_target && plan.planet_match(facts) == Verdict::NoMatch {
                            return Ok(());
                        }
                    }
                    let needed = if inspect_target {
                        plan.inspection_requirements(facts)
                    } else {
                        plan.missing_planet_requirements(facts)
                    };
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
                        if !inspect_target && plan.planet_match(facts) == Verdict::NoMatch {
                            return Ok(());
                        }
                    }
                    let needed = if inspect_target {
                        plan.inspection_requirements(facts)
                    } else {
                        plan.missing_planet_requirements(facts)
                    };
                    if needed.terrain {
                        let layers = crate::terrain::features(live)?;
                        facts.observe_terrain_layers(layers);
                        if !inspect_target && plan.planet_match(facts) == Verdict::NoMatch {
                            return Ok(());
                        }
                    }
                    let needed = if inspect_target {
                        plan.inspection_requirements(facts)
                    } else {
                        plan.missing_planet_requirements(facts)
                    };
                    let palette_colours = needed.colours & !3;
                    if palette_colours != 0 {
                        let details =
                            capture::capture(live, generator, u32::from(palette_colours) << 8)?;
                        colour_appearance::apply(facts, &details)?;
                        outcome.work.colour_captures += 1;
                        if !inspect_target && plan.planet_match(facts) == Verdict::NoMatch {
                            return Ok(());
                        }
                    }
                    // Recipe enumeration cannot yet establish planet-wide occurrence or
                    // exclusion, so search cannot use it to resolve the predicate. Inspection
                    // only pays for the inventory when the selected criteria request it.
                    let flora_needed = inspect_target
                        && operation.needs_flora_inventory(needed.colours);
                    let grass = if needed.colours & 1 != 0 {
                        let captured = capture::capture(live, generator, capture::colour(0))?;
                        outcome.work.colour_captures += 1;
                        let prepared = grass_appearance::prepare(
                            captured.grass_terrain[0],
                            captured.screen_filter,
                        );
                        if let Ok(prepared) = &prepared
                            && let Some(candidates) = prepared.fixed_colour()
                        {
                            facts.restrict_grass_colours(candidates);
                            if !inspect_target && plan.planet_match(facts) == Verdict::NoMatch {
                                return Ok(());
                            }
                        }
                        Some(prepared)
                    } else {
                        None
                    };
                    if let Some(objects) = self
                        .objects
                        .as_ref()
                        .filter(|_| needed.objects)
                    {
                        let need_glow = [PlanetFlag::GlowingGrass, PlanetFlag::GlowingFlora]
                            .into_iter().enumerate().fold(0, |mask, (bit, flag)| {
                                mask | if plan.planet.iter().any(|predicate|
                                    matches!(predicate, search_probes_model::criteria::Predicate::Planet(selected, _) if *selected == flag))
                                    && facts.planet_flag(flag).is_none() { 1 << bit } else { 0 }
                            });
                        let mut flora_models = Vec::new();
                        let need_glow = need_glow | if flora_needed { 4 } else { 0 };
                        let flags = objects.flags(live, self.object_summaries, need_glow, Some(&mut flora_models), cancel)?;
                        if flags.missing_tables != 0 && (!inspect_target || needed.objects) {
                            return Err("an object table could not be loaded");
                        }
                        if let Some(layers) = facts.terrain_layers()
                            && (flags.missing_tables == 0 || flags.floating_islands != 0)
                        {
                            facts.set(Field::Terrain, search_probes_model::terrain::FEATURE_SET | layers
                                | if flags.floating_islands != 0 { search_probes_model::terrain::FLOATING_ISLANDS } else { 0 });
                        }
                        if need_glow & 1 != 0 && (flags.missing_tables == 0 || flags.glowing_grass) {
                            facts.set_planet_flag(PlanetFlag::GlowingGrass, flags.glowing_grass);
                        }
                        if need_glow & 2 != 0 && (flags.missing_tables == 0 || flags.glowing_flora) {
                            facts.set_planet_flag(PlanetFlag::GlowingFlora, flags.glowing_flora);
                        }
                        if flags.missing_tables == 0 || flags.titan_worms {
                            facts.set_planet_flag(PlanetFlag::TitanWorms, flags.titan_worms);
                        }
                        outcome.work.object_scans += 1;
                        if !inspect_target && plan.planet_match(facts) == Verdict::NoMatch {
                            return Ok(());
                        }
                        if let Some(grass) = grass.filter(|_| flags.missing_tables == 0) {
                            let colours = if flags.ground_grass == 0 {
                                search_probes_model::vocabulary::GRASS_COLOUR_SET
                            } else {
                                grass_appearance::get(&grass?, flags.ground_grass)?
                            };
                            facts.set(Field::Grass, colours);
                        }
                        if !inspect_target && plan.planet_match(facts) == Verdict::NoMatch {
                            return Ok(());
                        }
                        if flora_needed {
                            let filter=capture::capture(live,generator,capture::SCREEN_FILTER)?.screen_filter;
                            let context=crate::flora_appearance::Context {
                                palettes:capture::flora_palettes(live),
                                ground_colours:capture::flora_ground_colours(live),
                                // The 7.11 palette-mode RVA is zero-filled and has no code
                                // references in the verified 7.14 executable. Keep the current
                                // default mode instead of reading that unrelated storage.
                                palette_mode: 0,
                            };
                            outcome.pending_flora = Some(PendingFlora {
                                address: address.planet(index)?,
                                planet: index,
                                models: flora_models,
                                context,
                                filter,
                            });
                        }
                    }
                    let needed = if inspect_target {
                        plan.inspection_requirements(facts)
                    } else {
                        plan.missing_planet_requirements(facts)
                    };
                    // Fauna generation is the most expensive independent acquisition stage. Run
                    // it only after cheaper metadata, colour and object facts have had a chance to
                    // reject the planet.
                    if needed.fauna {
                        if plan.targets_animals() && facts.fauna().is_none() {
                            if self.fauna.is_none() {
                                self.fauna = Some(crate::fauna::Context::from_verified_image(self.base)?);
                            }
                            let observed = crate::fauna::capture(
                                self.fauna.as_ref().unwrap(),
                                live,
                                address.planet(index)?,
                            )?;
                            outcome.work.fauna = outcome
                                .work
                                .fauna
                                .saturating_add(observed.species.len() as u64);
                            facts.set_planet_flag(PlanetFlag::PlayerAttackingFauna, observed.player_attacking);
                            facts.set_planet_flag(PlanetFlag::Sandworms, observed.sandworms);
                            facts.set_fauna(observed.species);
                        } else {
                            facts.set_planet_flag(PlanetFlag::PlayerAttackingFauna, crate::fauna::player_attacking(live)?);
                            facts.set_planet_flag(PlanetFlag::Sandworms, crate::fauna::sandworms(live)?);
                        }
                        if plan.planet.iter().any(|rule| matches!(rule, Predicate::Arena(_)))
                            && facts.arena().is_none()
                        {
                            if self.arena.is_none() {
                                self.arena = Some(crate::arena::Context::from_verified_image(self.base)?);
                            }
                            let inputs = crate::arena_inputs::Planet::capture(live, address.planet(index)?)?;
                            facts.set_arena(self.arena.as_ref().unwrap().capture_inputs(&inputs)?);
                            arena_inputs = Some(inputs);
                        }
                        if !inspect_target && plan.planet_match(facts) == Verdict::NoMatch {
                            return Ok(());
                        }
                    }
                    if inspect_target {
                        return Ok(());
                    }
                    match plan.planet_match(facts) {
                        Verdict::Unknown if plan.unresolved_arena_ranks(facts) => {
                            // A cached partial Arena observation still needs this fresh query's
                            // owned inputs; it must not depend on an earlier visitor's storage.
                            outcome.pending_arena.push(match arena_inputs {
                                Some(inputs) => inputs,
                                None => crate::arena_inputs::Planet::capture(live, address.planet(index)?)?,
                            });
                            return Ok(());
                        }
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
            outcome.pending_arena.clear();
            return Ok(outcome);
        }
        match plan.system_match(
            &outcome.snapshot.system,
            &outcome.snapshot.planets[..outcome.snapshot.count],
        ) {
            Verdict::NoMatch => {
                outcome.matches.clear();
                outcome.pending_arena.clear();
            }
            Verdict::Unknown => return Err("selected system criterion has no acquired value"),
            Verdict::Match => {}
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn current_planet_is_generated_before_system_followups() {
        assert_eq!(
            planet_order(5, Operation::Inspect(Some(3))),
            vec![3, 0, 1, 2, 4]
        );
        assert_eq!(planet_order(3, Operation::Search), vec![0, 1, 2]);
        assert_eq!(planet_order(3, Operation::Inspect(None)), vec![0, 1, 2]);
    }

    #[test]
    fn star_count_derivation_is_reserved_for_star_count_criteria() {
        let ordinary = search_probes_model::criteria::Criteria::default()
            .compile()
            .unwrap();
        assert!(!needs_star_count(&ordinary));

        let stars = search_probes_model::criteria::Criteria {
            star_counts: 0b101,
            ..Default::default()
        }
        .compile()
        .unwrap();
        assert!(needs_star_count(&stars));
    }

    #[test]
    fn flora_colour_is_rejected_before_search_generation_but_allowed_for_inspection() {
        let mut criteria = search_probes_model::criteria::Criteria::default();
        criteria.choices.insert(Field::FloraColour, 1);
        let plan = criteria.compile().unwrap();
        assert_eq!(
            Operation::Search.validate_plan(&plan),
            Err(search_probes_model::settings::FLORA_COLOUR_SEARCH_ERROR)
        );
        assert!(Operation::Inspect(Some(0)).validate_plan(&plan).is_ok());
        assert!(Operation::Inspect(None).validate_plan(&plan).is_ok());
        criteria.choices.clear();
        assert!(
            Operation::Search
                .validate_plan(&criteria.compile().unwrap())
                .is_ok()
        );
    }

    #[test]
    fn current_world_inspection_only_collects_requested_flora_inventory() {
        let facts = search_probes_model::facts::Facts::default();
        let mut criteria = search_probes_model::criteria::Criteria::default();
        criteria.choices.insert(Field::Grass, 1);
        let grass = criteria.compile().unwrap().inspection_requirements(&facts);
        assert!(!Operation::Inspect(Some(0)).needs_flora_inventory(grass.colours));

        criteria.choices.clear();
        criteria.choices.insert(Field::FloraColour, 1);
        let flora = criteria.compile().unwrap().inspection_requirements(&facts);
        assert!(Operation::Inspect(Some(0)).needs_flora_inventory(flora.colours));
        assert!(!Operation::Inspect(None).needs_flora_inventory(flora.colours));
        assert!(!Operation::Search.needs_flora_inventory(flora.colours));
    }

    #[test]
    fn cooperative_stop_observes_both_deadlines_and_user_cancellation() {
        let cancel = AtomicBool::new(false);
        assert!(!Stop::new(&cancel, None).requested());
        assert!(Stop::new(&cancel, Some(Instant::now() - Duration::from_secs(1))).requested());
        assert!(!Stop::new(&cancel, Some(Instant::now() + Duration::from_secs(60))).requested());
        cancel.store(true, Ordering::Release);
        assert!(Stop::new(&cancel, None).requested());
    }
}
