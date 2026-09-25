use search_probes_model::address::Address;

pub fn from_seed(seed: u64) -> Result<Address, &'static str> {
    let address = Address::from_raw(seed)?;
    if !(1..=8).contains(&address.coordinates().planet) || address.coordinates().system == 0 {
        return Err("Invalid Search Probe Target destination. Abandon the mission in the Log.");
    }
    Ok(address)
}

pub fn encode(address: Address) -> [u8; 17] {
    let mut encoded = [0; 17];
    for (index, byte) in encoded[..16].iter_mut().enumerate() {
        *byte = b"0123456789ABCDEF"[((address.raw() >> ((15 - index) * 4)) & 15) as usize];
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mission_seed_preserves_the_complete_destination() {
        let seed = 0x004422F9BA7FB7FB;
        assert_eq!(encode(from_seed(seed).unwrap()), *b"004422F9BA7FB7FB\0");
        for invalid in [
            0,
            0x000422F9BA7FB7FB,
            0x009422F9BA7FB7FB,
            0x004000F9BA7FB7FB,
            u64::MAX,
        ] {
            assert!(from_seed(invalid).is_err());
        }
    }
}
