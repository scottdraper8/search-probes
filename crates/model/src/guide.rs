use crate::settings::MAX_PRESETS;

pub fn mission(slot: usize) -> Result<String, &'static str> {
    validate(slot)?;
    Ok(format!("SQN_SP_P{:02}", slot + 1))
}

fn validate(slot: usize) -> Result<(), &'static str> {
    if slot < MAX_PRESETS {
        Ok(())
    } else {
        Err("invalid Guide preset slot")
    }
}

pub fn mission_slot(name: &[u8; 16]) -> Option<usize> {
    let prefix = b"SQN_SP_P0";
    if name[..prefix.len()] != *prefix
        || !(b'1'..=b'9').contains(&name[prefix.len()])
        || name[prefix.len() + 1..].iter().any(|&byte| byte != 0)
    {
        return None;
    }
    Some(usize::from(name[prefix.len()] - b'1'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_exact_owned_missions_dispatch() {
        for slot in 0..MAX_PRESETS {
            let mission = mission(slot).unwrap();
            let mut bytes = [0; 16];
            bytes[..mission.len()].copy_from_slice(mission.as_bytes());
            assert_eq!(mission_slot(&bytes), Some(slot));
            bytes[15] = 1;
            assert_eq!(mission_slot(&bytes), None);
            assert!(mission.len() < 16);
        }
        assert_eq!(mission_slot(&[0; 16]), None);
        assert!(mission(MAX_PRESETS).is_err());
    }
}
