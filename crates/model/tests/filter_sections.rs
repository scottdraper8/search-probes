use search_probes_model::{
    cache::SystemRecord,
    criteria::{
        CompositionField, Criteria, Field, PlanetFlag, Requirements, ResourceMode, Section,
        SelectionSide, SystemFlag, WaterFilter, WaterPresence, subtype_options_for,
    },
    facts::{Facts, Verdict},
};

fn planet(biome: u32, subtype: u32, size: u32, water: WaterPresence) -> Facts {
    let mut facts = Facts::default();
    facts.set(Field::Biome, biome);
    facts.set(Field::Subtype, subtype);
    facts.set(Field::Size, size);
    facts.set_water(water);
    facts
}

#[test]
fn unified_water_truth_tables_and_unknown_exclusions() {
    use WaterFilter::*;
    use WaterPresence::*;
    for (rule, expected) in [
        (Any, [true, true, true]),
        (RequireAny, [false, true, true]),
        (RequireDeep, [false, false, true]),
        (ExcludeDeep, [true, true, false]),
        (ExcludeAll, [true, false, false]),
    ] {
        let criteria = Criteria {
            planet_water: rule,
            system_water: rule,
            ..Default::default()
        };
        let plan = criteria.compile().unwrap();
        let mut system = Facts::default();
        system.set_planet_count(2);
        for (water, accepts) in [Dry, Shallow, Deep].into_iter().zip(expected) {
            let mut facts = Facts::default();
            facts.set_water(water);
            let result = if accepts {
                Verdict::Match
            } else {
                Verdict::NoMatch
            };
            assert_eq!(plan.planet_match(&facts), result, "{rule:?} {water:?}");
            assert_eq!(
                plan.system_match(&system, &[facts.clone(), facts.clone()]),
                result
            );
            let mut merged = Facts::default();
            merged.merge(&facts);
            assert_eq!(plan.planet_match(&merged), result);
            let partial = plan.system_match(&system, &[facts, Facts::default()]);
            assert_eq!(
                partial,
                match rule {
                    Any => Verdict::Match,
                    RequireAny | RequireDeep if accepts => Verdict::Match,
                    ExcludeAll | ExcludeDeep if !accepts => Verdict::NoMatch,
                    _ => Verdict::Unknown,
                }
            );
        }
        assert_eq!(
            plan.planet_match(&Facts::default()),
            if rule == Any {
                Verdict::Match
            } else {
                Verdict::Unknown
            }
        );
        let mixture = [planet(0, 1, 0, Dry), planet(0, 1, 1, Deep)];
        assert_eq!(
            plan.system_match(&system, &mixture),
            if matches!(rule, Any | RequireAny | RequireDeep) {
                Verdict::Match
            } else {
                Verdict::NoMatch
            }
        );
    }
}

#[test]
fn subsection_any_includes_exclusions_and_subsections_still_all_match() {
    let mut criteria = Criteria {
        planet_water: WaterFilter::RequireAny,
        ..Default::default()
    };
    criteria.choices.insert(Field::Sentinels, 1); // Absent.
    criteria.planets.insert(PlanetFlag::GlowingGrass, false);
    criteria
        .match_modes
        .insert(Section::Conditions, ResourceMode::Any);
    let any = criteria.compile().unwrap();
    criteria.match_modes.clear();
    let all = criteria.compile().unwrap();
    assert_ne!(all, any);
    let mut facts = Facts::default();
    facts.set_water(WaterPresence::Shallow);
    facts.set(Field::Sentinels, 3);
    facts.set_planet_flag(PlanetFlag::GlowingGrass, false);
    assert_eq!(any.planet_match(&facts), Verdict::Match);
    assert_eq!(all.planet_match(&facts), Verdict::NoMatch);
    facts.set_planet_flag(PlanetFlag::GlowingGrass, true);
    assert_eq!(any.planet_match(&facts), Verdict::NoMatch);

    let mut criteria = Criteria::default();
    criteria.planets.insert(PlanetFlag::GlowingGrass, true);
    criteria.planets.insert(PlanetFlag::GlowingFlora, false);
    criteria
        .match_modes
        .insert(Section::Environment, ResourceMode::Any);
    let plan = criteria.compile().unwrap();
    let mut facts = Facts::default();
    facts.set_planet_flag(PlanetFlag::GlowingFlora, false);
    assert_eq!(plan.planet_match(&facts), Verdict::Match);
    assert_eq!(
        plan.missing_planet_requirements(&facts),
        Requirements::default()
    );
}

