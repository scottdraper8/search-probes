use search_probes_model::{address::Address, cache::MAX_NAME_BYTES};

type Generate = unsafe extern "system" fn(*const u8, u64, *mut u8, *mut u8);

/// # Safety
/// Requires the verified 7.06 image and its initialized application on the gameplay thread.
/// The native generator reads shared naming tables; all output and RNG storage belongs to this call.
pub unsafe fn generate(base: usize, address: Address) -> Result<String, &'static str> {
    let function = base + 0xe872c0;
    let prefix = [0x48, 0x89, 0x5c, 0x24, 0x10, 0x4c, 0x89, 0x44, 0x24, 0x18];
    if unsafe { std::slice::from_raw_parts(function as *const u8, prefix.len()) } != prefix {
        return Err("system name generator prefix mismatch");
    }
    let application = unsafe { ((base + 0x6e8d708) as *const *const u8).read() };
    if application.is_null() {
        return Err("system name generator has no application");
    }
    let function = unsafe { std::mem::transmute::<usize, Generate>(function) };
    unsafe { invoke(function, application.add(0xcd0), address) }
}

unsafe fn invoke(
    function: Generate,
    generator: *const u8,
    address: Address,
) -> Result<String, &'static str> {
    let mut name = [0u8; 128];
    let mut localized = [0u8; 128];
    unsafe {
        function(
            generator,
            address.system().raw(),
            name.as_mut_ptr(),
            localized.as_mut_ptr(),
        );
    }
    let bytes = if localized[0] != 0 { &localized } else { &name };
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .ok_or("unterminated system name")?;
    if end == 0 {
        return Err("native system name is empty");
    }
    let name = String::from_utf8_lossy(&bytes[..end]);
    let mut end = name.len().min(MAX_NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    Ok(name[..end].into())
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "system" fn native(
        generator: *const u8,
        seed: u64,
        name: *mut u8,
        localized: *mut u8,
    ) {
        assert_eq!(
            seed,
            Address::from_raw(0x0020010000000000)
                .unwrap()
                .system()
                .raw()
        );
        assert_eq!(unsafe { name.read() }, 0);
        assert_eq!(unsafe { localized.read() }, 0);
        let output: &[u8] = if generator.is_null() {
            b"Procedural name\0"
        } else {
            b"\xe6\xb0\xb4\0"
        };
        unsafe {
            std::ptr::copy_nonoverlapping(
                output.as_ptr(),
                if generator.is_null() { name } else { localized },
                output.len(),
            )
        };
    }

    #[test]
    fn naming_uses_system_identity_and_prefers_localized_output() {
        let address = Address::from_raw(0x0020010000000000).unwrap();
        assert_eq!(
            unsafe { invoke(native, std::ptr::null(), address) }.unwrap(),
            "Procedural name"
        );
        assert_eq!(
            unsafe { invoke(native, b"x".as_ptr(), address) }.unwrap(),
            "水"
        );
    }
}
