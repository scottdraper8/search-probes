use crate::{Result, files};
use quick_xml::{Reader, events::Event};
use std::{collections::BTreeSet, path::Path};

pub const MISSION_TABLE: &str = "METADATA/SIMULATION/MISSIONS/TABLES/WIKIMISSIONTABLE.EXML";

fn fauna_mission(role: u8) -> String {
    format!("SQN_SP_F{role:02}")
}

pub fn expand_fauna_missions(bytes: &[u8]) -> Result<Vec<u8>> {
    let source = std::str::from_utf8(bytes)?;
    let marker =
        r#"<Property name="Missions" value="GcGenericMissionSequence" _id="SQN_SP_FAUNA">"#;
    let start = source
        .find(marker)
        .ok_or("missing fauna mission template")?;
    let next = source[start + marker.len()..]
        .find("\n    <Property name=\"Missions\" value=\"GcGenericMissionSequence\"")
        .ok_or("fauna mission template is not followed by another mission")?;
    let end = start + marker.len() + next;
    let template = &source[start..end];
    if template.matches("_id=\"SQN_SP_FAUNA\"").count() != 1
        || template
            .matches("name=\"MissionID\" value=\"SQN_SP_FAUNA\"")
            .count()
            != 1
        || (0..search_probes_model::target::FAUNA_ROLES)
            .any(|role| source.contains(&format!("_id=\"{}\"", fauna_mission(role))))
    {
        return Err("fauna mission template cannot be expanded safely".into());
    }
    let mut expanded = String::with_capacity(source.len() + template.len() * 30);
    expanded.push_str(&source[..end]);
    for role in 0..search_probes_model::target::FAUNA_ROLES {
        let mission = fauna_mission(role);
        let clone = template
            .replacen("_id=\"SQN_SP_FAUNA\"", &format!("_id=\"{mission}\""), 1)
            .replacen(
                "name=\"MissionID\" value=\"SQN_SP_FAUNA\"",
                &format!("name=\"MissionID\" value=\"{mission}\""),
                1,
            );
        expanded.push('\n');
        expanded.push_str(&clone);
    }
    expanded.push_str(&source[end..]);
    Ok(expanded.into_bytes())
}

