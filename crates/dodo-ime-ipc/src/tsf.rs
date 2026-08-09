//! Stable identifiers for dodo's Windows Text Services Framework host.
//!
//! The DLL and dodo's installer are separate processes, so the COM class and
//! language-profile identifiers belong in the shared contract rather than in
//! either implementation. They are per-user registrations under `HKCU`; never
//! reuse either GUID for another input method.

/// The name of the packaged TSF COM server.
pub const DLL_NAME: &str = "dodo_ime_windows.dll";
/// The directory beside `dodo.exe` that carries the TSF artifact.
pub const PACKAGE_DIRECTORY: &str = "input-method";
/// The COM class dodo registers as its text input processor.
pub const CLSID: &str = "{B97610DC-4C6B-457D-9B44-AD82B79A6789}";
/// The Vietnamese language profile exposed by that text input processor.
pub const PROFILE_GUID: &str = "{50DF8D24-EB5D-42EF-B8DF-ABC7FD03DF1E}";
/// Windows' Vietnamese LANGID.
pub const LANGUAGE_ID: u16 = 0x042a;
/// The user-visible profile name Windows shows.
pub const PROFILE_NAME: &str = "Dodo Vietnamese";

#[cfg(test)]
mod tests {
    use super::{CLSID, DLL_NAME, LANGUAGE_ID, PACKAGE_DIRECTORY, PROFILE_GUID};

    #[test]
    fn the_packaged_server_has_stable_identifiers() {
        assert_eq!(DLL_NAME, "dodo_ime_windows.dll");
        assert_eq!(PACKAGE_DIRECTORY, "input-method");
        assert_eq!(LANGUAGE_ID, 0x042a);
        for guid in [CLSID, PROFILE_GUID] {
            assert!(guid.starts_with('{') && guid.ends_with('}'));
            assert_eq!(guid.len(), 38);
        }
        assert_ne!(CLSID, PROFILE_GUID);
    }
}
