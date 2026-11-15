use super::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(15 * 60);

pub struct Updates {
    next_check: Instant,
    child: Option<Child>,
    pub ready: Option<String>,
    pub failed: bool,
}

impl Updates {
    pub fn new() -> Self {
        Self { next_check: Instant::now(), child: None, ready: None, failed: false }
    }

    pub fn tick(&mut self, storage: &Path) -> bool {
        let now = Instant::now();
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(None) => {},
                Ok(Some(exit)) => {
                    self.next_check = now + if exit.success() { CHECK_INTERVAL } else { RETRY_INTERVAL };
                    self.child = None;
                }
                Err(_) => { self.next_check = now + RETRY_INTERVAL; self.child = None; }
            }
        }

        if self.child.is_none() && now >= self.next_check {
            self.next_check = now + RETRY_INTERVAL;
            self.child = std::env::current_exe().and_then(|exe| decoder::command(storage)?.arg("--check-update")
                .arg(storage).arg(exe).arg(VERSION).spawn()).ok();
        }

        let staged = storage.join("updates");
        let next = fs::read_to_string(staged.join("ready.txt")).ok().and_then(|text| {
            let mut lines = text.lines();
            let target = lines.next()?;
            let tag = lines.next()?;
            let current = std::env::current_exe().ok()?;
            (current.to_string_lossy().eq_ignore_ascii_case(target) && tag != format!("v{VERSION}")).then(|| tag.to_owned())
        });
        let failed = next.is_some() && staged.join("last-error.txt").exists();
        if self.ready == next && self.failed == failed { return false; }
        self.ready = next;
        self.failed = failed;
        true
    }

    pub fn apply(&self, storage: &Path, replays: &Path, restart: bool) -> std::io::Result<()> {
        let exe = std::env::current_exe()?;
        decoder::command(storage)?.arg("--apply-update").arg(storage).arg(exe).arg(VERSION)
            .arg(std::process::id().to_string()).arg(replays)
            .arg(if restart { "restart" } else { "close" })
            .arg(if startup::enabled(storage) { "standby" } else { "no-standby" })
            .spawn()?;
        Ok(())
    }
}

pub unsafe fn in_progress() -> bool {
    let mutex = OpenMutexW(SYNCHRONIZE, 0, wide("Local\\FortniteChestLedgerUpdating").as_ptr());
    if mutex.is_null() { return false; }
    CloseHandle(mutex);
    true
}
