use super::*;
use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
use windows_sys::Win32::System::Registry::*;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "FortniteChestLedger";

pub fn enabled(storage: &Path) -> bool {
    storage.join("open-with-fortnite").exists() && unsafe { registered_for_current_exe() }
}

unsafe fn registered_for_current_exe() -> bool {
    let mut key: HKEY = null_mut();
    if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_QUERY_VALUE, &mut key) != 0 { return false; }
    let mut value = [0u16; 1024];
    let mut size = std::mem::size_of_val(&value) as u32;
    let status = RegQueryValueExW(key, wide(VALUE).as_ptr(), null_mut(), null_mut(), value.as_mut_ptr().cast(), &mut size);
    RegCloseKey(key);
    if status != 0 { return false; }
    let Ok(exe) = std::env::current_exe() else { return false; };
    let command = String::from_utf16_lossy(&value[..value.iter().position(|&unit| unit == 0).unwrap_or(value.len())]);
    let prefix = format!("\"{}\" --standby", exe.display());
    command.to_lowercase().starts_with(&prefix.to_lowercase())
}

pub unsafe fn remove_for_uninstall(storage: &Path, replays: &Path) -> Result<(), String> {
    if registered_for_current_exe() {
        configure(storage, replays, false)?;
        // The watcher checks its setting every four seconds before exiting.
        let name = wide("Local\\FortniteChestLedgerStandby");
        for _ in 0..50 {
            let running = OpenMutexW(MUTEX_MODIFY_STATE, 0, name.as_ptr());
            if running.is_null() { break; }
            CloseHandle(running);
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(())
}

pub unsafe fn configure(storage: &Path, replays: &Path, enable: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let command = format!("\"{}\" --standby --storage \"{}\" --replays \"{}\"",
        exe.display(), storage.display(), replays.display());
    let marker = storage.join("open-with-fortnite");
    if enable { fs::write(&marker, []).map_err(|e| e.to_string())?; }
    if let Err(error) = registration(RUN_KEY, enable.then_some(command.as_str())) {
        if enable { let _ = fs::remove_file(&marker); }
        return Err(error);
    }
    if enable {
        if let Err(error) = Command::new(exe).arg("--standby").arg("--storage").arg(storage)
            .arg("--replays").arg(replays).creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            let _ = registration(RUN_KEY, None);
            let _ = fs::remove_file(marker);
            return Err(error.to_string());
        }
    } else if marker.exists() { fs::remove_file(marker).map_err(|e| e.to_string())?; }
    Ok(())
}

unsafe fn registration(key_path: &str, command: Option<&str>) -> Result<(), String> {
    let mut key: HKEY = null_mut();
    let result = RegCreateKeyExW(HKEY_CURRENT_USER, wide(key_path).as_ptr(), 0, null(), 0,
        KEY_SET_VALUE, null(), &mut key, null_mut());
    if result != 0 { return Err(format!("Could not save the startup setting ({result}).")); }
    let status = if let Some(command) = command {
        let command = wide(command);
        RegSetValueExW(key, wide(VALUE).as_ptr(), 0, REG_SZ, command.as_ptr().cast(), (command.len() * 2) as u32)
    } else {
        RegDeleteValueW(key, wide(VALUE).as_ptr())
    };
    RegCloseKey(key);
    if status == 0 || (command.is_none() && status == ERROR_FILE_NOT_FOUND) { Ok(()) }
    else { Err(format!("Could not save the startup setting ({status}).")) }
}

unsafe fn running_process(target: &str) -> bool {
    let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if snapshot == INVALID_HANDLE_VALUE { return false; }
    let mut entry: PROCESSENTRY32W = zeroed();
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut more = Process32FirstW(snapshot, &mut entry) != 0;
    let mut found = false;
    while more {
        let length = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
        if String::from_utf16_lossy(&entry.szExeFile[..length]).eq_ignore_ascii_case(target) { found = true; break; }
        more = Process32NextW(snapshot, &mut entry) != 0;
    }
    CloseHandle(snapshot);
    found
}

pub unsafe fn standby(storage: PathBuf, replays: PathBuf) {
    let mutex = CreateMutexW(null(), 0, wide("Local\\FortniteChestLedgerStandby").as_ptr());
    if mutex.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
        if !mutex.is_null() { CloseHandle(mutex); }
        return;
    }
    let mut was_running = false;
    while enabled(&storage) {
        let running = running_process("FortniteClient-Win64-Shipping.exe");
        if running && !was_running {
            if let Ok(exe) = std::env::current_exe() {
                if Command::new(exe).arg("--auto").arg("--storage").arg(&storage)
                    .arg("--replays").arg(&replays).creation_flags(CREATE_NO_WINDOW)
                    .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_err() {
                    std::thread::sleep(Duration::from_secs(4));
                    continue;
                }
            }
        }
        was_running = running;
        std::thread::sleep(Duration::from_secs(4));
    }
    CloseHandle(mutex);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_own_running_executable() {
        let name = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().into_owned();
        unsafe {
            assert!(running_process(&name));
            assert!(!running_process("FortniteChestLedger-absent-test.exe"));
        }
    }

    #[test]
    fn writes_and_removes_startup_value_in_test_key() {
        unsafe {
            let key = "Software\\FortniteChestLedgerTests";
            registration(key, Some("\"C:\\example path\\TreasureLedger.exe\" --standby")).unwrap();
            registration(key, None).unwrap();
            assert_eq!(RegDeleteKeyW(HKEY_CURRENT_USER, wide(key).as_ptr()), 0);
        }
    }
}
