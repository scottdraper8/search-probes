use search_probes_model::{
    address::Address,
    target::{Target, TargetKind},
};

pub fn from_seed(kind: TargetKind, seed: u64) -> Result<Target, &'static str> {
    let address = Address::from_raw(seed)?;
    let target = Target { address, kind };
    if target.validate().is_err() {
        return Err("Invalid Search Probes destination. Abandon the mission in the Log.");
    }
    Ok(target)
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
        assert_eq!(
            encode(from_seed(TargetKind::Planet, seed).unwrap().address),
            *b"004422F9BA7FB7FB\0"
        );
        for invalid in [
            0,
            0x000422F9BA7FB7FB,
            0x009422F9BA7FB7FB,
            0x004000F9BA7FB7FB,
            u64::MAX,
        ] {
            assert!(from_seed(TargetKind::Planet, invalid).is_err());
        }
    }
}
