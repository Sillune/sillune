use anyhow::{Result, bail};

fn target(id: &str) -> Result<String> {
    if id.is_empty()
        || id.len() > 90
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        bail!("Identifiant de secret invalide");
    }
    Ok(format!("Sillune/{id}"))
}

#[cfg(windows)]
pub fn get(id: &str) -> Result<Option<String>> {
    use windows_sys::Win32::Security::Credentials::{
        CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW,
    };
    let name = target(id)?.encode_utf16().chain([0]).collect::<Vec<_>>();
    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    // The returned credential and its blob are owned by Credential Manager until CredFree.
    unsafe {
        if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(1168) {
                return Ok(None);
            }
            return Err(error.into());
        }
        let size = (*credential).CredentialBlobSize as usize;
        let value = if size == 0 {
            Ok(String::new())
        } else {
            String::from_utf8(
                std::slice::from_raw_parts((*credential).CredentialBlob, size).to_vec(),
            )
        };
        CredFree(credential.cast());
        Ok(Some(value?))
    }
}

#[cfg(windows)]
pub fn set(id: &str, value: &str) -> Result<()> {
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredWriteW,
    };
    let mut name = target(id)?.encode_utf16().chain([0]).collect::<Vec<_>>();
    if value.len() > 2500 {
        bail!("Secret trop long pour le Gestionnaire d’identifiants Windows");
    }
    // Every pointer refers to a local buffer that lives for the duration of the call.
    unsafe {
        if value.is_empty() {
            if CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) == 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(1168) {
                    return Err(error.into());
                }
            }
        } else {
            let mut bytes = value.as_bytes().to_vec();
            let mut entry: CREDENTIALW = std::mem::zeroed();
            entry.Type = CRED_TYPE_GENERIC;
            entry.TargetName = name.as_mut_ptr();
            entry.Persist = CRED_PERSIST_LOCAL_MACHINE;
            entry.CredentialBlob = bytes.as_mut_ptr();
            entry.CredentialBlobSize = bytes.len() as u32;
            if CredWriteW(&entry, 0) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn get(id: &str) -> Result<Option<String>> {
    target(id)?;
    Ok(None)
}

#[cfg(not(windows))]
pub fn set(id: &str, _: &str) -> Result<()> {
    target(id)?;
    bail!("Le Gestionnaire d’identifiants Windows est requis")
}