#[test]
fn any_success_avoids_expensive_preparation_without_turning_unknowns_into_absence() {
    let mut criteria = Criteria::default();
    criteria.choices.insert(Field::Sky, 1 << 3);
    criteria.choices.insert(Field::FloraColour, 1 << 10);
    criteria
        .match_modes
        .insert(Section::Colours, ResourceMode::Any);
    let plan = criteria.compile().unwrap();
    let mut facts = Facts::default();
    facts.set(
        Field::Sky,
        search_probes_model::vocabulary::GRASS_COLOUR_SET | 1 << 3,
    );
    assert_eq!(plan.planet_match(&facts), Verdict::Match);
    assert_eq!(
        plan.missing_planet_requirements(&facts),
        Requirements::default()
    );
    facts.set(
        Field::Sky,
        search_probes_model::vocabulary::GRASS_COLOUR_SET | 1 << 0,
    );
    assert_eq!(plan.planet_match(&facts), Verdict::Unknown);
    assert!(plan.missing_planet_requirements(&facts).objects);
}

#[test]
fn all_system_inclusions_can_match_different_planets_and_stay_system_results() {
    let mut criteria = Criteria::default();
    criteria
        .composition
        .set(CompositionField::Biomes, SelectionSide::Include, 1 | 2);
    criteria
        .composition
        .set(CompositionField::Sizes, SelectionSide::Include, 1 | 16);
    let all = criteria.compile().unwrap();
    criteria
        .match_modes
        .insert(Section::Composition, ResourceMode::Any);
    let any = criteria.compile().unwrap();
    assert!(all.planet.is_empty() && any.planet.is_empty());
    let mut system = Facts::default();
    system.set_planet_count(2);
    let planets = [
        planet(0, 1, 0, WaterPresence::Dry),
        planet(1, 1, 4, WaterPresence::Dry),
    ];
    assert_eq!(all.system_match(&system, &planets), Verdict::Match);
    let same = [planets[0].clone(), planets[0].clone()];
    assert_eq!(all.system_match(&system, &same), Verdict::NoMatch);
    assert_eq!(any.system_match(&system, &same), Verdict::Match);
    let mut stored = std::array::from_fn(|_| Facts::default());
    stored[..2].clone_from_slice(&planets);
    let record = SystemRecord::new(system, stored).unwrap();
    let evaluation = record.evaluate(&all);
    assert_eq!((evaluation.matches, evaluation.unknown), (1, 0));
    assert!(!all.requirements.objects && !all.requirements.metadata);
}

#[test]
fn system_exclusions_wait_for_every_planet_and_any_can_be_satisfied_by_an_exclusion() {
    let mut criteria = Criteria::default();
    criteria
        .composition
        .set(CompositionField::Biomes, SelectionSide::Exclude, 1 | 2);
    criteria
        .composition
        .set(CompositionField::Sizes, SelectionSide::Include, 16);
    let all = criteria.compile().unwrap();
    criteria
        .match_modes
        .insert(Section::Composition, ResourceMode::Any);
    let any = criteria.compile().unwrap();
    let mut system = Facts::default();
    system.set_planet_count(2);
    let ordinary = planet(2, 1, 0, WaterPresence::Dry);
    assert_eq!(
        all.system_match(&system, &[ordinary.clone(), Facts::default()]),
        Verdict::Unknown
    );
    assert_eq!(
        any.system_match(&system, &[ordinary.clone(), Facts::default()]),
        Verdict::Unknown
    );
    assert_eq!(
        all.system_match(&system, &[ordinary.clone(), ordinary.clone()]),
        Verdict::NoMatch
    );
    assert_eq!(
        any.system_match(&system, &[ordinary.clone(), ordinary.clone()]),
        Verdict::Match
    );
    let excluded = planet(0, 1, 0, WaterPresence::Dry);
    assert_eq!(
        any.system_match(&system, &[ordinary.clone(), excluded]),
        Verdict::NoMatch
    );
}

