use search_probes_model::{
    address::{Address, Coordinates},
    arena::{ArenaCriteria, ArenaField, Creature, RankEvidence, Ranks},
    criteria::{Criteria, Field, PlanetFlag, Section, SystemFlag, WaterFilter},
    facts::{Facts, Verdict},
    fauna::{Domain, Family, FaunaCriteria, FaunaField, NameIdentity, Species},
    runtime::CurrentWorld,
    vocabulary::SPECIALTY_RESOURCE_SET,
};
use serde_json::json;

#[test]
fn choices_are_unions_and_distinct_filters_are_conjunctions() {
    let mut criteria = Criteria::default();
    criteria.choices.insert(Field::Size, 0b11);
    criteria.choices.insert(Field::Biome, 1 << 2);
    let plan = criteria.compile().unwrap();
    for size in 0..3 {
        for biome in 0..4 {
            let mut facts = Facts::default();
            facts.set(Field::Size, size);
            facts.set(Field::Biome, biome);
            let expected = if size < 2 && biome == 2 {
                Verdict::Match
            } else {
                Verdict::NoMatch
            };
            assert_eq!(plan.planet_match(&facts), expected, "{size}, {biome}");
            assert_eq!(
                plan.planet_section_verdicts(&facts)[&Section::PlanetType],
                expected
            );
        }
    }
}

#[test]
fn empty_filters_impose_no_restriction() {
    let mut criteria = Criteria::default();
    criteria.choices.insert(Field::Size, 0);
    criteria
        .fauna
        .insert(Domain::Land, FaunaCriteria::default());
    let plan = criteria.compile().unwrap();
    assert!(plan.system.is_empty());
    assert!(plan.planet.is_empty());
    assert_eq!(plan.system_match(&Facts::default(), &[]), Verdict::Match);
    assert_eq!(plan.planet_match(&Facts::default()), Verdict::Match);
}

#[test]
fn conjunction_preserves_unknown_until_a_filter_rejects() {
    let mut criteria = Criteria::default();
    criteria.choices.insert(Field::Size, 0b11);
    criteria.choices.insert(Field::Biome, 1 << 2);
    let plan = criteria.compile().unwrap();
    for size in [None, Some(0), Some(2)] {
        for biome in [None, Some(2), Some(3)] {
            let mut facts = Facts::default();
            if let Some(size) = size {
                facts.set(Field::Size, size);
            }
            if let Some(biome) = biome {
                facts.set(Field::Biome, biome);
            }
            let expected = if size == Some(2) || biome == Some(3) {
                Verdict::NoMatch
            } else if size.is_none() || biome.is_none() {
                Verdict::Unknown
            } else {
                Verdict::Match
            };
            assert_eq!(plan.planet_match(&facts), expected, "{size:?}, {biome:?}");
        }
    }
}

#[test]
fn resource_filters_follow_the_same_union_and_conjunction_contract() {
    let mut criteria = Criteria::default();
    // Selecting more than three alternatives is valid; one is sufficient.
    criteria.resources.selected = 0b1111;
    criteria.choices.insert(Field::SpecialtyResources, 0b11);
    criteria.choices.insert(Field::ResourceRichness, 1 << 1);
    let plan = criteria.compile().unwrap();
    for resources in [0, 1, 2, 4, 8, 16, 31] {
        for specialty in 0..4 {
            for richness in 0..3 {
                let mut facts = Facts::default();
                facts.set_resources(resources);
                facts.set(
                    Field::SpecialtyResources,
                    SPECIALTY_RESOURCE_SET | specialty,
                );
                facts.set(Field::ResourceRichness, richness);
                let expected = if resources & 0b1111 != 0 && specialty != 0 && richness == 1 {
                    Verdict::Match
                } else {
                    Verdict::NoMatch
                };
                assert_eq!(
                    plan.planet_match(&facts),
                    expected,
                    "{resources}, {specialty}, {richness}"
                );
                assert_eq!(
                    plan.planet_section_verdicts(&facts)[&Section::Resources],
                    expected
                );
            }
        }
    }
}

