//! Current, standalone JSON snapshots. Runtime addresses and seeds become precision-safe strings.
use crate::{
    address::Address,
    cache::CachedMatch,
    fauna::Domain,
    session::{Finish, Phase, SearchRecord},
    settings::{ExecutionSettings, Preset, SearchOptions, Settings},
    target::TargetKind,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportAddress {
    pub packed_hex: String,
    pub galaxy: u8,
    pub portal_code: String,
    pub galactic_coordinates: String,
}

impl From<Address> for ExportAddress {
    fn from(address: Address) -> Self {
        Self {
            packed_hex: format!("{:014x}", address.raw()),
            galaxy: address.coordinates().galaxy,
            portal_code: address.portal_code(),
            galactic_coordinates: address.galactic_coordinates(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportFauna {
    pub role: u8,
    pub species_seed_hex: String,
    pub genus_seed_hex: String,
    pub name: Option<Box<str>>,
    pub domain: Domain,
    pub cave: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportResult {
    pub address: ExportAddress,
    pub kind: TargetKind,
    pub system_name: Option<Box<str>>,
    pub planet_name: Option<Box<str>>,
    pub fauna: Option<ExportFauna>,
}

impl From<&CachedMatch> for ExportResult {
    fn from(result: &CachedMatch) -> Self {
        let mut address = ExportAddress::from(result.address);
        address.portal_code = result.target().portal_code();
        Self {
            address,
            kind: result.kind,
            system_name: result.system_name.clone(),
            planet_name: result.planet_name.clone(),
            fauna: result.fauna.as_ref().map(|fauna| ExportFauna {
                role: fauna.role,
                species_seed_hex: format!("{:016x}", fauna.name_identity.species_seed),
                genus_seed_hex: format!("{:016x}", fauna.name_identity.genus_seed),
                name: fauna.name.clone(),
                domain: fauna.domain,
                cave: fauna.cave,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchExportKind {
    SearchProbesSearch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchExport {
    pub kind: SearchExportKind,
    pub search_id: u64,
    pub started_unix_ms: u64,
    pub finished_unix_ms: Option<u64>,
    pub search: SearchOptions,
    pub execution: ExecutionSettings,
    pub origin: ExportAddress,
    pub phase: Phase,
    pub finish: Option<Finish>,
    pub user_paused: bool,
    pub elapsed_ms: u64,
    pub systems: u64,
    pub cached_systems: u64,
    pub unresolved_arena_planets: u64,
    pub unresolved_arena_examples: Vec<ExportAddress>,
    pub results: Vec<ExportResult>,
    pub error: Option<String>,
}

impl From<&SearchRecord> for SearchExport {
    fn from(record: &SearchRecord) -> Self {
        let progress = &record.progress;
        Self {
            kind: SearchExportKind::SearchProbesSearch,
            search_id: progress.id,
            started_unix_ms: record.started_unix_ms,
            finished_unix_ms: record.finished_unix_ms,
            search: progress.options.clone(),
            execution: record.execution,
            origin: progress.origin.into(),
            phase: progress.phase,
            finish: progress.finish,
            user_paused: progress.user_paused,
            elapsed_ms: progress.elapsed_ms,
            systems: progress.systems,
            cached_systems: progress.cached_systems,
            unresolved_arena_planets: progress.unresolved_arena_planets,
            unresolved_arena_examples: progress
                .unresolved_arena_examples
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
            results: progress.results.iter().map(Into::into).collect(),
            error: progress.error.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetsExportKind {
    SearchProbesPresets,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetsExport {
    pub kind: PresetsExportKind,
    pub presets: Vec<Preset>,
}

impl PresetsExport {
    pub fn new(settings: &Settings) -> Result<Self, &'static str> {
        settings.validate()?;
        Ok(Self {
            kind: PresetsExportKind::SearchProbesPresets,
            presets: settings.presets.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{address::Coordinates, cache::FaunaMatch, fauna::NameIdentity, session::Session};

    #[test]
    fn search_export_keeps_large_addresses_and_seeds_exact_and_does_not_borrow_live_state() {
        let address = Address::new(Coordinates {
            x: 1,
            y: 2,
            z: 3,
            galaxy: 255,
            system: 4095,
            planet: 15,
        })
        .unwrap();
        assert!(address.raw() > (1_u64 << 53));
        let mut session = Session::default();
        session
            .start(
                SearchOptions::default(),
                address.system(),
                Settings::default().execution(),
            )
            .unwrap();
        let mut record = (*session.history().pop().unwrap()).clone();
        std::sync::Arc::make_mut(&mut record.progress)
            .results
            .push(CachedMatch {
                address,
                kind: TargetKind::Fauna,
                system_name: Some("System".into()),
                planet_name: Some("Planet".into()),
                fauna: Some(FaunaMatch {
                    role: 2,
                    name_identity: NameIdentity {
                        species_seed: u64::MAX,
                        genus_seed: (1_u64 << 63) + 1,
                    },
                    name: Some("A \"quoted\" animal 🐾".into()),
                    domain: Domain::Land,
                    cave: true,
                }),
            });
        let export = SearchExport::from(&record);
        std::sync::Arc::make_mut(&mut record.progress)
            .results
            .clear();
        let value = serde_json::to_value(&export).unwrap();
        assert_eq!(
            value["results"][0]["address"]["packed_hex"],
            format!("{:014x}", address.raw())
        );
        assert_eq!(
            value["results"][0]["fauna"]["species_seed_hex"],
            "ffffffffffffffff"
        );
        assert_eq!(
            value["results"][0]["fauna"]["genus_seed_hex"],
            "8000000000000001"
        );
        let decoded: SearchExport = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(decoded.results[0].address, ExportAddress::from(address));
        let mut unknown = value;
        unknown["obsolete"] = true.into();
        assert!(serde_json::from_value::<SearchExport>(unknown).is_err());
    }

    #[test]
    fn presets_and_empty_current_search_export_without_results() {
        let mut settings = Settings::default();
        settings.presets.push(Preset {
            name: "Rings".into(),
            search: settings.search.clone(),
        });
        let presets = PresetsExport::new(&settings).unwrap();
        settings.presets.clear();
        assert_eq!(presets.presets.len(), 1);
        let value = serde_json::to_value(&presets).unwrap();
        assert_eq!(value["kind"], "search_probes_presets");
        assert!(value.get("interface").is_none());
    }
}
