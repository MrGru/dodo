#![cfg(target_os = "windows")]

//! Windows-native boundary check: class-factory ABI and TSF interface identity.
//!
//! It deliberately does not register the DLL, change input sources, or type
//! input. CI runs it on a Windows runner; a desktop still needs the documented
//! end-to-end install and typing pass.

use std::ptr::null_mut;

use dodo_ime_windows::DllGetClassObject;
use windows::Win32::Foundation::S_OK;
use windows::Win32::UI::TextServices::ITfTextInputProcessor;
use windows::core::{GUID, IUnknown, Interface};

const CLASS_ID: GUID = GUID::from_u128(0xb97610dc_4c6b_457d_9b44_ad82b79a6789);

#[test]
fn the_factory_exposes_a_text_input_processor() {
    let mut object = null_mut();
    let result = unsafe { DllGetClassObject(&CLASS_ID, &ITfTextInputProcessor::IID, &mut object) };
    assert_eq!(result, S_OK);
    assert!(!object.is_null());
    // QueryInterface added a reference for the out pointer. Reclaim it without
    // invoking any host callback or touching TSF registration.
    unsafe { drop(IUnknown::from_raw(object)) };
}