fn properties(path: &Path, mission: Option<&str>) -> Result<Vec<(String, String)>> {
    let bytes = std::fs::read(path)?;
    let mut reader = Reader::from_reader(bytes.as_slice());
    let mut result = Vec::new();
    let mut depth = 0;
    let mut selected = None;
    loop {
        let event = reader.read_event()?;
        if matches!(event, Event::Start(_)) {
            depth += 1;
        }
        match event {
            Event::Start(element) | Event::Empty(element)
                if element.name().as_ref() == b"Property" =>
            {
                let mut name = None;
                let mut value = None;
                let mut id = None;
                for attribute in element.attributes() {
                    let attribute = attribute?;
                    let text = attribute
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Explicit1_0,
                            reader.decoder(),
                        )?
                        .into_owned();
                    match attribute.key.as_ref() {
                        b"name" => name = Some(text),
                        b"value" => value = Some(text),
                        b"_id" => id = Some(text),
                        _ => {}
                    }
                }
                if mission.is_some() && id.as_deref() == mission {
                    selected = Some(depth);
                }
                if mission.is_none() || selected.is_some() {
                    if let (Some(name), Some(value)) = (name, value) {
                        result.push((name, value));
                    }
                }
            }
            Event::End(_) => {
                if selected == Some(depth) {
                    selected = None;
                }
                depth -= 1;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(result)
}

pub fn validate(root: &Path, expanded: bool) -> Result<()> {
    let expected: BTreeSet<_> = [
        "LocTable.MXML",
        "METADATA/REALITY/WIKI.EXML",
        "METADATA/SIMULATION/MISSIONS/TABLES/WIKIMISSIONTABLE.EXML",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let actual: BTreeSet<_> = files(root)?
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        })
        .collect::<std::result::Result<_, _>>()?;
    if actual != expected {
        return Err("unexpected asset inventory".into());
    }
    let wiki = properties(&root.join("METADATA/REALITY/WIKI.EXML"), None)?;
    let table = root.join(MISSION_TABLE);
    let mut navigation = Vec::new();
    let localization = properties(&root.join("LocTable.MXML"), None)?;
    let values = |items: &[(String, String)], key: &str| -> Vec<String> {
        items
            .iter()
            .filter(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
            .collect()
    };
    let missions: Vec<_> = (0..search_probes_model::settings::MAX_PRESETS)
        .map(search_probes_model::guide::mission)
        .collect::<std::result::Result<_, _>>()?;
    let mut guide = Vec::new();
    for mission in &missions {
        guide.extend(properties(&table, Some(mission))?);
    }
    let mut expected = missions.clone();
    expected.extend([
        search_probes_model::target::TargetKind::Planet
            .asset_mission()
            .to_owned(),
        search_probes_model::target::TargetKind::System
            .asset_mission()
            .to_owned(),
        search_probes_model::target::LEGACY_FAUNA_MISSION.to_owned(),
    ]);
    if expanded {
        expected.extend((0..search_probes_model::target::FAUNA_ROLES).map(fauna_mission));
    }
    expected.push("SQN_SP_SIGNAL".to_owned());
    if values(&properties(&table, None)?, "MissionID") != expected {
        return Err("unexpected mission table entries".into());
    }
    if values(&wiki, "Mission") != missions
        || values(&guide, "MissionID") != missions
        || !values(&guide, "Name").is_empty()
        || !values(&guide, "Event").is_empty()
    {
        return Err("Guide slot identities do not match the native dispatcher".into());
    }
    let mut targets = vec![
        (
            search_probes_model::target::TargetKind::Planet,
            search_probes_model::target::TargetKind::Planet
                .asset_mission()
                .to_owned(),
        ),
        (
            search_probes_model::target::TargetKind::System,
            search_probes_model::target::TargetKind::System
                .asset_mission()
                .to_owned(),
        ),
        (
            search_probes_model::target::TargetKind::Fauna,
            search_probes_model::target::LEGACY_FAUNA_MISSION.to_owned(),
        ),
    ];
    if expanded {
        targets.extend((0..search_probes_model::target::FAUNA_ROLES).map(|role| {
            (
                search_probes_model::target::TargetKind::Fauna,
                fauna_mission(role),
            )
        }));
    }
    for (kind, mission) in targets {
        use search_probes_model::target::TargetKind;
        let target = properties(&table, Some(&mission))?;
        let (search, location, conditions) = match kind {
            TargetKind::Planet => (
                "Any",
                "PlanetSearch",
                vec!["GcMissionConditionIsScanEventOnCurrentPlanet"],
            ),
            TargetKind::System => ("SpaceMarker", "Nearest", vec![]),
            TargetKind::Fauna => (
                "Any",
                "PlanetSearch",
                vec![
                    "GcMissionConditionIsScanEventOnCurrentPlanet",
                    "GcMissionConditionMissionMessage",
                ],
            ),
        };
        if values(&target, "MissionID") != [mission]
            || values(&target, "Name") != [kind.event()]
            || values(&target, "UseScanEventDetailsInLogInfo") != ["true"]
            || values(&target, "ObjectivesCanBeFormattedBySequences") != ["true"]
            || values(&target, "FormattableObjective") != values(&target, "ObjectiveID")
            || values(&target, "FormattableObjectiveTip") != values(&target, "ObjectiveTipID")
            || values(&target, "UAsList") != ["0000000000000000"]
            || values(&target, "SolarSystemLocation") != ["FromList"]
            || values(&target, "SearchType") != [search]
            || values(&target, "BuildingLocation") != [location]
            || values(&target, "AlwaysShow") != ["false"]
            || values(&target, "ShowOnlyIfSequenceTarget") != ["false"]
            || values(&target, "DisableMultiplayerSync") != ["false"]
            || values(&target, "ReplaceEventIfAlreadyActive") != ["false"]
            || values(&target, "Conditions") != conditions
            || values(&target, "GalaxyMapMessage") != ["UI_SCANEVENT_GOTO_MAP"]
            || values(&target, "Distance") != ["1000000000.000000"]
            || values(&target, "EndEventWhenReached") != ["false"]
            || values(&target, "CanFormatObjectives") != ["true"]
            || values(&target, "Event")
                .iter()
                .any(|event| event != kind.event())
        {
            return Err("navigation assets do not match the native destination contract".into());
        }
        let mut stages = vec![
            "GcMissionSequenceEndScanEvent",
            "GcMissionSequenceStartScanEvent",
            "GcMissionSequenceGroup",
            "GcMissionSequenceGetToScanEvent",
        ];
        if kind != TargetKind::System {
            stages.push("GcMissionSequenceWaitForConditions");
        }
        stages.extend(["GcMissionSequenceEndScanEvent"]);
        if kind == TargetKind::Fauna {
            stages.push("GcMissionSequenceWaitForConditions");
        }
        stages.push("GcMissionSequenceShowMissionUpdateMessage");
        if values(&target, "Stage") != stages {
            return Err("unsupported destination lifecycle".into());
        }
        navigation.extend(target);
    }
    let signal = properties(&table, Some("SQN_SP_SIGNAL"))?;
    if values(&signal, "MissionID") != ["SQN_SP_SIGNAL"]
        || values(&signal, "Stage") != ["GcMissionSequenceBroadcastMessage"]
        || values(&signal, "MessageID") != ["SQN_SP_SCANNED"]
        || values(&signal, "Seeded") != ["true"]
    {
        return Err("unsupported creature-scan completion signal".into());
    }
    if !values(&guide, "Stage").is_empty()
        || values(&navigation, "NeedsBiome")
            .iter()
            .any(|v| v != "false")
    {
        return Err("unexpected Guide stage or target biome restriction".into());
    }
    let ids: BTreeSet<_> = values(&localization, "Id").into_iter().collect();
    for (name, value) in wiki.iter().chain(guide.iter()).chain(navigation.iter()) {
        if value.starts_with("SQN_SP_")
            && !matches!(name.as_str(), "Mission" | "MissionID")
            && !ids.contains(value)
        {
            return Err(format!("missing localization {value}").into());
        }
    }
    Ok(())
}
