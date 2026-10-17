//! The COM half of quick actions: claim tug's AppUserModelID and hand the Tasks to Windows as
//! a jump list (ICustomDestinationList, one IShellLinkW per task pointing back at tug.exe). Thin
//! on purpose: what the tasks are lives in `mod.rs`.

use std::path::Path;

use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
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
    // SetValue copies the value; ours is freed when `title` goes out of scope, set or not.
    let title = text_value(task.title)?;
    props.SetValue(&PKEY_Title, &title.0)?;
    props.Commit()?;
    Ok(link)
}

/// A PROPVARIANT that frees what it holds when dropped. windows 0.62's raw PROPVARIANT has no
/// Drop, so without this the title string SHStrDupW allocated leaked once per task, every time
/// the jump list was built.
struct OwnedPropVariant(PROPVARIANT);

impl OwnedPropVariant {
    /// Free the contents (PropVariantClear) and leave it empty (VT_EMPTY). Safe to repeat.
    fn clear(&mut self) {
        // SAFETY: the value was built by `text_value` (or is already empty), which is exactly
        // what PropVariantClear expects.
        unsafe {
            let _ = PropVariantClear(&mut self.0);
        }
    }
}

impl Drop for OwnedPropVariant {
    fn drop(&mut self) {
        self.clear();
    }
}

/// A VT_LPWSTR PROPVARIANT (what the shell expects for a title). The string is allocated with
/// CoTaskMemAlloc (SHStrDupW), which `OwnedPropVariant` frees (PropVariantClear) when dropped.
fn text_value(text: &str) -> Result<OwnedPropVariant> {
    // SAFETY: SHStrDupW copies a valid string; the union is written as the VT_LPWSTR variant it
    // is then tagged as.
    unsafe {
        let copy = SHStrDupW(&HSTRING::from(text))?;
        let mut value = OwnedPropVariant(PROPVARIANT::default());
        let inner = &mut *value.0.Anonymous.Anonymous;
        inner.vt = VT_LPWSTR;
        inner.Anonymous.pwszVal = copy;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Variant::VT_EMPTY;

    #[test]
    fn a_title_value_is_freed_when_cleared() {
        let mut v = text_value("New message").unwrap();
        // SAFETY: reading the tag and pointer of a value this test just built.
        unsafe {
            let inner = &v.0.Anonymous.Anonymous;
            assert_eq!(inner.vt, VT_LPWSTR);
            assert_eq!(inner.Anonymous.pwszVal.to_string().unwrap(), "New message");
        }
        // What Drop does: PropVariantClear frees the string and empties the value.
        v.clear();
        // SAFETY: as above.
        unsafe {
            let inner = &v.0.Anonymous.Anonymous;
            assert_eq!(inner.vt, VT_EMPTY);
            assert!(inner.Anonymous.pwszVal.is_null());
        }
        // Clearing again (Drop after an explicit clear) is harmless.
        v.clear();
    }
}
