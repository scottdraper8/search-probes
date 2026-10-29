use crate::address::Address;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Planet,
    System,
}

impl TargetKind {
    pub const ALL: [Self; 2] = [Self::Planet, Self::System];

    pub fn label(self) -> &'static str {
        match self {
            Self::Planet => "Planet",
            Self::System => "System",
        }
    }

    pub fn mission(self) -> &'static str {
        match self {
            Self::Planet => "SQN_SP_NAV",
            Self::System => "SQN_SP_SYSTEM",
        }
    }

    pub fn event(self) -> &'static str {
        match self {
            Self::Planet => "SE_SQN_SP_TARGET",
            Self::System => "SE_SQN_SP_SYSTEM",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub address: Address,
    pub kind: TargetKind,
}

impl Target {
    pub fn validate(self) -> Result<(), &'static str> {
        let coordinates = self.address.coordinates();
        let valid_planet = match self.kind {
            TargetKind::Planet => (1..=8).contains(&coordinates.planet),
            TargetKind::System => coordinates.planet == 0,
        };
        if coordinates.system == 0 || !valid_planet {
            return Err("destination address does not match its target type");
        }
        Ok(())
    }

    pub fn portal_code(self) -> String {
        match self.kind {
            TargetKind::Planet => self.address.portal_code(),
            TargetKind::System => self
                .address
                .planet(0)
                .expect("first planet is a valid portal index")
                .portal_code(),
        }
    }
}
