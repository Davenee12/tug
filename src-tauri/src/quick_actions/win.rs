//! The COM half of quick actions: claim tug's AppUserModelID and hand the Tasks to Windows as
//! a jump list (ICustomDestinationList, one IShellLinkW per task pointing back at tug.exe). Thin
//! on purpose: what the tasks are lives in `mod.rs`.

use std::path::Path;

use windows::core::{Error, Interface, Result, HSTRING};
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
    // The shell rejects a verbatim `\\?\` target, which is what tauri's current_exe gives.
    let exe = super::shell_path(exe);
    // SAFETY: COM is initialised on this thread for the duration and released only if it was.
    unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let result = build(app_id, &exe, tasks);
        if init.is_ok() {
            CoUninitialize();
        }
        result
    }
}

/// SAFETY: COM must be initialised on the calling thread.
unsafe fn build(app_id: &str, exe: &Path, tasks: &[Task]) -> Result<()> {
    let list: ICustomDestinationList =
        CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER).map_err(at("create DestinationList"))?;
    list.SetAppID(&HSTRING::from(app_id)).map_err(at("SetAppID"))?;
    let mut slots = 0u32;
    // What the person removed from the list: only for custom categories, which tug has none of.
    let _removed: IObjectArray = list.BeginList(&mut slots).map_err(at("BeginList"))?;
    let added = (|| {
        let collection: IObjectCollection = CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)
            .map_err(at("create EnumerableObjectCollection"))?;
        for task in tasks {
            collection
                .AddObject(&link(exe, task)?)
                .map_err(at(format!("link {}: AddObject", task.action.id())))?;
        }
        let array = collection.cast::<IObjectArray>().map_err(at("cast IObjectArray"))?;
        list.AddUserTasks(&array).map_err(at("AddUserTasks"))
    })();
    match added {
        Ok(()) => list.CommitList().map_err(at("CommitList")),
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
    let step = |what: &str| at(format!("link {}: {what}", task.action.id()));
    let link: IShellLinkW =
        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(step("create ShellLink"))?;
    let exe = HSTRING::from(exe);
    link.SetPath(&exe).map_err(step("SetPath"))?;
    link.SetArguments(&HSTRING::from(task.arguments.as_str()))
        .map_err(step("SetArguments"))?;
    link.SetIconLocation(&exe, 0).map_err(step("SetIconLocation"))?;
    link.SetDescription(&HSTRING::from(task.description))
        .map_err(step("SetDescription"))?;
    let props: IPropertyStore = link.cast().map_err(step("cast IPropertyStore"))?;
    // SetValue copies the value; ours is freed when `title` goes out of scope, set or not.
    let title = text_value(task.title).map_err(step("title string"))?;
    props.SetValue(&PKEY_Title, &title.0).map_err(step("SetValue(title)"))?;
    props.Commit().map_err(step("Commit"))?;
    Ok(link)
}

/// Prefix a failure with the step that returned it, so the log names the call
/// ("link new-message: SetPath: The parameter is incorrect. (0x80070057)"), not just the code.
fn at(step: impl Into<String>) -> impl FnOnce(Error) -> Error {
    let step = step.into();
    move |e| Error::new(e.code(), format!("{step}: {}", e.message()))
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

    /// Removes a throwaway jump list when the probe ends, passed or not.
    struct DeleteOnDrop(String);

    impl Drop for DeleteOnDrop {
        fn drop(&mut self) {
            // SAFETY: COM is initialised for the call and released only if this initialised it.
            unsafe {
                let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                if let Ok(list) =
                    CoCreateInstance::<_, ICustomDestinationList>(&DestinationList, None, CLSCTX_INPROC_SERVER)
                {
                    let _ = list.DeleteList(&HSTRING::from(self.0.as_str()));
                }
                if init.is_ok() {
                    CoUninitialize();
                }
            }
        }
    }

    /// The real jump list, end to end, on this PC's shell: `commit` with the real tasks under a
    /// throwaway app id (never tug's own, so the installed tug's list is untouched), pointing at
    /// this test exe and at an installed tug's per-user path, then the list is deleted again.
    /// Ignored because it writes to the signed-in user's shell:
    ///   cargo test --lib jump_list_probe -- --ignored --nocapture
    #[test]
    #[ignore = "writes a throwaway jump list to this user's shell"]
    fn jump_list_probe() {
        let id = format!("dev.davejames.tug.probe.{}", std::process::id());
        let _cleanup = DeleteOnDrop(id.clone());
        // What `install` passes: tauri's current_exe, canonicalized, so `\\?\C:\…` on Windows.
        let this_exe = tauri::utils::platform::current_exe().unwrap();
        let installed = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap()).join(r"tug\tug.exe");
        let verbatim_installed = std::path::PathBuf::from(format!(r"\\?\{}", installed.display()));
        for exe in [this_exe, installed, verbatim_installed] {
            for show_games in [true, false] {
                let tasks = super::super::tasks(show_games);
                match commit(&id, &exe, &tasks) {
                    Ok(()) => println!("jump list ok: {} tasks -> {}", tasks.len(), exe.display()),
                    Err(e) => panic!("jump list failed for {}: {e}", exe.display()),
                }
            }
        }
    }
}
