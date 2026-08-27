const TARGET: &str = "OmniVox/ai-provider/openrouter";

#[cfg(target_os = "windows")]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
pub fn save_openrouter_key(value: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    let trimmed = value.trim();
    if !trimmed.starts_with("sk-or-") || trimmed.len() < 20 {
        return Err("That does not look like an OpenRouter API key".into());
    }
    let mut target = wide(TARGET);
    let mut username = wide("OmniVox");
    let mut secret = trimmed.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        Comment: std::ptr::null_mut(),
        LastWritten: FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        },
        CredentialBlobSize: secret.len() as u32,
        CredentialBlob: secret.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: std::ptr::null_mut(),
        UserName: username.as_mut_ptr(),
    };
    let ok = unsafe { CredWriteW(&credential, 0) };
    secret.fill(0);
    if ok == 0 {
        Err(format!(
            "Windows could not securely save the API key: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub fn read_openrouter_key() -> Result<Option<String>, String> {
    use windows_sys::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };
    let target = wide(TARGET);
    let mut raw: *mut CREDENTIALW = std::ptr::null_mut();
    let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) };
    if ok == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(1168) {
            return Ok(None);
        }
        return Err(format!("Windows could not read the API key: {error}"));
    }
    if raw.is_null() {
        return Ok(None);
    }
    let credential = unsafe { &*raw };
    let bytes = unsafe {
        std::slice::from_raw_parts(
            credential.CredentialBlob,
            credential.CredentialBlobSize as usize,
        )
    };
    let result = String::from_utf8(bytes.to_vec())
        .map(Some)
        .map_err(|_| "Stored OpenRouter key is invalid UTF-8".to_string());
    unsafe { CredFree(raw.cast()) };
    result
}

#[cfg(target_os = "windows")]
pub fn delete_openrouter_key() -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};
    let target = wide(TARGET);
    let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if ok == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(1168) {
            return Ok(());
        }
        Err(format!("Windows could not delete the API key: {error}"))
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn save_openrouter_key(_value: &str) -> Result<(), String> {
    Err("Secure provider-key storage is currently implemented for Windows".into())
}
#[cfg(not(target_os = "windows"))]
pub fn read_openrouter_key() -> Result<Option<String>, String> {
    Ok(None)
}
#[cfg(not(target_os = "windows"))]
pub fn delete_openrouter_key() -> Result<(), String> {
    Ok(())
}
