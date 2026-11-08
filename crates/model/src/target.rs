use crate::address::Address;
use serde::{Deserialize, Serialize};

const FAUNA_MISSION_PREFIX: &str = "SQN_SP_F";
pub const FAUNA_ROLES: u8 = 30;
/// Mission progress retained while the nested fauna objective waits for its exact discovery.
/// Keep this shared with the checked mission asset contract.
pub const FAUNA_SCAN_READY_PROGRESS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Planet,
    System,
    Fauna,
}

impl TargetKind {
    pub const ALL: [Self; 3] = [Self::Planet, Self::System, Self::Fauna];

    pub fn label(self) -> &'static str {
        match self {
            Self::Planet => "Planet",
            Self::System => "System",
            Self::Fauna => "Animal",
        }
    }

    /// Fixed mission identity for target kinds that do not require a fauna-role slot.
    pub fn fixed_mission(self) -> Option<&'static str> {
        match self {
            Self::Planet => Some("SQN_SP_NAV"),
            Self::System => Some("SQN_SP_SYSTEM"),
            Self::Fauna => None,
        }
    }

    pub fn event(self) -> &'static str {
        match self {
            Self::Planet => "SE_SQN_SP_TARGET",
            Self::System => "SE_SQN_SP_SYSTEM",
            Self::Fauna => "SE_SQN_SP_FAUNA",
        }
    }

    pub fn from_mission(identity: &[u8]) -> Result<Option<(Self, Option<u8>)>, &'static str> {
        let identity = identity.split(|byte| *byte == 0).next().unwrap_or_default();
        if identity == Self::Planet.fixed_mission().unwrap().as_bytes() {
            return Ok(Some((Self::Planet, None)));
        }
        if identity == Self::System.fixed_mission().unwrap().as_bytes() {
            return Ok(Some((Self::System, None)));
        }
        let Some(role) = identity.strip_prefix(FAUNA_MISSION_PREFIX.as_bytes()) else {
            return Ok(None);
        };
        if role.len() != 2 || !role.iter().all(u8::is_ascii_digit) {
            return Err("invalid Search Probes Animal mission identity");
        }
        let role = (role[0] - b'0') * 10 + role[1] - b'0';
        if role >= FAUNA_ROLES {
            return Err("invalid Search Probes Animal mission role");
        }
        Ok(Some((Self::Fauna, Some(role))))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub address: Address,
    pub kind: TargetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fauna_role: Option<u8>,
}

impl Target {
    pub fn validate(self) -> Result<(), &'static str> {
        let coordinates = self.address.coordinates();
        let valid = match self.kind {
            TargetKind::Planet => {
                (1..=8).contains(&coordinates.planet) && self.fauna_role.is_none()
            }
            TargetKind::System => coordinates.planet == 0 && self.fauna_role.is_none(),
            TargetKind::Fauna => {
                (1..=8).contains(&coordinates.planet)
                    && self.fauna_role.is_some_and(|role| role < 30)
            }
        };
        if coordinates.system == 0 || !valid {
            return Err("destination address does not match its target type");
        }
        Ok(())
    }

    pub fn portal_code(self) -> String {
        match self.kind {
            TargetKind::Planet | TargetKind::Fauna => self.address.portal_code(),
            TargetKind::System => self
                .address
                .planet(0)
                .expect("first planet is a valid portal index")
                .portal_code(),
        }
    }

    pub fn mission(self) -> Result<String, &'static str> {
        self.validate()?;
        Ok(match (self.kind, self.fauna_role) {
            (TargetKind::Planet, None) => TargetKind::Planet.fixed_mission().unwrap().to_owned(),
            (TargetKind::System, None) => TargetKind::System.fixed_mission().unwrap().to_owned(),
            (TargetKind::Fauna, Some(role)) => format!("{FAUNA_MISSION_PREFIX}{role:02}"),
            _ => return Err("destination address does not match its target type"),
        })
    }

    pub fn from_mission(identity: &[u8], address: Address) -> Result<Option<Self>, &'static str> {
        let Some((kind, fauna_role)) = TargetKind::from_mission(identity)? else {
            return Ok(None);
        };
        let target = Self {
            address,
            kind,
            fauna_role,
        };
        target.validate()?;
        Ok(Some(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::Coordinates;

    fn address(planet: u8) -> Address {
        Address::new(Coordinates {
            x: 1,
            y: 2,
            z: 3,
            galaxy: 0,
            system: 4,
            planet,
        })
        .unwrap()
    }

    #[test]
    fn fauna_target_requires_a_bounded_role_and_planet() {
        assert!(
            Target {
                address: address(1),
                kind: TargetKind::Fauna,
                fauna_role: Some(29),
            }
            .validate()
            .is_ok()
        );
        assert!(
            Target {
                address: address(1),
                kind: TargetKind::Fauna,
                fauna_role: Some(30),
            }
            .validate()
            .is_err()
        );
        assert!(
            Target {
                address: address(0),
                kind: TargetKind::Fauna,
                fauna_role: Some(0),
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn non_fauna_target_rejects_a_fauna_role() {
        assert!(
            Target {
                address: address(1),
                kind: TargetKind::Planet,
                fauna_role: Some(0),
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn fauna_mission_identity_persists_the_role_without_changing_the_address() {
        let target = Target {
            address: address(3),
            kind: TargetKind::Fauna,
            fauna_role: Some(29),
        };
        let mission = target.mission().unwrap();
        assert_eq!(mission, "SQN_SP_F29");
        assert_eq!(
            Target::from_mission(mission.as_bytes(), target.address).unwrap(),
            Some(target)
        );
    }

    #[test]
    fn invalid_fauna_identity_is_rejected() {
        assert_eq!(
            TargetKind::from_mission(b"SQN_SP_FAUNA"),
            Err("invalid Search Probes Animal mission identity")
        );
    }
}