#[test]
fn dependent_filters_follow_current_population_and_weather_selections() {
    let mut criteria = Criteria::default();
    criteria.choices.insert(Field::StormFrequency, 1 << 0);
    criteria.choices.insert(Field::StormType, 1 << 2);
    criteria.planets.insert(PlanetFlag::ExtremeStorms, true);
    assert!(criteria.compile().is_err());
    criteria.enforce_weather_dependencies();
    assert!(!criteria.choices.contains_key(&Field::StormType));
    assert!(!criteria.planets.contains_key(&PlanetFlag::ExtremeStorms));

    criteria.choices.insert(Field::StormFrequency, 0b11);
    criteria.choices.insert(Field::StormType, 1 << 2);
    criteria.planets.insert(PlanetFlag::ExtremeStorms, true);
    criteria.enforce_weather_dependencies();
    assert!(criteria.choices.contains_key(&Field::StormType));
    assert!(criteria.planets[&PlanetFlag::ExtremeStorms]);

    criteria.choices.insert(Field::Population, 1 << 1);
    criteria.choices.insert(Field::Race, 1 << 0);
    criteria.systems.insert(SystemFlag::Pirate, true);
    assert!(criteria.compile().is_err());
    criteria.enforce_population_dependencies();
    assert!(!criteria.choices.contains_key(&Field::Race));
    assert!(!criteria.systems.contains_key(&SystemFlag::Pirate));
    assert!(criteria.compile().is_ok());

    criteria.choices.insert(Field::Population, 0b011);
    criteria.choices.insert(Field::Race, (1 << 0) | (1 << 7));
    criteria.systems.insert(SystemFlag::Pirate, true);
    criteria.enforce_population_dependencies();
    assert_eq!(criteria.choices[&Field::Race], (1 << 0) | (1 << 7));
    assert!(criteria.systems[&SystemFlag::Pirate]);
    assert!(criteria.compile().is_ok());

    criteria.choices.insert(Field::Population, 1 << 2);
    criteria.choices.insert(Field::Race, 1 << 0);
    criteria.enforce_population_dependencies();
    assert_eq!(criteria.choices[&Field::Race], 1 << 0);

    for population in [1 << 1, 1 << 2, (1 << 1) | (1 << 2)] {
        let mut unavailable = Criteria::default();
        unavailable.choices.insert(Field::Population, population);
        unavailable.choices.insert(Field::Wealth, 1);
        unavailable.choices.insert(Field::Economy, 1);
        unavailable.choices.insert(Field::ConflictLevel, 1);
        unavailable.enforce_population_dependencies();
        for field in [Field::Wealth, Field::Economy, Field::ConflictLevel] {
            assert!(!unavailable.choices.contains_key(&field));
            assert_eq!(unavailable.available_choice_mask(field), 0);
        }
    }

    let mut mixed = Criteria::default();
    mixed.choices.insert(Field::Population, (1 << 0) | (1 << 2));
    mixed.choices.insert(Field::Wealth, 1);
    mixed.choices.insert(Field::Economy, 1);
    mixed.choices.insert(Field::ConflictLevel, 1);
    mixed.enforce_population_dependencies();
    for field in [Field::Wealth, Field::Economy, Field::ConflictLevel] {
        assert!(mixed.choices.contains_key(&field));
    }

    let mut none_lifeform = Criteria::default();
    none_lifeform.choices.insert(Field::Race, 1 << 7);
    none_lifeform.choices.insert(Field::Wealth, 1);
    none_lifeform.choices.insert(Field::Economy, 1);
    none_lifeform.choices.insert(Field::ConflictLevel, 1);
    none_lifeform.systems.insert(SystemFlag::Pirate, false);
    none_lifeform.enforce_population_dependencies();
    for field in [Field::Wealth, Field::Economy, Field::ConflictLevel] {
        assert!(!none_lifeform.choices.contains_key(&field));
    }
    assert!(!none_lifeform.systems.contains_key(&SystemFlag::Pirate));

    let mut outlaw = Criteria::default();
    outlaw.systems.insert(SystemFlag::Pirate, true);
    outlaw.choices.insert(Field::Wealth, 0b1111);
    outlaw.choices.insert(Field::Economy, 1);
    outlaw.choices.insert(Field::ConflictLevel, 0b1111);
    outlaw.enforce_population_dependencies();
    assert_eq!(outlaw.choices[&Field::Wealth], 1 << 3);
    assert!(!outlaw.choices.contains_key(&Field::Economy));
    assert_eq!(outlaw.choices[&Field::ConflictLevel], 1 << 3);
}