#[test]
fn include_exclude_pairs_are_repeatable_and_biome_edits_clear_dependent_subtypes() {
    let mut criteria = Criteria::default();
    for field in [CompositionField::Biomes, CompositionField::Sizes] {
        for _ in 0..20 {
            criteria
                .composition
                .set(field, SelectionSide::Exclude, 1 | 16);
            criteria.composition.set(field, SelectionSide::Include, 16);
            assert_eq!(criteria.composition.selections(field).exclude, 1);
            criteria.composition.set(field, SelectionSide::Exclude, 16);
            assert_eq!(criteria.composition.selections(field).include, 0);
        }
    }
    criteria
        .composition
        .set(CompositionField::Biomes, SelectionSide::Include, 1);
    let (subtype, _) = subtype_options_for(1)[0];
    criteria
        .composition
        .set(CompositionField::Subtypes, SelectionSide::Include, subtype);
    criteria
        .composition
        .set(CompositionField::Biomes, SelectionSide::Exclude, 1);
    assert_eq!(criteria.composition.subtypes.include, 0);
    criteria
        .composition
        .set(CompositionField::Subtypes, SelectionSide::Exclude, subtype);
    criteria
        .composition
        .set(CompositionField::Biomes, SelectionSide::Exclude, 2);
    assert_eq!(criteria.composition.subtypes.exclude, 0);
}

#[test]
fn contextual_system_subtypes_match_on_the_same_planet_and_variants_count_as_one_choice() {
    let (mask, _) = subtype_options_for(1)
        .into_iter()
        .find(|(mask, _)| mask.count_ones() > 1)
        .unwrap();
    let subtype = mask.trailing_zeros() + 1;
    for side in [SelectionSide::Include, SelectionSide::Exclude] {
        for mode in [ResourceMode::All, ResourceMode::Any] {
            let mut criteria = Criteria::default();
            criteria.composition.set(CompositionField::Biomes, side, 1);
            criteria
                .composition
                .set(CompositionField::Subtypes, side, mask);
            criteria.match_modes.insert(Section::Composition, mode);
            let plan = criteria.compile().unwrap();
            let mut system = Facts::default();
            system.set_planet_count(2);
            let other_subtype = (1..30)
                .find(|value| mask & (1 << (value - 1)) == 0)
                .unwrap();
            let unrelated = [
                planet(0, other_subtype, 0, WaterPresence::Dry),
                planet(1, subtype, 0, WaterPresence::Dry),
            ];
            assert_eq!(
                plan.system_match(&system, &unrelated),
                if side == SelectionSide::Include {
                    Verdict::NoMatch
                } else {
                    Verdict::Match
                }
            );
            let matching = [
                unrelated[0].clone(),
                planet(0, subtype, 0, WaterPresence::Dry),
            ];
            assert_eq!(
                plan.system_match(&system, &matching),
                if side == SelectionSide::Include {
                    Verdict::Match
                } else {
                    Verdict::NoMatch
                }
            );
        }
    }
}

#[test]
fn planet_subtypes_remain_bound_to_biomes_in_any_mode() {
    let (mask, _) = subtype_options_for(1)[0];
    let mut criteria = Criteria::default();
    criteria.set_biome_choices(1);
    criteria.choices.insert(Field::Subtype, mask);
    criteria
        .match_modes
        .insert(Section::Environment, ResourceMode::Any);
    let plan = criteria.compile().unwrap();
    let matching = planet(0, mask.trailing_zeros() + 1, 0, WaterPresence::Dry);
    assert_eq!(plan.planet_match(&matching), Verdict::Match);
    let different = planet(1, mask.trailing_zeros() + 1, 0, WaterPresence::Dry);
    assert_eq!(plan.planet_match(&different), Verdict::NoMatch);
}

