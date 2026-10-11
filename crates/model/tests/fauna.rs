use search_probes_model::{
    criteria::{Criteria, PlanetFlag, ResourceMode},
    facts::{Facts, Verdict},
    fauna::{Domain, Family, FaunaCriteria, FaunaField, HeightRange, Species},
};
fn animal(family: Family, domain: Domain, cave: bool, height: u32, rarity: u8) -> Species {
    Species {
        family,
        domain,
        cave,
        activity: 2,
        rarity,
        heights_cm: [height, height + 10],
    }
}
fn criteria(
    domain: Domain,
    choices: &[(FaunaField, u64)],
    height: HeightRange,
    mode: ResourceMode,
) -> Criteria {
    let mut c = Criteria::default();
    c.fauna.insert(
        domain,
        FaunaCriteria {
            choices: choices.iter().copied().collect(),
            height,
            ..Default::default()
        },
    );
    c.match_modes.insert(domain.section(), mode);
    c
}
#[test]
fn all_fields_describe_one_creature_and_any_is_an_alternative() {
    let mut c = criteria(
        Domain::Land,
        &[
            (FaunaField::Family, 1 << Family::Cats as u8),
            (FaunaField::Habitat, 2),
        ],
        HeightRange::default(),
        ResourceMode::All,
    );
    let all = c.compile().unwrap();
    let mut facts = Facts::default();
    assert_eq!(all.planet_match(&facts), Verdict::Unknown);
    facts.set_fauna(vec![
        animal(Family::Cats, Domain::Land, false, 120, 0),
        animal(Family::Rodents, Domain::Land, true, 80, 1),
    ]);
    assert_eq!(all.planet_match(&facts), Verdict::NoMatch);
    c.match_modes
        .insert(Domain::Land.section(), ResourceMode::Any);
    assert_eq!(c.compile().unwrap().planet_match(&facts), Verdict::Match);
    facts.set_fauna(vec![animal(Family::Cats, Domain::Land, true, 120, 0)]);
    assert_eq!(all.planet_match(&facts), Verdict::Match);
}
#[test]
fn independent_domains_and_general_exclusions_apply_together() {
    let mut c = criteria(
        Domain::Land,
        &[(FaunaField::Family, 1 << Family::Cats as u8)],
        HeightRange::default(),
        ResourceMode::All,
    );
    c.fauna.insert(
        Domain::Aquatic,
        FaunaCriteria {
            choices: [(FaunaField::Family, 1 << Family::Fish as u8)].into(),
            ..Default::default()
        },
    );
    c.planets.insert(PlanetFlag::PlayerAttackingFauna, false);
    let plan = c.compile().unwrap();
    let mut f = Facts::default();
    f.set_planet_flag(PlanetFlag::PlayerAttackingFauna, false);
    f.set_fauna(vec![animal(Family::Cats, Domain::Land, false, 120, 0)]);
    assert_eq!(plan.planet_match(&f), Verdict::NoMatch);
    f.set_fauna(vec![
        animal(Family::Cats, Domain::Land, false, 120, 0),
        animal(Family::Fish, Domain::Aquatic, false, 40, 0),
    ]);
    assert_eq!(plan.planet_match(&f), Verdict::Match);
    f.set_planet_flag(PlanetFlag::PlayerAttackingFauna, true);
    assert_eq!(plan.planet_match(&f), Verdict::NoMatch);
}
#[test]
fn height_range_matches_an_actual_reported_gender_height_not_a_span_between_them() {
    let c = criteria(
        Domain::Land,
        &[],
        HeightRange {
            minimum_cm: Some(125),
            maximum_cm: Some(125),
        },
        ResourceMode::All,
    );
    let p = c.compile().unwrap();
    let mut f = Facts::default();
    f.set_fauna(vec![animal(Family::Cats, Domain::Land, false, 120, 0)]);
    assert_eq!(p.planet_match(&f), Verdict::NoMatch);
    f.set_fauna(vec![animal(Family::Cats, Domain::Land, false, 125, 0)]);
    assert_eq!(p.planet_match(&f), Verdict::Match);
}
#[test]
fn unknown_and_known_empty_survive_cache_merges_without_native_pointers() {
    let p = criteria(
        Domain::Airborne,
        &[(FaunaField::Family, 1 << Family::Birds as u8)],
        HeightRange::default(),
        ResourceMode::All,
    )
    .compile()
    .unwrap();
    let mut f = Facts::default();
    assert_eq!(p.planet_match(&f), Verdict::Unknown);
    let mut observation = Facts::default();
    observation.set_fauna(vec![animal(Family::Birds, Domain::Airborne, false, 100, 0)]);
    f.merge(&observation);
    drop(observation);
    assert_eq!(p.planet_match(&f), Verdict::Match);
    f.merge(&Facts::default());
    assert_eq!(p.planet_match(&f), Verdict::Match);
    let clone = f.clone();
    assert!(std::ptr::eq(
        f.fauna().unwrap().as_ptr(),
        clone.fauna().unwrap().as_ptr()
    ));
    let mut empty = Facts::default();
    empty.set_fauna(vec![]);
    f.merge(&empty);
    assert_eq!(p.planet_match(&f), Verdict::NoMatch);
    assert!(!p.missing_planet_requirements(&f).fauna);
}
#[test]
fn irrelevant_domains_and_unavailable_categories_are_rejected() {
    for (domain, field, mask) in [
        (Domain::Airborne, FaunaField::Category, 1 << 2),
        (Domain::Aquatic, FaunaField::Habitat, 1),
        (Domain::Land, FaunaField::Family, 1 << Family::Fish as u8),
    ] {
        assert!(
            criteria(
                domain,
                &[(field, mask)],
                HeightRange::default(),
                ResourceMode::All
            )
            .compile()
            .is_err()
        );
    }
    assert!(
        criteria(
            Domain::Land,
            &[],
            HeightRange {
                minimum_cm: Some(200),
                maximum_cm: Some(100)
            },
            ResourceMode::All
        )
        .compile()
        .is_err()
    );
}
#[test]
fn defaults_request_no_fauna_and_presets_preserve_domain_rules() {
    assert!(!Criteria::default().compile().unwrap().requirements.fauna);
    let c = criteria(
        Domain::Land,
        &[(FaunaField::Family, 1 << Family::PlantCats as u8)],
        HeightRange {
            minimum_cm: Some(150),
            maximum_cm: None,
        },
        ResourceMode::Any,
    );
    let json = serde_json::to_string(&c).unwrap();
    assert_eq!(serde_json::from_str::<Criteria>(&json).unwrap(), c);
    for id in [
        "creature_discovery",
        "creature_taming",
        "robot_creature_discovery",
        "weird_creature_discovery",
    ] {
        assert!(
            serde_json::from_str::<Criteria>(&format!("{{\"planets\":{{\"{id}\":true}}}}"))
                .is_err()
        );
    }
}