#[test]
fn latent_economy_values_do_not_make_empty_or_abandoned_systems_match() {
    for population in [1, 2] {
        let mut system = Facts::default();
        system.set(Field::Population, population);
        system.set(Field::Wealth, 0);
        system.set(Field::Economy, 0);
        system.set(Field::ConflictLevel, 0);
        system.set_system_flag(SystemFlag::Pirate, false);

        for field in [Field::Wealth, Field::Economy, Field::ConflictLevel] {
            let mut criteria = Criteria::default();
            criteria.choices.insert(field, 1);
            assert_eq!(
                criteria.compile().unwrap().system_match(&system, &[]),
                Verdict::NoMatch,
                "{field:?} matched population {population}"
            );
        }
    }
}

#[test]
fn target_choices_are_pruned_by_system_wide_exclusions() {
    let mut criteria = Criteria::default();
    criteria.choices.insert(Field::Biome, 0b1_0001);
    criteria.composition.biomes.exclude = 1;
    assert!(criteria.compile().is_err());
    criteria.enforce_dependencies();
    assert_eq!(criteria.biome_choices(), Some(1 << 4));

    criteria.choices.insert(Field::Size, 0b11);
    criteria.composition.sizes.exclude = 1;
    assert!(criteria.compile().is_err());
    criteria.enforce_dependencies();
    assert_eq!(criteria.choices[&Field::Size], 1 << 1);

    criteria.set_biome_choices(1 << 15);
    assert_eq!(criteria.derived_planet_size(), Some(4));
    assert!(!criteria.choices.contains_key(&Field::Size));

    criteria.composition.sizes.exclude = 1 << 4;
    criteria.enforce_dependencies();
    assert_eq!(criteria.biome_choices(), Some(0));
    assert_eq!(criteria.derived_planet_size(), None);
}

#[test]
fn aggregate_system_exclusions_remove_impossible_target_details() {
    let mut criteria = Criteria::default();
    criteria
        .choices
        .insert(Field::Sentinels, (1 << 1) | (1 << 3));
    criteria.systems.insert(SystemFlag::Dissonance, false);
    assert!(criteria.compile().is_err());
    criteria.enforce_dependencies();
    assert_eq!(criteria.sentinel_choices(), Some(1 << 1));

    criteria.planets.insert(PlanetFlag::ExtremeStorms, true);
    criteria.systems.insert(SystemFlag::ExtremeStorms, false);
    criteria.enforce_dependencies();
    assert!(!criteria.planets.contains_key(&PlanetFlag::ExtremeStorms));

    criteria.choices.insert(Field::WaterColour, 1);
    criteria.planet_water = WaterFilter::RequireDeep;
    criteria.system_water = WaterFilter::ExcludeAll;
    assert!(criteria.compile().is_err());
    criteria.enforce_dependencies();
    assert_eq!(criteria.planet_water, WaterFilter::Any);
    assert!(!criteria.choices.contains_key(&Field::WaterColour));
}

#[test]
fn star_count_selection_uses_the_generated_system_count() {
    let mut criteria = Criteria {
        star_counts: 0b110,
        ..Criteria::default()
    };
    let plan = criteria.compile().unwrap();
    for (count, expected) in [
        (1, Verdict::NoMatch),
        (2, Verdict::Match),
        (3, Verdict::Match),
    ] {
        let mut facts = Facts::default();
        facts.set_star_count(count);
        assert_eq!(plan.system_match(&facts, &[]), expected);
    }
    criteria.star_counts = 0b1000;
    assert!(criteria.compile().is_err());
}

