use search_probes_model::{
    cache::CachedMatch,
    export::{PresetsExport, SearchExport},
    session::SearchRecord,
    settings::Settings,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Text,
    Csv,
    Json,
}

impl ExportFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Text => "Text (.txt)",
            Self::Csv => "CSV (.csv)",
            Self::Json => "Search JSON (.json)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    pub format: ExportFormat,
    pub contents: String,
    subject: &'static str,
}

impl ExportRequest {
    pub fn new(format: ExportFormat, results: &[CachedMatch]) -> Result<Self, String> {
        let contents = match format {
            ExportFormat::Text => text(results),
            ExportFormat::Csv => csv(results),
            ExportFormat::Json => return Err("JSON search export needs a search snapshot".into()),
        };
        Ok(Self {
            format,
            contents,
            subject: "Results",
        })
    }

    pub fn search_json(record: &SearchRecord) -> Result<Self, String> {
        Ok(Self {
            format: ExportFormat::Json,
            contents: serde_json::to_string_pretty(&SearchExport::from(record))
                .map_err(|error| error.to_string())?,
            subject: "Search",
        })
    }

    pub fn presets_json(settings: &Settings) -> Result<Self, String> {
        let snapshot = PresetsExport::new(settings).map_err(str::to_owned)?;
        Ok(Self {
            format: ExportFormat::Json,
            contents: serde_json::to_string_pretty(&snapshot).map_err(|error| error.to_string())?,
            subject: "Presets",
        })
    }

    pub fn suggested_name(&self) -> String {
        format!("Search Probes {}.{}", self.subject, self.format.extension())
    }

    pub fn dialog_title(&self) -> String {
        format!("Export Search Probes {}", self.subject.to_lowercase())
    }
}

fn name(result: &CachedMatch) -> &str {
    result.title().unwrap_or("Name unavailable")
}

fn domain(result: &CachedMatch) -> Option<&'static str> {
    result
        .fauna
        .as_ref()
        .map(|fauna| fauna.domain.dwelling(fauna.cave))
}

fn text(results: &[CachedMatch]) -> String {
    let mut output = String::new();
    for (index, result) in results.iter().enumerate() {
        if index > 0 {
            output.push_str("\r\n");
        }
        output.push_str("Name: ");
        output.push_str(name(result));
        output.push_str("\r\nPortal coordinates: ");
        output.push_str(&result.target().portal_code());
        output.push_str("\r\nGalactic coordinates: ");
        output.push_str(&result.address.galactic_coordinates());
        if let Some(domain) = domain(result) {
            output.push_str("\r\nDomain: ");
            output.push_str(domain);
        }
        output.push_str("\r\n");
    }
    output
}

fn csv_field(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn csv(results: &[CachedMatch]) -> String {
    let mut output =
        String::from("\"Name\",\"Portal coordinates\",\"Galactic coordinates\",\"Domain\"\r\n");
    for result in results {
        output.push_str(&csv_field(name(result)));
        output.push(',');
        output.push_str(&csv_field(&result.target().portal_code()));
        output.push(',');
        output.push_str(&csv_field(&result.address.galactic_coordinates()));
        output.push(',');
        output.push_str(&csv_field(domain(result).unwrap_or("")));
        output.push_str("\r\n");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use search_probes_model::{
        address::{Address, Coordinates},
        cache::FaunaMatch,
        fauna::{Domain, NameIdentity},
        target::TargetKind,
    };

    fn fauna(name: &str, domain: Domain, cave: bool) -> CachedMatch {
        CachedMatch {
            address: Address::new(Coordinates {
                x: 1,
                y: 2,
                z: 3,
                galaxy: 0,
                system: 4,
                planet: 1,
            })
            .unwrap(),
            kind: TargetKind::Fauna,
            system_name: Some("System".into()),
            planet_name: Some("Planet".into()),
            fauna: Some(FaunaMatch {
                role: 0,
                name_identity: NameIdentity {
                    species_seed: 1,
                    genus_seed: 2,
                },
                name: Some(name.into()),
                domain,
                cave,
            }),
        }
    }

    #[test]
    fn text_export_includes_the_mission_domain_wording() {
        let request = ExportRequest::new(
            ExportFormat::Text,
            &[
                fauna("Cave Dweller", Domain::Land, true),
                fauna("Sky Ray", Domain::Airborne, false),
            ],
        )
        .unwrap();

        assert!(request.contents.contains("Name: Cave Dweller"));
        assert!(request.contents.contains("Domain: underground-dwelling"));
        assert!(request.contents.contains("Domain: airborne"));
        assert!(
            request
                .contents
                .contains("Portal coordinates: 100402003001")
        );
        assert!(
            request
                .contents
                .contains("Galactic coordinates: 0800:0081:0802:0004")
        );
    }

    #[test]
    fn csv_export_quotes_names_and_keeps_domain_in_its_own_column() {
        let request = ExportRequest::new(
            ExportFormat::Csv,
            &[fauna("A \"quoted\" animal", Domain::Land, false)],
        )
        .unwrap();

        assert_eq!(
            request.contents,
            "\"Name\",\"Portal coordinates\",\"Galactic coordinates\",\"Domain\"\r\n\
             \"A \"\"quoted\"\" animal\",\"100402003001\",\"0800:0081:0802:0004\",\"surface-dwelling\"\r\n"
        );
    }

    #[test]
    fn json_save_requests_capture_search_and_preset_snapshots_with_distinct_dialog_names() {
        use search_probes_model::{session::Session, settings::Preset};
        let mut settings = Settings::default();
        settings.presets.push(Preset {
            name: "Rings".into(),
            search: settings.search.clone(),
        });
        let mut session = Session::default();
        let origin = fauna("Animal", Domain::Land, false).address.system();
        session
            .start(settings.search.clone(), origin, settings.execution())
            .unwrap();
        let request = ExportRequest::search_json(&session.history()[0]).unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&request.contents).unwrap();
        assert_eq!(snapshot["results"], serde_json::json!([]));
        assert_eq!(snapshot["kind"], "search_probes_search");
        assert_eq!(request.format.extension(), "json");
        assert_eq!(request.suggested_name(), "Search Probes Search.json");
        assert_eq!(request.dialog_title(), "Export Search Probes search");
        let presets = ExportRequest::presets_json(&settings).unwrap();
        settings.presets.clear();
        let snapshot: serde_json::Value = serde_json::from_str(&presets.contents).unwrap();
        assert_eq!(snapshot["presets"].as_array().unwrap().len(), 1);
        assert_eq!(presets.suggested_name(), "Search Probes Presets.json");
        assert!(ExportRequest::new(ExportFormat::Json, &[]).is_err());
    }
}
