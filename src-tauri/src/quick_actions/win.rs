//! The COM half of quick actions: claim tug's AppUserModelID and hand the Tasks to Windows as
//! a jump list (ICustomDestinationList, one IShellLinkW per task pointing back at tug.exe). Thin
//! on purpose: what the tasks are lives in `mod.rs`.

use std::path::Path;

use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, SHStrDupW,
    SetCurrentProcessExplicitAppUserModelID, ShellLink,
};

use super::Task;

pub fn set_process_app_id(id: &str) -> Result<()> {
    // SAFETY: a plain call with a valid, NUL-terminated string that outlives it.
    unsafe { SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(id)) }
}

/// Replace tug's jump list with `tasks`, each launching `exe` with its arguments. Runs on its own
/// thread (see `install`), which it makes a COM apartment for the length of the call.
pub fn commit(app_id: &str, exe: &Path, tasks: &[Task]) -> Result<()> {
    // SAFETY: COM is initialised on this thread for the duration and released only if it was.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = build(app_id, exe, tasks);
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}

/// SAFETY: COM must be initialised on the calling thread.
unsafe fn build(app_id: &str, exe: &Path, tasks: &[Task]) -> Result<()> {
    let list: ICustomDestinationList = CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
    list.SetAppID(&HSTRING::from(app_id))?;
    let mut slots = 0u32;
    // What the person removed from the list: only for custom categories, which tug has none of.
    let _removed: IObjectArray = list.BeginList(&mut slots)?;
    let added = (|| {
        let collection: IObjectCollection = CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
        for task in tasks {
            collection.AddObject(&link(exe, task)?)?;
        }
        list.AddUserTasks(&collection.cast::<IObjectArray>()?)
    })();
    match added {
        Ok(()) => list.CommitList(),
        Err(e) => {
            let _ = list.AbortList();
            Err(e)
        }
    }
}

/// A task: a shell link to tug.exe with the action's arguments, tug's own icon, the title the
/// jump list shows and the description as its tooltip.
///
/// SAFETY: COM must be initialised on the calling thread.
unsafe fn link(exe: &Path, task: &Task) -> Result<IShellLinkW> {
    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
    let exe = HSTRING::from(exe);
    link.SetPath(&exe)?;
    link.SetArguments(&HSTRING::from(task.arguments.as_str()))?;
    link.SetIconLocation(&exe, 0)?;
    link.SetDescription(&HSTRING::from(task.description))?;
    let props: IPropertyStore = link.cast()?;
    props.SetValue(&PKEY_Title, &text_value(task.title)?)?;
    props.Commit()?;
    Ok(link)
}

/// A VT_LPWSTR PROPVARIANT (what the shell expects for a title). The string is allocated with
/// CoTaskMemAlloc (SHStrDupW), which PROPVARIANT's drop (PropVariantClear) frees.
fn text_value(text: &str) -> Result<PROPVARIANT> {
    // SAFETY: SHStrDupW copies a valid string; the union is written as the VT_LPWSTR variant it
    // is then tagged as.
    unsafe {
        let copy = SHStrDupW(&HSTRING::from(text))?;
        let mut value = PROPVARIANT::default();
        let inner = &mut *value.Anonymous.Anonymous;
        inner.vt = VT_LPWSTR;
        inner.Anonymous.pwszVal = copy;
        Ok(value)
    }
}