#[test]
fn planet_count_selection_accepts_only_selected_exact_counts() {
    let criteria = Criteria {
        planet_counts: 0b10_1010,
        ..Criteria::default()
    };
    let plan = criteria.compile().unwrap();
    for count in 1..=6 {
        let mut facts = Facts::default();
        facts.set_planet_count(count);
        let expected = if [2, 4, 6].contains(&count) {
            Verdict::Match
        } else {
            Verdict::NoMatch
        };
        assert_eq!(plan.system_match(&facts, &[]), expected);
    }
}

#[test]
fn composition_unions_include_any_selected_value_and_exclude_every_selected_value() {
    let mut criteria = Criteria::default();
    criteria.composition.biomes.include = 0b11;
    criteria.composition.biomes.exclude = 0b1100;
    criteria.composition.sizes.include = 0b11;
    let plan = criteria.compile().unwrap();
    for biome in 0..5 {
        for size in 0..3 {
            let mut planet = Facts::default();
            planet.set(Field::Biome, biome);
            planet.set(Field::Size, size);
            let expected = if biome < 2 && size < 2 {
                Verdict::Match
            } else {
                Verdict::NoMatch
            };
            assert_eq!(
                plan.system_match(&Facts::default(), &[planet]),
                expected,
                "{biome}, {size}"
            );
        }
    }
    let mut allowed = Facts::default();
    allowed.set(Field::Biome, 0);
    allowed.set(Field::Size, 0);
    for biome in [2, 3] {
        let mut excluded = Facts::default();
        excluded.set(Field::Biome, biome);
        excluded.set(Field::Size, 0);
        assert_eq!(
            plan.system_match(&Facts::default(), &[allowed.clone(), excluded]),
            Verdict::NoMatch
        );
    }
}

#[test]
fn composition_subtypes_stay_bound_to_the_same_planet_and_negative_scope() {
    let mut criteria = Criteria::default();
    criteria.composition.biomes.include = 1;
    // Two supported, separately selectable subtype alternatives for biome zero.
    let alternatives = search_probes_model::criteria::subtype_options_for(1);
    let subtypes = alternatives[0].0 | alternatives[1].0;
    criteria.composition.subtypes.include = subtypes;
    let plan = criteria.compile().unwrap();
    let planet = |biome, subtype| {
        let mut facts = Facts::default();
        facts.set(Field::Biome, biome);
        facts.set(Field::Subtype, subtype);
        facts
    };
    for choice in 0..29 {
        let expected = if subtypes & (1 << choice) != 0 {
            Verdict::Match
        } else {
            Verdict::NoMatch
        };
        assert_eq!(
            plan.system_match(&Facts::default(), &[planet(0, choice + 1)]),
            expected
        );
    }
    let selected = subtypes.trailing_zeros() + 1;
    let unselected = (!subtypes).trailing_zeros() + 1;
    assert_eq!(
        plan.system_match(
            &Facts::default(),
            &[planet(0, unselected), planet(1, selected)]
        ),
        Verdict::NoMatch
    );

    criteria.composition.biomes.include = 0;
    criteria.composition.subtypes.include = 0;
    criteria.composition.biomes.exclude = 1;
    criteria.composition.subtypes.exclude = subtypes;
    let negative = criteria.compile().unwrap();
    assert_eq!(
        negative.system_match(&Facts::default(), &[planet(0, selected)]),
        Verdict::NoMatch
    );
    assert_eq!(
        negative.system_match(
            &Facts::default(),
            &[planet(0, unselected), planet(1, selected)]
        ),
        Verdict::Match
    );
}

