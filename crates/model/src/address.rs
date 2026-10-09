use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct Address(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coordinates {
    pub x: i16,
    pub y: i16,
    pub z: i16,
    pub galaxy: u8,
    pub system: u16,
    /// Zero denotes a system; generated planet index n uses n + 1.
    pub planet: u8,
}

impl Address {
    pub fn new(c: Coordinates) -> Result<Self, &'static str> {
        if !(-2047..=2047).contains(&c.x)
            || !(-127..=127).contains(&c.y)
            || !(-2047..=2047).contains(&c.z)
            || c.system > 4095
            || c.planet > 15
        {
            return Err("coordinates outside the native generation bounds");
        }
        Ok(Self(
            (c.x as u64 & 0xfff)
                | ((c.z as u64 & 0xfff) << 12)
                | ((c.y as u64 & 0xff) << 24)
                | ((c.galaxy as u64) << 32)
                | ((c.system as u64) << 40)
                | ((c.planet as u64) << 52),
        ))
    }

    pub fn from_raw(value: u64) -> Result<Self, &'static str> {
        if value >> 56 != 0 {
            return Err("address contains reserved upper bits");
        }
        let address = Self(value);
        Self::new(address.coordinates())
    }

    pub fn raw(self) -> u64 {
        self.0
    }

    pub fn portal_code(self) -> String {
        let c = self.coordinates();
        format!(
            "{:X}{:03X}{:02X}{:03X}{:03X}",
            c.planet,
            c.system,
            c.y as u16 & 0xff,
            c.z as u16 & 0xfff,
            c.x as u16 & 0xfff
        )
    }

    pub fn galactic_coordinates(self) -> String {
        let c = self.coordinates();
        format!(
            "{:04X}:{:04X}:{:04X}:{:04X}",
            c.x + 0x7ff,
            c.y + 0x7f,
            c.z + 0x7ff,
            c.system
        )
    }

    pub fn coordinates(self) -> Coordinates {
        Coordinates {
            x: ((self.0 as i16) << 4) >> 4,
            z: (((self.0 >> 12) as i16) << 4) >> 4,
            y: (self.0 >> 24) as u8 as i8 as i16,
            galaxy: (self.0 >> 32) as u8,
            system: ((self.0 >> 40) & 0xfff) as u16,
            planet: ((self.0 >> 52) & 0xf) as u8,
        }
    }

    pub fn system(self) -> Self {
        Self(self.0 & !(0xf << 52))
    }

    pub fn region(self) -> Self {
        Self(self.0 & ((1 << 40) - 1))
    }

    pub fn planet(self, index: usize) -> Result<Self, &'static str> {
        if index >= 8 {
            return Err("generated planet index outside 0..8");
        }
        Ok(Self(self.system().0 | (((index + 1) as u64) << 52)))
    }
}

impl TryFrom<u64> for Address {
    type Error = &'static str;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::from_raw(value)
    }
}
impl From<Address> for u64 {
    fn from(value: Address) -> Self {
        value.raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_coordinates_and_identity_round_trip() {
        for x in [-2047, -1, 0, 1, 2047] {
            for y in [-127, -1, 0, 1, 127] {
                for z in [-2047, -1, 0, 1, 2047] {
                    let c = Coordinates {
                        x,
                        y,
                        z,
                        galaxy: 255,
                        system: 4095,
                        planet: 0,
                    };
                    let a = Address::new(c).unwrap();
                    assert_eq!(a.coordinates(), c);
                    assert_eq!(Address::from_raw(a.raw()).unwrap(), a);
                    for p in 0..8 {
                        let destination = a.planet(p).unwrap();
                        assert_eq!(destination.coordinates().planet, p as u8 + 1);
                        assert_eq!(destination.system(), a);
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_addresses_fail_closed() {
        assert!(Address::from_raw(1 << 63).is_err());
        assert!(Address::from_raw(0x800).is_err());
        assert!(Address::from_raw(0x80 << 24).is_err());
        assert!(Address::from_raw(0).unwrap().planet(8).is_err());
    }

    #[test]
    fn copied_native_address_exports_portal_and_galactic_formats() {
        let address = Address::from_raw(0x0010_e800_0798_a054).unwrap();
        assert_eq!(address.portal_code(), "10E80798A054");
        assert_eq!(address.galactic_coordinates(), "0853:0086:0189:00E8");
        let other_planet = address.planet(5).unwrap();
        assert_eq!(other_planet.portal_code(), "60E80798A054");
        assert_eq!(
            other_planet.galactic_coordinates(),
            address.galactic_coordinates()
        );
        let other_galaxy = Address::new(Coordinates {
            galaxy: 9,
            ..address.coordinates()
        })
        .unwrap();
        assert_eq!(other_galaxy.portal_code(), address.portal_code());
        assert_eq!(
            other_galaxy.galactic_coordinates(),
            address.galactic_coordinates()
        );
    }

    #[test]
    fn coordinate_exports_preserve_signed_bounds_and_leading_zeroes() {
        for (x, y, z, portal, galactic) in [
            (-2047, -127, -2047, "1FFF81801801", "0000:0000:0000:0FFF"),
            (-1, -1, -1, "1FFFFFFFFFFF", "07FE:007E:07FE:0FFF"),
            (0, 0, 0, "1FFF00000000", "07FF:007F:07FF:0FFF"),
            (2047, 127, 2047, "1FFF7F7FF7FF", "0FFE:00FE:0FFE:0FFF"),
        ] {
            let address = Address::new(Coordinates {
                x,
                y,
                z,
                system: 4095,
                planet: 1,
                galaxy: 255,
            })
            .unwrap();
            assert_eq!(address.portal_code(), portal);
            assert_eq!(address.galactic_coordinates(), galactic);
        }
    }
}