#[test]
fn new_controls_persist_and_removed_controls_are_rejected() {
    let mut criteria = Criteria {
        system_water: WaterFilter::ExcludeDeep,
        planet_water: WaterFilter::RequireAny,
        ..Default::default()
    };
    criteria.systems.insert(SystemFlag::Dissonance, true);
    criteria
        .composition
        .set(CompositionField::Sizes, SelectionSide::Include, 16);
    criteria
        .match_modes
        .insert(Section::Profile, ResourceMode::Any);
    let encoded = serde_json::to_string(&criteria).unwrap();
    let decoded: Criteria = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.compile().unwrap(), criteria.compile().unwrap());
    for flag in [
        "giant",
        "gas_giant",
        "non_gas_giant",
        "waterworld",
        "water",
        "deep_water",
        "weird",
        "infested",
        "relic",
        "rgb",
        "corrupt_sentinels",
        "dead",
    ] {
        assert!(
            serde_json::from_str::<Criteria>(&format!(r#"{{"systems":{{"{flag}":true}}}}"#))
                .is_err()
        );
    }
    for flag in ["water", "deep_water", "non_gas_giant"] {
        assert!(
            serde_json::from_str::<Criteria>(&format!(r#"{{"planets":{{"{flag}":true}}}}"#))
                .is_err()
        );
    }
    criteria.composition.sizes.exclude = 16;
    assert!(criteria.compile().is_err());
}

#[test]
fn subtype_values_in_different_biomes_do_not_clear_each_other_by_native_index() {
    let mut witness = None;
    'biomes: for (include, _) in Field::Biome.available_options() {
        for (exclude, _) in Field::Biome
            .available_options()
            .filter(|(index, _)| *index != include)
        {
            for (include_mask, include_label) in subtype_options_for(1 << include) {
                if let Some((exclude_mask, _)) = subtype_options_for(1 << exclude)
                    .into_iter()
                    .find(|(mask, label)| include_mask & mask != 0 && *label != include_label)
                {
                    witness = Some((include, exclude, include_mask, exclude_mask));
                    break 'biomes;
                }
            }
        }
    }
    let (include, exclude, include_mask, exclude_mask) = witness.unwrap();
    let mut criteria = Criteria::default();
    criteria.composition.set(
        CompositionField::Biomes,
        SelectionSide::Include,
        1 << include,
    );
    criteria.composition.set(
        CompositionField::Biomes,
        SelectionSide::Exclude,
        1 << exclude,
    );
    criteria.composition.set(
        CompositionField::Subtypes,
        SelectionSide::Include,
        include_mask,
    );
    criteria.composition.set(
        CompositionField::Subtypes,
        SelectionSide::Exclude,
        exclude_mask,
    );
    assert_eq!(criteria.composition.subtypes.include, include_mask);
    assert_eq!(criteria.composition.subtypes.exclude, exclude_mask);
    let plan = criteria.compile().unwrap();
    let facts = planet(
        include as u32,
        include_mask.trailing_zeros() + 1,
        0,
        WaterPresence::Dry,
    );
    let mut system = Facts::default();
    system.set_planet_count(1);
    assert_eq!(plan.system_match(&system, &[facts]), Verdict::Match);
}

#[test]
fn sizes_resolve_from_solar_inputs_and_system_profile_any_preserves_query_unknowns() {
    let mut criteria = Criteria::default();
    criteria
        .composition
        .set(CompositionField::Sizes, SelectionSide::Include, 16);
    let plan = criteria.compile().unwrap();
    assert_eq!(plan.requirements, Requirements::default());
    let mut system = Facts::default();
    system.set_planet_count(1);
    let mut giant = Facts::default();
    giant.set(Field::Size, 4);
    assert_eq!(
        plan.system_match(&system, std::slice::from_ref(&giant)),
        Verdict::Match
    );

    let mut criteria = Criteria {
        system_water: WaterFilter::ExcludeAll,
        ..Default::default()
    };
    criteria.systems.insert(SystemFlag::Dissonance, true);
    criteria
        .match_modes
        .insert(Section::Profile, ResourceMode::Any);
    let plan = criteria.compile().unwrap();
    assert_eq!(
        plan.system_match(&system, std::slice::from_ref(&giant)),
        Verdict::Unknown
    );
    system.set_system_flag(SystemFlag::Dissonance, true);
    assert_eq!(
        plan.system_match(&system, std::slice::from_ref(&giant)),
        Verdict::Match
    );
    system.set_system_flag(SystemFlag::Dissonance, false);
    assert_eq!(
        plan.system_match(&system, std::slice::from_ref(&giant)),
        Verdict::Unknown
    );
    giant.set_water(WaterPresence::Dry);
    assert_eq!(
        plan.system_match(&system, std::slice::from_ref(&giant)),
        Verdict::Match
    );
}
