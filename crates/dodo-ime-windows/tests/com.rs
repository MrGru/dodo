#![cfg(target_os = "windows")]

//! Windows-native boundary check: class-factory ABI and TSF interface identity.
//!
//! It deliberately does not register the DLL, change input sources, or type
//! input. CI runs it on a Windows runner; a desktop still needs the documented
//! end-to-end install and typing pass.

use std::ptr::null_mut;

use dodo_ime_windows::DllGetClassObject;
use windows::Win32::Foundation::{E_NOINTERFACE, S_OK};
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::UI::TextServices::{ITfTextInputProcessor, ITfTextInputProcessorEx};
use windows::core::{GUID, IUnknown, Interface};

const CLASS_ID: GUID = GUID::from_u128(0xb97610dc_4c6b_457d_9b44_ad82b79a6789);
const UNSUPPORTED_ID: GUID = GUID::from_u128(0x2eb6f0a7_2f86_46fa_b8d3_034d71590e5b);

fn factory() -> IClassFactory {
    let mut object = null_mut();
    let result = unsafe { DllGetClassObject(&CLASS_ID, &IClassFactory::IID, &mut object) };
    assert_eq!(result, S_OK);
    assert!(!object.is_null());
    unsafe { IClassFactory::from_raw(object) }
}

#[test]
fn factory_creates_base_and_extended_text_input_processors() {
    let processor: ITfTextInputProcessor = unsafe {
        factory()
            .CreateInstance(None::<&IUnknown>)
            .expect("factory must expose the base text-input-processor interface")
    };
    let extended: ITfTextInputProcessorEx = processor
        .cast()
        .expect("processor must expose the extended text-input-processor interface");

    // Both interface views must retain COM's one object identity.
    let base_identity: IUnknown = processor.cast().unwrap();
    let extended_identity: IUnknown = extended.cast().unwrap();
    assert_eq!(base_identity.as_raw(), extended_identity.as_raw());

    let mut unsupported = null_mut();
    assert_eq!(
        unsafe { processor.query(&UNSUPPORTED_ID, &mut unsupported) },
        E_NOINTERFACE
    );
    assert!(unsupported.is_null());
}