#[test]
fn pair_scoped_subtype_exclusions_preserve_the_same_subtype_on_other_biomes() {
    let mut criteria = Criteria::default();
    criteria.set_biome_choices(1 << 1); // Toxic
    criteria.choices.insert(Field::Subtype, 1 << 2); // Ruins
    criteria.composition.biomes.exclude = 1 << 0; // Lush
    criteria.composition.subtypes.exclude = 1 << 2; // Ruins on Lush

    assert!(
        criteria
            .subtype_options()
            .iter()
            .any(|(mask, label)| *label == "Ruins" && mask & (1 << 2) != 0)
    );
    let plan = criteria.compile().unwrap();
    let mut toxic_ruins = Facts::default();
    toxic_ruins.set(Field::Biome, 1);
    toxic_ruins.set(Field::Subtype, 3);
    assert_eq!(plan.planet_match(&toxic_ruins), Verdict::Match);
    assert_eq!(
        plan.system_match(&Facts::default(), &[toxic_ruins]),
        Verdict::Match
    );
}

#[test]
fn negative_flags_and_mixed_system_planet_filters_are_all_required() {
    let mut criteria = Criteria::default();
    criteria.choices.insert(Field::Star, 0b11);
    criteria.systems.insert(SystemFlag::Pirate, false);
    criteria.planets.insert(PlanetFlag::Rings, false);
    criteria.choices.insert(Field::Size, 0b11);
    let plan = criteria.compile().unwrap();
    for star in 0..3 {
        for pirate in [false, true] {
            for size in 0..3 {
                for rings in [false, true] {
                    let mut system = Facts::default();
                    system.set(Field::Star, star);
                    system.set_system_flag(SystemFlag::Pirate, pirate);
                    let mut planet = Facts::default();
                    planet.set(Field::Size, size);
                    planet.set_planet_flag(PlanetFlag::Rings, rings);
                    let address = Address::new(Coordinates {
                        x: 0,
                        y: 0,
                        z: 0,
                        galaxy: 0,
                        system: 0,
                        planet: 1,
                    })
                    .unwrap();
                    let world = CurrentWorld::capture(
                        1,
                        address,
                        criteria.clone(),
                        &plan,
                        &system,
                        &[planet],
                        None,
                    )
                    .unwrap();
                    let expected = if star < 2 && !pirate && size < 2 && !rings {
                        Verdict::Match
                    } else {
                        Verdict::NoMatch
                    };
                    assert_eq!(
                        world.target_verdict, expected,
                        "{star}, {pirate}, {size}, {rings}"
                    );
                    let export = serde_json::to_value(world).unwrap();
                    for section in export["section_evaluations"].as_array().unwrap() {
                        assert!(section.get("mode").is_none());
                    }
                }
            }
        }
    }
}

fn species(family: Family, rarity: u8) -> Species {
    Species {
        role: 0,
        name_identity: NameIdentity {
            species_seed: 1,
            genus_seed: 2,
        },
        family,
        domain: Domain::Land,
        activity: 2,
        rarity,
        cave: false,
        hemisphere: 0,
        heights_cm: [100, 120],
    }
}

#[test]
fn fauna_unions_require_all_fields_on_the_same_animal() {
    let mut fauna = FaunaCriteria {
        presence: Some(true),
        ..FaunaCriteria::default()
    };
    fauna.choices.insert(
        FaunaField::Family,
        (1 << Family::Cats as usize) | (1 << Family::Cattle as usize),
    );
    fauna.choices.insert(FaunaField::Rarity, 0b11);
    let selection = fauna.compile(Domain::Land).unwrap().unwrap();
    for family in [Family::Cats, Family::Cattle, Family::Rodents] {
        for rarity in 0..3 {
            let animals = [species(family, rarity)];
            let accepted = matches!(family, Family::Cats | Family::Cattle) && rarity < 2;
            assert_eq!(
                selection.evaluate(&animals),
                if accepted {
                    Verdict::Match
                } else {
                    Verdict::NoMatch
                }
            );
            assert_eq!(!selection.witnesses(&animals).is_empty(), accepted);
        }
    }
    assert_eq!(
        selection.evaluate(&[species(Family::Cats, 2), species(Family::Rodents, 0)]),
        Verdict::NoMatch
    );
    fauna.presence = Some(false);
    fauna.enforce_dependencies();
    assert!(fauna.choices.is_empty());
    let negative = fauna.compile(Domain::Land).unwrap().unwrap();
    assert_eq!(
        negative.evaluate(&[species(Family::Cats, 0)]),
        Verdict::NoMatch
    );
    assert_eq!(negative.evaluate(&[]), Verdict::Match);
}

