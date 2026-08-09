//! Standard COM exports and reversible per-user TSF registration.
//!
//! `regsvr32` calls `DllRegisterServer` and `DllUnregisterServer`. Both touch
//! only `HKCU\\Software\\Classes`, so dodo's Install/Reinstall/Uninstall actions
//! need no administrator, service, driver, or elevated helper. The language
//! profile is registered through TSF rather than invented registry keys.

use std::sync::atomic::{AtomicIsize, Ordering};

use dodo_ime_ipc::tsf;
use windows::Win32::Foundation::{BOOL, E_FAIL, E_INVALIDARG, S_FALSE, S_OK};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, IClassFactory, IClassFactory_Impl,
};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW,
};
use windows::Win32::UI::TextServices::{
    CLSID_TF_InputProcessorProfiles, ITfInputProcessorProfiles,
};
use windows::core::{Error, GUID, HRESULT, Interface, PCWSTR, Result, implement};

use crate::service::TextService;

const CLASS_ID: GUID = GUID::from_u128(0xb97610dc_4c6b_457d_9b44_ad82b79a6789);
const PROFILE_ID: GUID = GUID::from_u128(0x50df8d24_eb5d_42ef_b8df_abc7fd03df1e);
const CLASS_NOT_AVAILABLE: HRESULT = HRESULT(0x80040111_u32 as i32);
const NO_AGGREGATION: HRESULT = HRESULT(0x80040110_u32 as i32);

static DLL_MODULE: AtomicIsize = AtomicIsize::new(0);

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory {
    fn CreateInstance(
        &self,
        outer: Option<&windows::core::IUnknown>,
        iid: *const GUID,
        object: *mut *mut core::ffi::c_void,
    ) -> Result<()> {
        if outer.is_some() {
            return Err(Error::from(NO_AGGREGATION));
        }
        if iid.is_null() || object.is_null() {
            return Err(Error::from(E_INVALIDARG));
        }
        let service: windows::Win32::UI::TextServices::ITfTextInputProcessor =
            TextService::new().into();
        unsafe { service.query(iid, object).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> Result<()> {
        // Objects hold their own reference counts. The module reports S_FALSE
        // from DllCanUnloadNow, so TSF never unloads a live callback.
        Ok(())
    }
}

/// COM's class-factory entry point.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    class: *const GUID,
    iid: *const GUID,
    object: *mut *mut core::ffi::c_void,
) -> HRESULT {
    if object.is_null() || class.is_null() || iid.is_null() {
        return E_INVALIDARG;
    }
    unsafe { *object = std::ptr::null_mut() };
    if unsafe { *class } != CLASS_ID {
        return CLASS_NOT_AVAILABLE;
    }
    let factory: IClassFactory = Factory.into();
    unsafe { factory.query(iid, object) }
}

/// Keep the DLL loaded while TSF owns the service.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

/// Store this DLL's module handle. DllMain performs no allocation, COM call, or
/// I/O; the value is read later by DllRegisterServer.
#[unsafe(no_mangle)]
pub extern "system" fn DllMain(
    module: windows::Win32::Foundation::HINSTANCE,
    _reason: u32,
    _reserved: *mut core::ffi::c_void,
) -> BOOL {
    DLL_MODULE.store(module.0, Ordering::Relaxed);
    BOOL(1)
}

/// Registers this DLL and its Vietnamese language profile for the current user.
#[unsafe(no_mangle)]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    register().map_or_else(|error| error.code(), |_| S_OK)
}

/// Removes this DLL's profile and per-user COM registration.
#[unsafe(no_mangle)]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    unregister().map_or_else(|error| error.code(), |_| S_OK)
}

fn register() -> Result<()> {
    let apartment = Apartment::new()?;
    let path = module_path()?;
    write_com_registration(&path)?;

    let profiles: ITfInputProcessorProfiles =
        unsafe { CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER) }?;
    unsafe {
        profiles.Register(&CLASS_ID)?;
        profiles.AddLanguageProfile(
            &CLASS_ID,
            tsf::LANGUAGE_ID,
            &PROFILE_ID,
            &wide(tsf::PROFILE_NAME),
            &path,
            0,
        )?;
        profiles.EnableLanguageProfile(&CLASS_ID, tsf::LANGUAGE_ID, &PROFILE_ID, BOOL(1))?;
    }
    drop(apartment);
    Ok(())
}

fn unregister() -> Result<()> {
    let apartment = Apartment::new()?;
    if let Ok(profiles) = unsafe {
        CoCreateInstance::<_, ITfInputProcessorProfiles>(
            &CLSID_TF_InputProcessorProfiles,
            None,
            CLSCTX_INPROC_SERVER,
        )
    } {
        unsafe {
            let _ = profiles.RemoveLanguageProfile(&CLASS_ID, tsf::LANGUAGE_ID, &PROFILE_ID);
            let _ = profiles.Unregister(&CLASS_ID);
        }
    }
    let subkey = wide(&format!("Software\\Classes\\CLSID\\{}", tsf::CLSID));
    let removed = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr())) };
    // ERROR_FILE_NOT_FOUND is already uninstalled and is success for an
    // idempotent uninstall. Other failures are returned to the UI.
    if removed.0 != 0 && removed.0 != 2 {
        return Err(Error::from_win32());
    }
    drop(apartment);
    Ok(())
}

fn write_com_registration(path: &[u16]) -> Result<()> {
    let key_path = wide(&format!(
        "Software\\Classes\\CLSID\\{}\\InprocServer32",
        tsf::CLSID
    ));
    let mut key = HKEY::default();
    let created = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_path.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if created.0 != 0 {
        return Err(Error::from_win32());
    }
    let result = unsafe {
        windows::Win32::System::Registry::RegSetValueExW(
            key,
            PCWSTR::null(),
            0,
            REG_SZ,
            Some(as_bytes(path)),
        )
    };
    if result.0 != 0 {
        unsafe {
            let _ = RegCloseKey(key);
        };
        return Err(Error::from_win32());
    }
    let name = wide("ThreadingModel");
    let apartment = wide("Apartment");
    let result = unsafe {
        windows::Win32::System::Registry::RegSetValueExW(
            key,
            PCWSTR(name.as_ptr()),
            0,
            REG_SZ,
            Some(as_bytes(&apartment)),
        )
    };
    unsafe {
        let _ = RegCloseKey(key);
    };
    if result.0 != 0 {
        return Err(Error::from_win32());
    }
    Ok(())
}

fn module_path() -> Result<Vec<u16>> {
    let module = DLL_MODULE.load(Ordering::Relaxed);
    if module == 0 {
        return Err(Error::from(E_FAIL));
    }
    let mut path = vec![0_u16; 32_768];
    let length = unsafe {
        GetModuleFileNameW(
            windows::Win32::Foundation::HMODULE(module),
            path.as_mut_slice(),
        )
    } as usize;
    if length == 0 || length >= path.len() {
        return Err(Error::from_win32());
    }
    path.truncate(length);
    path.push(0);
    Ok(path)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn as_bytes(units: &[u16]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(units.as_ptr().cast(), std::mem::size_of_val(units)) }
}

struct Apartment(bool);

impl Apartment {
    fn new() -> Result<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result.is_ok() {
            Ok(Self(true))
        } else {
            Err(Error::from(result))
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}
