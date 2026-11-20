use search_probes_model::{
    address::{Address, Coordinates},
    arena::{ArenaCriteria, ArenaField, Creature, RankEvidence, Ranks},
    criteria::{Criteria, Field, PlanetFlag, Section, SystemFlag},
    facts::{Facts, Verdict},
    fauna::{Domain, Family, FaunaCriteria, FaunaField, NameIdentity, Species},
    runtime::CurrentWorld,
    vocabulary::UNIQUE_RESOURCE_SET,
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
    criteria.choices.insert(Field::UniqueResources, 0b11);
    criteria.choices.insert(Field::ResourceRichness, 1 << 1);
    let plan = criteria.compile().unwrap();
    for resources in [0, 1, 2, 4, 8, 16, 31] {
        for unique in 0..4 {
            for richness in 0..3 {
                let mut facts = Facts::default();
                facts.set_resources(resources);
                facts.set(Field::UniqueResources, UNIQUE_RESOURCE_SET | unique);
                facts.set(Field::ResourceRichness, richness);
                let expected = if resources & 0b1111 != 0 && unique != 0 && richness == 1 {
                    Verdict::Match
                } else {
                    Verdict::NoMatch
                };
                assert_eq!(
                    plan.planet_match(&facts),
                    expected,
                    "{resources}, {unique}, {richness}"
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
    let negative = fauna.compile(Domain::Land).unwrap().unwrap();
    assert_eq!(
        negative.evaluate(&[species(Family::Cats, 0)]),
        Verdict::NoMatch
    );
    assert_eq!(negative.evaluate(&[]), Verdict::NoMatch);
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
    let selection = arena.compile().unwrap()[0];
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
}

#[test]
fn removed_mode_fields_are_rejected_instead_of_defaulted_or_ignored() {
    for value in [
        json!({"match_modes": {}}),
        json!({"match_modes": {"resources": "any"}}),
        json!({"unique_resource_mode": "all"}),
        json!({"unique_resource_mode": "any"}),
        json!({"resources": {"selected": 3, "mode": "all"}}),
        json!({"resources": {"selected": 3, "mode": "any"}}),
        json!({"fauna": {"land": {"mode": "all"}}}),
        json!({"arena": {"mode": "any"}}),
    ] {
        assert!(
            serde_json::from_value::<Criteria>(value.clone()).is_err(),
            "accepted {value}"
        );
    }
    let criteria: Criteria = serde_json::from_value(json!({"resources": {"selected": 3}})).unwrap();
    let export = serde_json::to_value(&criteria).unwrap();
    assert!(export.get("match_modes").is_none());
    assert!(export.get("unique_resource_mode").is_none());
    assert_eq!(export["resources"], json!({"selected": 3}));
    assert_eq!(
        serde_json::from_value::<Criteria>(export).unwrap(),
        criteria
    );
}