#[test]
fn fauna_details_require_presence_and_exclusion_clears_details() {
    let mut fauna = FaunaCriteria::default();
    fauna.choices.insert(FaunaField::Rarity, 1 << 2);
    assert!(fauna.compile(Domain::Land).is_err());
    fauna.enforce_dependencies();
    assert_eq!(fauna.presence, Some(true));
    assert!(fauna.compile(Domain::Land).is_ok());

    fauna.presence = Some(false);
    fauna.height.minimum_cm = Some(100);
    fauna.enforce_dependencies();
    assert!(fauna.choices.is_empty());
    assert_eq!(fauna.height, Default::default());
    assert_eq!(fauna.presence, Some(false));
}

#[test]
fn fixed_fauna_and_arena_relationships_prune_impossible_values() {
    let mut fauna = FaunaCriteria::default();
    fauna.choices.insert(FaunaField::Category, 1 << 1);
    fauna.choices.insert(
        FaunaField::Family,
        (1 << Family::Antelopes as usize) | (1 << Family::RobotAntelopes as usize),
    );
    assert!(fauna.compile(Domain::Land).is_err());
    fauna.enforce_domain_dependencies(Domain::Land);
    assert_eq!(
        fauna.choices[&FaunaField::Family],
        1 << Family::RobotAntelopes as usize
    );

    let mut arena = ArenaCriteria::default();
    for field in [ArenaField::Combat, ArenaField::Agility, ArenaField::Health] {
        arena.choices.insert(field, 1 << 3);
    }
    arena.choices.insert(ArenaField::OverallRank, 0b1111);
    arena.presence = Some(true);
    assert!(arena.compile().is_err());
    arena.enforce_dependencies();
    assert_eq!(arena.choices[&ArenaField::OverallRank], 1 << 3);
}

#[test]
fn planet_life_and_sentinel_dependencies_remove_redundant_details() {
    let mut flora = Criteria::default();
    flora.choices.insert(Field::Flora, 1);
    flora.choices.insert(Field::Grass, 1);
    flora.planets.insert(PlanetFlag::GlowingGrass, true);
    assert!(flora.compile().is_err());
    flora.enforce_dependencies();
    assert!(!flora.choices.contains_key(&Field::Grass));
    assert!(!flora.planets.contains_key(&PlanetFlag::GlowingGrass));

    let mut fauna = Criteria::default();
    fauna.choices.insert(Field::Fauna, 1);
    fauna.fauna.insert(
        Domain::Land,
        FaunaCriteria {
            presence: Some(true),
            ..Default::default()
        },
    );
    fauna.arena.presence = Some(true);
    fauna.planets.insert(PlanetFlag::PlayerAttackingFauna, true);
    assert!(fauna.compile().is_err());
    fauna.enforce_dependencies();
    assert!(fauna.fauna.is_empty());
    assert_eq!(fauna.arena, ArenaCriteria::default());
    assert!(
        !fauna
            .planets
            .contains_key(&PlanetFlag::PlayerAttackingFauna)
    );

    let mut sentinels = Criteria::default();
    sentinels
        .choices
        .insert(Field::Sentinels, (1 << 1) | (1 << 2));
    sentinels.planets.insert(PlanetFlag::Sentinels, true);
    sentinels.planets.insert(PlanetFlag::ExtremeSentinels, true);
    sentinels
        .planets
        .insert(PlanetFlag::CorruptSentinels, false);
    sentinels.enforce_dependencies();
    assert_eq!(sentinels.choices[&Field::Sentinels], 1 << 2);
    for flag in PlanetFlag::SENTINEL_LEVEL_PROJECTIONS {
        assert!(!sentinels.planets.contains_key(flag));
    }
}

