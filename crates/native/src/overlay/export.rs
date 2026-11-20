use search_probes_ui::{ExportFormat, ExportRequest};
use std::{ffi::OsString, fs, os::windows::ffi::OsStringExt, path::PathBuf};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::Controls::Dialogs::{
        CommDlgExtendedError, GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
        OFN_PATHMUSTEXIST, OPENFILENAMEW,
    },
};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn filter(format: ExportFormat) -> Vec<u16> {
    let value = match format {
        ExportFormat::Text => "Text files (*.txt)\0*.txt\0All files (*.*)\0*.*\0\0",
        ExportFormat::Csv => "CSV files (*.csv)\0*.csv\0All files (*.*)\0*.*\0\0",
        ExportFormat::Json => "JSON files (*.json)\0*.json\0All files (*.*)\0*.*\0\0",
    };
    value.encode_utf16().collect()
}

pub fn save(hwnd: usize, request: &ExportRequest) -> Result<Option<String>, String> {
    let mut file = vec![0u16; 32_768];
    let suggested = wide(&request.suggested_name());
    file[..suggested.len()].copy_from_slice(&suggested);
    let filter = filter(request.format);
    let title = wide(&request.dialog_title());
    let extension = wide(request.format.extension());
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd as HWND,
        lpstrFilter: filter.as_ptr(),
        nFilterIndex: 1,
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        lpstrTitle: title.as_ptr(),
        Flags: OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        lpstrDefExt: extension.as_ptr(),
        ..Default::default()
    };
    if unsafe { GetSaveFileNameW(&mut dialog) } == 0 {
        let error = unsafe { CommDlgExtendedError() };
        return if error == 0 {
            Ok(None)
        } else {
            Err(format!("Windows save dialog failed with code {error:#x}"))
        };
    }
    let length = file
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(file.len());
    if length == 0 {
        return Err("Windows save dialog returned an empty path".into());
    }
    let path = PathBuf::from(OsString::from_wide(&file[..length]));
    fs::write(&path, request.contents.as_bytes())
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(Some(path.display().to_string()))
}
