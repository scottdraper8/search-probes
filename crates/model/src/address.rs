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