#[test]
fn direct_sentinel_level_projections_cannot_create_impossible_conjunctions() {
    for flags in [
        [
            (PlanetFlag::ExtremeSentinels, true),
            (PlanetFlag::CorruptSentinels, true),
        ],
        [
            (PlanetFlag::Sentinels, false),
            (PlanetFlag::ExtremeSentinels, true),
        ],
    ] {
        let mut criteria = Criteria::default();
        criteria.planets.extend(flags);
        assert_eq!(
            criteria.compile(),
            Err("sentinel filters have no possible shared level")
        );
        criteria.enforce_dependencies();
        assert!(criteria.compile().is_err());
    }

    let mut direct = Criteria::default();
    direct.planets.insert(PlanetFlag::ExtremeSentinels, true);
    assert_eq!(
        direct.compile(),
        Err("use the unified Sentinels filter instead of sentinel level projections")
    );
}

#[test]
fn fauna_domains_and_general_filters_are_independent_conjuncts() {
    let mut criteria = Criteria::default();
    criteria.fauna.insert(
        Domain::Land,
        FaunaCriteria {
            presence: Some(true),
            ..FaunaCriteria::default()
        },
    );
    criteria.fauna.insert(
        Domain::Airborne,
        FaunaCriteria {
            presence: Some(false),
            ..FaunaCriteria::default()
        },
    );
    criteria.planets.insert(PlanetFlag::Sandworms, false);
    let plan = criteria.compile().unwrap();
    for land in [false, true] {
        for airborne in [false, true] {
            for sandworms in [false, true] {
                let mut animals = Vec::new();
                if land {
                    animals.push(species(Family::Cats, 0));
                }
                if airborne {
                    let mut bird = species(Family::Birds, 0);
                    bird.domain = Domain::Airborne;
                    animals.push(bird);
                }
                let mut facts = Facts::default();
                facts.set_fauna(animals);
                facts.set_planet_flag(PlanetFlag::Sandworms, sandworms);
                let expected = if land && !airborne && !sandworms {
                    Verdict::Match
                } else {
                    Verdict::NoMatch
                };
                assert_eq!(plan.planet_match(&facts), expected);
            }
        }
    }
}

#[test]
fn arena_unions_require_all_fields_on_the_same_creature() {
    let mut arena = ArenaCriteria::default();
    arena.choices.insert(ArenaField::Combat, 0b11);
    arena.choices.insert(ArenaField::Health, 1 << 3);
    assert!(arena.compile().is_err());
    arena.enforce_dependencies();
    assert_eq!(arena.presence, Some(true));
    let selections = arena.compile().unwrap();
    let selection = selections[1];
    let creature = |combat, health| Creature {
        role: 0,
        affinity: 1,
        ranks: RankEvidence::exact(Ranks {
            combat,
            agility: 0,
            health,
        })
        .unwrap(),
    };
    for combat in 0..4 {
        for health in 0..4 {
            let animals = [creature(combat, health)];
            let accepted = combat < 2 && health == 3;
            assert_eq!(
                selection.evaluate(&animals),
                if accepted {
                    Verdict::Match
                } else {
                    Verdict::NoMatch
                }
            );
            assert_eq!(!selection.witnesses(&animals).is_empty(), accepted);
        }
    }
    assert_eq!(
        selection.evaluate(&[creature(0, 0), creature(3, 3)]),
        Verdict::NoMatch
    );
    assert_eq!(selection.evaluate(&[]), Verdict::NoMatch);

    arena.presence = Some(false);
    arena.enforce_dependencies();
    assert!(arena.choices.is_empty());
}

#[test]
fn criteria_contract_rejects_unknown_or_incomplete_fields() {
    assert!(serde_json::from_value::<Criteria>(json!({"unexpected": true})).is_err());
    assert!(serde_json::from_value::<Criteria>(json!({"resources": {"selected": 3}})).is_err());
    let mut criteria = Criteria::default();
    criteria.resources.selected = 3;
    let export = serde_json::to_value(&criteria).unwrap();
    assert_eq!(export["resources"], json!({"selected": 3}));
    assert_eq!(
        serde_json::from_value::<Criteria>(export).unwrap(),
        criteria
    );
}
