use search_probes_model::cache::CachedMatch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Text,
    Csv,
}

impl ExportFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Csv => "csv",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Text => "Text (.txt)",
            Self::Csv => "CSV (.csv)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    pub format: ExportFormat,
    pub contents: String,
}

impl ExportRequest {
    pub fn new(format: ExportFormat, results: &[CachedMatch]) -> Self {
        let contents = match format {
            ExportFormat::Text => text(results),
            ExportFormat::Csv => csv(results),
        };
        Self { format, contents }
    }

    pub fn suggested_name(&self) -> String {
        format!("Search Probes Results.{}", self.format.extension())
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
        );

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
        );

        assert_eq!(
            request.contents,
            "\"Name\",\"Portal coordinates\",\"Galactic coordinates\",\"Domain\"\r\n\
             \"A \"\"quoted\"\" animal\",\"100402003001\",\"0800:0081:0802:0004\",\"surface-dwelling\"\r\n"
        );
    }
}