#[test]
fn independent_fauna_sections_cannot_be_bypassed_by_general_any() {
    use search_probes_model::criteria::{Field, Section};
    let mut c = criteria(
        Domain::Land,
        &[(FaunaField::Family, 1 << Family::Cats as u8)],
        HeightRange::default(),
        ResourceMode::All,
    );
    c.fauna.insert(
        Domain::Aquatic,
        FaunaCriteria {
            choices: [(FaunaField::Family, 1 << Family::Squid as u8)].into(),
            ..Default::default()
        },
    );
    c.planets.insert(PlanetFlag::TitanWorms, true);
    c.planets.insert(PlanetFlag::PlayerAttackingFauna, false);
    c.match_modes
        .insert(Section::GeneralFauna, ResourceMode::Any);
    let mut f = Facts::default();
    f.set_planet_flag(PlanetFlag::TitanWorms, true);
    f.set_planet_flag(PlanetFlag::PlayerAttackingFauna, true);
    f.set_fauna(vec![animal(Family::Cats, Domain::Land, false, 120, 0)]);
    assert_eq!(c.compile().unwrap().planet_match(&f), Verdict::NoMatch);
    f.set_fauna(vec![
        animal(Family::Cats, Domain::Land, false, 120, 0),
        animal(Family::Squid, Domain::Aquatic, false, 80, 1),
    ]);
    assert_eq!(c.compile().unwrap().planet_match(&f), Verdict::Match);
    c.match_modes.remove(&Section::GeneralFauna);
    assert_eq!(c.compile().unwrap().planet_match(&f), Verdict::NoMatch);
    f.set_planet_flag(PlanetFlag::PlayerAttackingFauna, false);
    assert_eq!(c.compile().unwrap().planet_match(&f), Verdict::Match);
    c.choices.insert(Field::Biome, 1);
    f.set(Field::Biome, 1);
    assert_eq!(c.compile().unwrap().planet_match(&f), Verdict::NoMatch);
}

