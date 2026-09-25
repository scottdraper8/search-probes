use std::{ffi::OsString, io, path::PathBuf};
use windows_sys::Win32::System::{
    Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory},
    LibraryLoader::GetModuleFileNameW,
    Threading::GetCurrentProcess,
};

pub fn executable_path() -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = vec![0u16; 32768];
    let length = unsafe {
        GetModuleFileNameW(
            std::ptr::null_mut(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

pub fn read<const N: usize>(address: usize) -> Result<[u8; N], &'static str> {
    if address < 0x10000
        || address
            .checked_add(N)
            .is_none_or(|end| end > 0x0000800000000000)
    {
        return Err("native address outside user memory");
    }
    let mut bytes = [0u8; N];
    let mut copied = 0;
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const _,
            bytes.as_mut_ptr().cast(),
            N,
            &mut copied,
        )
    };
    if ok == 0 || copied != N {
        return Err("native memory is unavailable");
    }
    Ok(bytes)
}

pub fn pointer(address: usize) -> Result<usize, &'static str> {
    let value = u64::from_le_bytes(read(address)?) as usize;
    if !(0x10000..0x0000800000000000).contains(&value) {
        return Err("native pointer is unavailable");
    }
    Ok(value)
}

pub fn u32_at(address: usize) -> Result<u32, &'static str> {
    Ok(u32::from_le_bytes(read(address)?))
}

/// # Safety
/// The caller owns this writable native field for the current gameplay callback.
pub unsafe fn write<const N: usize>(address: usize, bytes: &[u8; N]) -> Result<(), &'static str> {
    read::<N>(address)?;
    let mut copied = 0;
    let ok = unsafe {
        WriteProcessMemory(
            GetCurrentProcess(),
            address as *mut _,
            bytes.as_ptr().cast(),
            N,
            &mut copied,
        )
    };
    if ok == 0 || copied != N || read::<N>(address)? != *bytes {
        return Err("native field write failed");
    }
    Ok(())
}