#[test]
fn subsection_any_stays_local_and_general_success_keeps_needed_species_acquisition() {
    use search_probes_model::criteria::{Requirements, Section};
    let mut c = criteria(
        Domain::Land,
        &[
            (FaunaField::Family, 1 << Family::Cats as u8),
            (FaunaField::Habitat, 2),
        ],
        HeightRange::default(),
        ResourceMode::Any,
    );
    c.fauna.insert(
        Domain::Aquatic,
        FaunaCriteria {
            presence: Some(true),
            ..Default::default()
        },
    );
    c.planets.insert(PlanetFlag::TitanWorms, true);
    c.match_modes
        .insert(Section::GeneralFauna, ResourceMode::Any);
    let mut f = Facts::default();
    f.set_planet_flag(PlanetFlag::TitanWorms, true);
    let p = c.compile().unwrap();
    assert_eq!(p.planet_match(&f), Verdict::Unknown);
    assert!(p.missing_planet_requirements(&f).fauna);
    f.set_fauna(vec![animal(Family::Cats, Domain::Land, false, 120, 0)]);
    assert_eq!(p.planet_match(&f), Verdict::NoMatch);
    f.set_fauna(vec![
        animal(Family::Cats, Domain::Land, false, 120, 0),
        animal(Family::Fish, Domain::Aquatic, false, 80, 0),
    ]);
    assert_eq!(p.planet_match(&f), Verdict::Match);
    assert_eq!(p.missing_planet_requirements(&f), Requirements::default());
    c.match_modes.insert(Section::LandFauna, ResourceMode::All);
    assert_eq!(c.compile().unwrap().planet_match(&f), Verdict::NoMatch);
    // The removed parent mode is not accepted as a preset key.
    assert!(serde_json::from_str::<Criteria>(r#"{"match_modes":{"fauna":"Any"}}"#).is_err());
}

#[test]
fn broad_domain_presence_supports_require_exclude_and_unknowns() {
    for mode in [ResourceMode::All, ResourceMode::Any] {
        for expected in [true, false] {
            let mut c = Criteria::default();
            for &domain in Domain::ALL {
                c.fauna.insert(
                    domain,
                    FaunaCriteria {
                        presence: Some(expected),
                        ..Default::default()
                    },
                );
                c.match_modes.insert(domain.section(), mode);
            }
            let p = c.compile().unwrap();
            let mut f = Facts::default();
            assert_eq!(p.planet_match(&f), Verdict::Unknown);
            f.set_fauna(vec![]);
            assert_eq!(
                p.planet_match(&f),
                if expected {
                    Verdict::NoMatch
                } else {
                    Verdict::Match
                }
            );
            f.set_fauna(vec![
                animal(Family::Cats, Domain::Land, false, 100, 0),
                animal(Family::Squid, Domain::Aquatic, false, 80, 1),
                animal(Family::Birds, Domain::Airborne, false, 100, 0),
            ]);
            assert_eq!(
                p.planet_match(&f),
                if expected {
                    Verdict::Match
                } else {
                    Verdict::NoMatch
                }
            );
            let json = serde_json::to_string(&c).unwrap();
            assert_eq!(serde_json::from_str::<Criteria>(&json).unwrap(), c);
        }
    }
    let mut c = Criteria::default();
    c.match_modes.insert(
        search_probes_model::criteria::Section::GeneralFauna,
        ResourceMode::Any,
    );
    assert_eq!(
        c.compile().unwrap().planet_match(&Facts::default()),
        Verdict::Match
    );
    assert!(!c.compile().unwrap().requirements.fauna);
}

#[test]
fn presence_is_one_subsection_criterion_in_both_match_modes() {
    use search_probes_model::criteria::Section;
    let mut c = criteria(
        Domain::Land,
        &[(FaunaField::Family, 1 << Family::Cats as u8)],
        HeightRange::default(),
        ResourceMode::All,
    );
    c.fauna.get_mut(&Domain::Land).unwrap().presence = Some(false);
    let mut facts = Facts::default();
    facts.set_fauna(vec![animal(Family::Cats, Domain::Land, false, 120, 0)]);
    assert_eq!(c.compile().unwrap().planet_match(&facts), Verdict::NoMatch);
    c.match_modes.insert(Section::LandFauna, ResourceMode::Any);
    assert_eq!(c.compile().unwrap().planet_match(&facts), Verdict::Match);
    facts.set_fauna(vec![]);
    assert_eq!(c.compile().unwrap().planet_match(&facts), Verdict::Match);
    assert_eq!(
        c.compile().unwrap().planet_match(&Facts::default()),
        Verdict::Unknown
    );

    c.fauna.get_mut(&Domain::Land).unwrap().presence = Some(true);
    facts.set_fauna(vec![animal(Family::Rodents, Domain::Land, true, 120, 1)]);
    assert_eq!(c.compile().unwrap().planet_match(&facts), Verdict::Match);
    c.match_modes.insert(Section::LandFauna, ResourceMode::All);
    assert_eq!(c.compile().unwrap().planet_match(&facts), Verdict::NoMatch);
}
