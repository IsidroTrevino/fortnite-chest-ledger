#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::mem::zeroed;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant, SystemTime};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Com::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED, ODS_FOCUS, SetWindowTheme};
mod startup;
mod decoder;
mod update;
mod rewards;

const INK: u32 = rgb(237, 241, 247);
const MUTED: u32 = rgb(143, 156, 176);
const PAPER: u32 = rgb(14, 19, 28);
const WHITE: u32 = rgb(23, 30, 42);
const NIGHT: u32 = rgb(18, 24, 35);
const GOLD: u32 = rgb(246, 192, 108);
const PALE: u32 = rgb(195, 205, 220);
const LINE: u32 = rgb(37, 47, 64);
const TEAL: u32 = rgb(105, 211, 185);
const OVERLAY_W: i32 = 248;
const OVERLAY_H: i32 = 55;
const ID_OVERLAY: usize = 101;
const ID_LIST: usize = 103;
const ID_SCAN: usize = 104;
const ID_STARTUP: usize = 105;
const ID_DATA: usize = 106;
const ID_UPDATE: usize = 107;
const ID_OVERVIEW: usize = 108;
const ID_REWARDS: usize = 109;
const ID_REWARD_LIST: usize = 110;
const ID_CHECK_UPDATE: usize = 111;

thread_local! { static STATE: RefCell<Option<App>> = const { RefCell::new(None) }; }

struct MatchRow {
    date: String,
    mode: String,
    opened: String,
    broken: String,
    squad: String,
    detail: String,
}

#[derive(Default)]
struct Snapshot {
    player: String,
    total: String,
    average: String,
    matches: String,
    last: String,
    opened: String,
    broken: String,
    status: String,
    combined: String,
    solo_opens: String,
    squad_opens: String,
    token_chance: String,
    one_in: String,
    reward_coverage: String,
    rewards: Vec<rewards::Card>,
    rows: Vec<MatchRow>,
}

impl Snapshot {
    fn parse(text: &str) -> Self {
        let mut snapshot = Self::default();
        for line in text.lines() {
            let parts: Vec<_> = line.split('\t').collect();
            match parts.as_slice() {
                ["summary", player, total, average, matches, last, opened, broken] => {
                    snapshot.player = (*player).into();
                    snapshot.total = (*total).into();
                    snapshot.average = (*average).into();
                    snapshot.matches = (*matches).into();
                    snapshot.last = (*last).into();
                    snapshot.opened = (*opened).into();
                    snapshot.broken = (*broken).into();
                }
                ["state", status] => snapshot.status = (*status).into(),
                ["odds", combined, solo, squad, chance, one_in] => {
                    snapshot.combined = (*combined).into();
                    snapshot.solo_opens = (*solo).into();
                    snapshot.squad_opens = (*squad).into();
                    snapshot.token_chance = (*chance).into();
                    snapshot.one_in = (*one_in).into();
                }
                ["reward-coverage", count] => snapshot.reward_coverage = (*count).into(),
                ["reward", id, name, category, group, count, latest] => {
                    snapshot.rewards.push(rewards::Card {
                        id: (*id).into(), name: (*name).into(), category: (*category).into(),
                        group: (*group).into(), count: count.parse().unwrap_or(0), latest: (*latest).into(),
                    });
                }
                ["match", date, mode, opened, broken, squad, detail] => {
                    snapshot.rows.push(MatchRow {
                        date: (*date).into(), mode: (*mode).into(), opened: (*opened).into(),
                        broken: (*broken).into(), squad: (*squad).into(),
                        detail: detail.replace('\u{2028}', "\r\n"),
                    });
                }
                _ => {}
            }
        }
        snapshot
    }
}

struct Watch {
    path: PathBuf,
    handle: HANDLE,
    exists: bool,
}

impl Watch {
    fn new(path: PathBuf) -> Self {
        let exists = path.is_dir();
        let handle = if exists {
            unsafe { FindFirstChangeNotificationW(wide(&path.to_string_lossy()).as_ptr(), 0,
                FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_LAST_WRITE) }
        } else { INVALID_HANDLE_VALUE };
        Self { path, handle, exists }
    }

    fn changed(&mut self) -> bool {
        let exists = self.path.is_dir();
        if exists != self.exists {
            *self = Self::new(self.path.clone());
            return true;
        }
        if self.handle == INVALID_HANDLE_VALUE { return false; }
        unsafe {
            if WaitForSingleObject(self.handle, 0) != WAIT_OBJECT_0 { return false; }
            if FindNextChangeNotification(self.handle) == 0 {
                *self = Self::new(self.path.clone());
            }
        }
        true
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE { unsafe { FindCloseChangeNotification(self.handle); } }
    }
}

struct App {
    window: HWND,
    overlay: HWND,
    overlay_button: HWND,
    scan_button: HWND,
    startup_button: HWND,
    data_button: HWND,
    update_button: HWND,
    check_button: HWND,
    overview_button: HWND,
    rewards_button: HWND,
    updates: update::Updates,
    list: HWND,
    reward_list: HWND,
    show_rewards: bool,
    storage: PathBuf,
    replay_watch: Watch,
    child: Option<Child>,
    due: Option<Instant>,
    periodic: Instant,
    status: String,
    overlay_on: bool,
    overlay_demo: bool,
    foreground: HWND,
    fortnite: bool,
    overlay_location: Option<(i32, i32)>,
    snapshot_time: Option<SystemTime>,
    snapshot: Snapshot,
    list_font: HFONT,
    button_font: HFONT,
}

impl App {
    unsafe fn open(window: HWND, overlay: HWND, storage: PathBuf, replays: PathBuf, overlay_demo: bool) -> Self {
        let _ = fs::create_dir_all(&storage);
        let overlay_on = !storage.join("overlay-off").exists();
        let overlay_button = control(window, "BUTTON", "", ID_OVERLAY, WS_TABSTOP);
        let scan_button = control(window, "BUTTON", "Scan now", ID_SCAN, WS_TABSTOP);
        let startup_button = control(window, "BUTTON", "", ID_STARTUP, WS_TABSTOP);
        let data_button = control(window, "BUTTON", "Open history folder", ID_DATA, WS_TABSTOP);
        let update_button = control(window, "BUTTON", "Restart to update", ID_UPDATE, WS_TABSTOP);
        let check_button = control(window, "BUTTON", "Check for updates", ID_CHECK_UPDATE, WS_TABSTOP);
        let overview_button = control(window, "BUTTON", "Overview", ID_OVERVIEW, WS_TABSTOP);
        let rewards_button = control(window, "BUTTON", "Treasure rewards", ID_REWARDS, WS_TABSTOP);
        ShowWindow(update_button, SW_HIDE);
        let list_style = WS_TABSTOP | WS_VSCROLL | LBS_NOTIFY as u32 | LBS_NOINTEGRALHEIGHT as u32 |
            LBS_OWNERDRAWFIXED as u32 | LBS_HASSTRINGS as u32;
        let list = control(window, "LISTBOX", "", ID_LIST, list_style);
        let reward_list = control(window, "LISTBOX", "", ID_REWARD_LIST, list_style);
        ShowWindow(reward_list, SW_HIDE);
        SendMessageW(list, LB_SETITEMHEIGHT, 0, 44);
        SendMessageW(reward_list, LB_SETITEMHEIGHT, 0, rewards::ROW_HEIGHT as isize);
        let list_font = font("Segoe UI", 15, 400);
        let button_font = font("Segoe UI", 15, 400);
        for listbox in [list, reward_list] {
            SetWindowTheme(listbox, wide("DarkMode_Explorer").as_ptr(), null());
            SendMessageW(listbox, WM_SETFONT, list_font as usize, 1);
        }
        for button in [overlay_button, scan_button, startup_button, data_button, update_button, check_button,
            overview_button, rewards_button] {
            SendMessageW(button, WM_SETFONT, button_font as usize, 1);
        }
        let mut app = Self {
            window, overlay, overlay_button, scan_button, startup_button, data_button, update_button,
            check_button, overview_button, rewards_button, updates: update::Updates::new(), list, reward_list,
            show_rewards: false, storage,
            replay_watch: Watch::new(replays), child: None,
            due: Some(Instant::now()), periodic: Instant::now() + Duration::from_secs(300),
            status: "Starting replay scan".into(), overlay_on, overlay_demo, foreground: null_mut(), fortnite: false,
            overlay_location: None, snapshot_time: None, snapshot: Snapshot::default(), list_font, button_font,
        };
        app.label_overlay();
        app.label_startup();
        app.refresh_snapshot();
        app.layout();
        app
    }

    unsafe fn refresh_snapshot(&mut self) {
        let path = self.storage.join("snapshot.tsv");
        let time = fs::metadata(&path).and_then(|meta| meta.modified()).ok();
        if time.is_none() || self.snapshot_time == time { return; }
        let Ok(text) = fs::read_to_string(path) else { return; };
        let selected = SendMessageW(self.list, LB_GETCURSEL, 0, 0).max(0) as usize;
        let top = SendMessageW(self.list, LB_GETTOPINDEX, 0, 0).max(0) as usize;
        let previous = self.snapshot.rows.get(selected).map(|row| (row.date.clone(), row.mode.clone()));
        self.snapshot_time = time;
        self.snapshot = Snapshot::parse(&text);
        SendMessageW(self.list, WM_SETREDRAW, 0, 0);
        SendMessageW(self.list, LB_RESETCONTENT, 0, 0);
        for row in &self.snapshot.rows {
            let s = format!("{} · {} · {} opened · {} broken · {} squad seen", row.date, row.mode, row.opened, row.broken, row.squad);
            SendMessageW(self.list, LB_ADDSTRING, 0, wide(&s).as_ptr() as isize);
        }
        if !self.snapshot.rows.is_empty() {
            let next = previous.and_then(|(date, mode)| self.snapshot.rows.iter().position(|r| r.date == date && r.mode == mode)).unwrap_or(0);
            SendMessageW(self.list, LB_SETCURSEL, next, 0);
            SendMessageW(self.list, LB_SETTOPINDEX, top.min(self.snapshot.rows.len() - 1), 0);
        }
        SendMessageW(self.list, WM_SETREDRAW, 1, 0);
        rewards::refresh(self.reward_list, &self.snapshot.rewards);
        self.update_view();
        InvalidateRect(self.list, null(), 1);
        InvalidateRect(self.window, null(), 0);
        InvalidateRect(self.overlay, null(), 0);
    }

    unsafe fn update_view(&self) {
        for (control, visible) in [(self.list, !self.show_rewards), (self.scan_button, !self.show_rewards),
            (self.reward_list, self.show_rewards)] {
            ShowWindow(control, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
        for control in [self.window, self.overview_button, self.rewards_button] {
            InvalidateRect(control, null(), 0);
        }
    }

    unsafe fn switch_view(&mut self, show_rewards: bool) {
        self.show_rewards = show_rewards;
        self.update_view();
    }

    unsafe fn tick(&mut self) {
        if self.updates.tick(&self.storage) {
            ShowWindow(self.update_button, if self.updates.ready.is_some() { SW_SHOWNOACTIVATE } else { SW_HIDE });
            InvalidateRect(self.window, null(), 0);
        }
        let now = Instant::now();
        let replay_change = self.replay_watch.changed();
        if replay_change { self.due = Some(now + Duration::from_secs(8)); }
        if now >= self.periodic && self.due.is_none() { self.due = Some(now); }

        let mut finished_error = None;
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(exit)) => {
                    if !exit.success() && exit.code() != Some(3) { finished_error = Some(format!("Decoder failed ({exit}). Open history folder for details.")); }
                    self.child = None;
                }
                Ok(None) => {}
                Err(error) => {
                    finished_error = Some(format!("Could not monitor decoder: {error}"));
                    self.child = None;
                }
            }
        }

        if self.child.is_none() && self.due.is_some_and(|due| now >= due) {
            self.due = None;
            self.periodic = now + Duration::from_secs(300);
            match decoder::start(&self.storage, &self.replay_watch.path) {
                Ok(child) => { self.child = Some(child); self.status.clear(); }
                Err(error) => finished_error = Some(format!("Could not start TreasureDecoder.exe: {error}")),
            }
        }

        self.refresh_snapshot();
        let status = if let Some(error) = finished_error { error }
        else if self.child.is_some() {
            fs::read_to_string(self.storage.join("progress.txt")).ok().filter(|text| !text.is_empty())
                .unwrap_or_else(|| "Checking completed replays".into())
        } else if self.status.starts_with("Could not") || self.status.starts_with("Decoder failed") {
            self.status.clone()
        } else if self.snapshot.status.is_empty() {
            "Waiting for a finished Fortnite replay".into()
        } else { self.snapshot.status.clone() };
        if status != self.status {
            self.status = status;
            InvalidateRect(self.window, null(), 0);
        }
        self.place_overlay();
    }

    unsafe fn place_overlay(&mut self) {
        let foreground = GetForegroundWindow();
        if foreground != self.foreground {
            self.foreground = foreground;
            self.fortnite = is_fortnite(foreground);
        }
        if !self.overlay_on || (!self.fortnite && !self.overlay_demo) || self.snapshot.rows.is_empty() {
            if IsWindowVisible(self.overlay) != 0 { ShowWindow(self.overlay, SW_HIDE); }
            self.overlay_location = None;
            return;
        }
        let target = if self.overlay_demo { self.window } else { foreground };
        let mut frame: RECT = zeroed();
        if GetClientRect(target, &mut frame) == 0 { return; }
        let mut point = POINT { x: 25, y: (frame.bottom - 147).max(8) };
        if ClientToScreen(target, &mut point) == 0 { return; }
        let next = Some((point.x, point.y));
        if self.overlay_location != next {
            self.overlay_location = next;
            SetWindowPos(self.overlay, HWND_TOPMOST, point.x, point.y, OVERLAY_W, OVERLAY_H, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }

    unsafe fn label_overlay(&self) {
        let label = if self.overlay_on { "Overlay on" } else { "Overlay off" };
        SetWindowTextW(self.overlay_button, wide(label).as_ptr());
    }

    unsafe fn label_startup(&self) {
        let value = if startup::enabled(&self.storage) { "Open with Fortnite  ON" } else { "Open with Fortnite  OFF" };
        SetWindowTextW(self.startup_button, wide(value).as_ptr());
        InvalidateRect(self.startup_button, null(), 0);
    }

    unsafe fn layout(&self) {
        let mut area: RECT = zeroed();
        GetClientRect(self.window, &mut area);
        let width = area.right;
        let height = area.bottom;
        MoveWindow(self.overlay_button, 18, 233, 174, 40, 1);
        MoveWindow(self.startup_button, 18, 283, 174, 44, 1);
        MoveWindow(self.overview_button, 18, 105, 174, 38, 1);
        MoveWindow(self.rewards_button, 18, 149, 174, 38, 1);
        MoveWindow(self.check_button, 18, 350, 174, 38, 1);
        MoveWindow(self.update_button, 18, 422, 174, 38, 1);
        MoveWindow(self.data_button, 18, height - 100, 174, 38, 1);
        MoveWindow(self.scan_button, width - 136, 37, 104, 36, 1);
        MoveWindow(self.list, 240, 442, (width - 272).max(1), (height - 633).max(44), 1);
        MoveWindow(self.reward_list, 240, 143, (width - 272).max(1), (height - 198).max(1), 1);
        InvalidateRect(self.window, null(), 0);
    }

    unsafe fn draw(&self, dc: HDC, area: RECT) {
        let width = area.right;
        let height = area.bottom;
        let x = 240;
        let available = width - x - 32;
        fill(dc, area, PAPER);
        fill(dc, rect(0, 0, 210, height), NIGHT);
        fill(dc, rect(209, 0, 1, height), LINE);
        DrawIconEx(dc, 23, 26, app_icon(), 40, 40, 0, null_mut(), DI_NORMAL);
        txt(dc, "TREASURE", 72, 28, 120, 21, 16, 700, INK);
        txt(dc, "CHEST LEDGER", 72, 49, 125, 18, 11, 600, MUTED);
        txt(dc, "TRACKING", 28, 202, 150, 20, 11, 700, MUTED);
        let update_status = match &self.updates.ready {
            Some(tag) if self.updates.failed => format!("{tag} could not install"),
            Some(tag) => format!("{tag} ready to install"),
            None => self.updates.status.into(),
        };
        txt(dc, &update_status, 28, 396, 158, 20, 12, 500, MUTED);
        txt(dc, "Your counts are saved", 27, height - 155, 167, 19, 13, 600, PALE);
        txt(dc, "after every match.", 27, height - 136, 167, 19, 13, 400, MUTED);
        txt(dc, &format!("VERSION {}", update::VERSION), 27, height - 38, 160, 18, 10, 700, MUTED);
        fill(dc, rect(x, height - 38, 5, 5), if self.child.is_some() { GOLD } else { TEAL });
        txt(dc, &self.status, x + 14, height - 45, available - 14, 23, 12, 400, MUTED);

        if self.show_rewards {
            rewards::heading(dc, x, available, &self.snapshot.rewards, &self.snapshot.reward_coverage);
            return;
        }

        txt(dc, "Your treasure hunt", x, 31, 530, 41, 32, 700, INK);
        let subtitle = if self.snapshot.player.is_empty() { "Waiting for your first completed match".into() }
            else { format!("{}  /  Automatically updated after each match", self.snapshot.player) };
        txt(dc, &subtitle, x, 80, available, 22, 14, 400, MUTED);
        let gap = 12;
        let card = (available - 3 * gap) / 4;
        let metrics = [("Your chests", &self.snapshot.total, "Opened + broken"),
                       ("Average / match", &self.snapshot.average, "Your recorded matches"),
                       ("Last match", &self.snapshot.last, "Your latest chest count"),
                       ("Matches saved", &self.snapshot.matches, "Kept after replay deletion")];
        for (index, (label, value, caption)) in metrics.iter().enumerate() {
            let left = x + index as i32 * (card + gap);
            rounded(dc, rect(left, 124, card, 115), if index == 0 { rgb(43, 35, 27) } else { WHITE }, 14);
            txt(dc, label, left + 16, 138, card - 24, 21, 14, 500, if index == 0 { GOLD } else { PALE });
            txt(dc, display(value), left + 16, 164, card - 24, 40, 35, 700, if index == 0 { GOLD } else { INK });
            txt(dc, caption, left + 16, 211, card - 24, 18, 11, 400, MUTED);
        }
        let activity_width = available * 36 / 100;
        rounded(dc, rect(x, 255, activity_width, 116), WHITE, 12);
        txt(dc, "Recent matches", x + 16, 268, activity_width - 32, 21, 14, 600, PALE);
        let values: Vec<f64> = self.snapshot.rows.iter().take(20).rev().map(|r|
            r.opened.replace(',', "").parse::<f64>().unwrap_or(0.0) + r.broken.replace(',', "").parse::<f64>().unwrap_or(0.0)).collect();
        let maximum = values.iter().copied().fold(1.0, f64::max);
        let chart_width = activity_width - 32;
        let bar_width = (chart_width / 20 - 5).max(3);
        for (index, value) in values.iter().enumerate() {
            let bar_height = (value / maximum * 47.0).round().max(2.0) as i32;
            rounded(dc, rect(x + 16 + index as i32 * (chart_width / 20), 352 - bar_height, bar_width, bar_height),
                if index + 1 == values.len() { GOLD } else { rgb(68, 101, 111) }, 4);
        }
        let sx = x + activity_width + 12;
        let sw = (available - activity_width - 24) / 2;
        let solo = display(&self.snapshot.solo_opens);
        let squad = display(&self.snapshot.squad_opens);
        let breakdown = format!("{solo} solo  /  {squad} nearby squad");
        let baseline = format!("Normal odds · 1 in {}", display(&self.snapshot.one_in));
        for (left, label, value, caption, color) in [
            (sx, "Combined chests opened", &self.snapshot.combined, &breakdown, TEAL),
            (sx + sw + 12, "Pirate Shop token chance", &self.snapshot.token_chance, &baseline, GOLD),
        ] {
            rounded(dc, rect(left, 255, sw, 116), WHITE, 12);
            txt(dc, label, left + 16, 268, sw - 30, 21, 14, 600, PALE);
            txt(dc, display(value), left + 16, 294, sw - 30, 38, 30, 700, color);
            txt(dc, caption, left + 16, 337, sw - 30, 18, 12, 500, MUTED);
        }
        txt(dc, "Match history", x, 389, 220, 25, 20, 600, INK);
        txt(dc, &format!("{} opened   /   {} broken", display(&self.snapshot.opened), display(&self.snapshot.broken)),
            width - 310, 393, 278, 21, 13, 400, MUTED);
        let cols = columns(available);
        for (label, offset, length) in [("MATCH", cols[0], 120), ("MODE", cols[1], 140),
            ("OPENED", cols[2], 68), ("BROKEN", cols[3], 68), ("SQUAD SEEN", cols[4], 108)] {
            txt(dc, label, x + offset, 421, length, 18, 10, 600, MUTED);
        }
        let bottom = height - 178;
        rounded(dc, rect(x, bottom, available, 120), WHITE, 12);
        let selected = SendMessageW(self.list, LB_GETCURSEL, 0, 0);
        if let Some(row) = usize::try_from(selected).ok().and_then(|i| self.snapshot.rows.get(i)) {
            txt(dc, "Squad breakdown", x + 16, bottom + 10, 240, 21, 14, 600, INK);
            txt(dc, "Nearby events from your replay", x + available - 230, bottom + 12, 214, 19, 12, 500, TEAL);
            for (i, line) in row.detail.lines().take(5).enumerate() {
                txt(dc, line, x + 16, bottom + 36 + i as i32 * 15, available - 32, 18, 12, 400, MUTED);
            }
        } else {
            txt(dc, "Completed matches will appear here automatically.", x + 16, bottom + 24, available - 32, 25, 15, 400, MUTED);
        }
    }

    unsafe fn draw_item(&self, item: &DRAWITEMSTRUCT) {
        let dc = item.hDC;
        let r = item.rcItem;
        let id = item.CtlID as usize;
        if id == ID_REWARD_LIST {
            rewards::draw_row(item, &self.snapshot.rewards);
        } else if id == ID_LIST {
            fill(dc, r, PAPER);
            let Some(row) = self.snapshot.rows.get(item.itemID as usize) else { return; };
            let selected = item.itemState & ODS_SELECTED != 0;
            if selected { rounded(dc, rect(r.left, r.top + 2, r.right - r.left - 3, 39), rgb(35, 47, 64), 8); }
            else if item.itemID % 2 == 0 { rounded(dc, rect(r.left, r.top + 2, r.right - r.left - 3, 39), NIGHT, 8); }
            let mut area: RECT = zeroed();
            GetClientRect(self.window, &mut area);
            let cols = columns(area.right - 272);
            for (value, offset, w, color) in [
                (row.date.as_str(), cols[0], 122, INK), (row.mode.as_str(), cols[1], 140, MUTED),
                (row.opened.as_str(), cols[2], 65, INK), (row.broken.as_str(), cols[3], 65, MUTED),
                (row.squad.as_str(), cols[4], 65, INK)] {
                txt(dc, value, r.left + offset, r.top + 13, w, 21, 13, 400, color);
            }
            if item.itemState & ODS_FOCUS != 0 {
                let focus = rect(r.left + 2, r.top + 3, r.right - r.left - 6, 37);
                DrawFocusRect(dc, &focus);
            }
        } else {
            let mut label = [0u16; 128];
            let n = GetWindowTextW(item.hwndItem, label.as_mut_ptr(), 128);
            let label = String::from_utf16_lossy(&label[..n.max(0) as usize]);
            let nav_active = (id == ID_OVERVIEW && !self.show_rewards) || (id == ID_REWARDS && self.show_rewards);
            let active = (id == ID_OVERLAY && self.overlay_on) || (id == ID_STARTUP && startup::enabled(&self.storage));
            let pressed = item.itemState & ODS_SELECTED != 0;
            fill(dc, r, if item.CtlID as usize == ID_SCAN { PAPER } else { NIGHT });
            rounded(dc, rect(r.left, r.top, r.right - r.left, r.bottom - r.top),
                if pressed { rgb(64, 67, 69) } else if nav_active { rgb(45, 39, 32) }
                else if active { rgb(38, 48, 53) } else { WHITE }, 9);
            txt(dc, &label, r.left + 12, r.top + (r.bottom - r.top - 19) / 2, r.right - r.left - 19, 23,
                if id == ID_STARTUP { 12 } else { 13 }, 500, if nav_active { GOLD } else if active { TEAL } else { PALE });
            if item.itemState & ODS_FOCUS != 0 {
                let focus = rect(r.left + 3, r.top + 3, r.right - r.left - 6, r.bottom - r.top - 6);
                DrawFocusRect(dc, &focus);
            }
        }
    }

}

unsafe fn is_fortnite(hwnd: HWND) -> bool {
    if hwnd.is_null() { return false; }
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if handle.is_null() { return false; }
    let mut image = [0_u16; 2048];
    let mut len = image.len() as u32;
    let found = QueryFullProcessImageNameW(handle, 0, image.as_mut_ptr(), &mut len) != 0;
    CloseHandle(handle);
    found && Path::new(&String::from_utf16_lossy(&image[..len as usize])).file_name()
        .and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("FortniteClient-Win64-Shipping.exe"))
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_MEASUREITEM => {
            let item = &mut *(lp as *mut MEASUREITEMSTRUCT);
            item.itemHeight = if item.CtlID as usize == ID_REWARD_LIST { rewards::ROW_HEIGHT } else { 44 };
            1
        }
        WM_DRAWITEM => { with_app(|app| app.draw_item(&*(lp as *const DRAWITEMSTRUCT))); 1 }
        WM_CTLCOLORLISTBOX => {
            SetBkColor(wp as HDC, PAPER);
            SetTextColor(wp as HDC, INK);
            SetDCBrushColor(wp as HDC, PAPER);
            GetStockObject(DC_BRUSH) as isize
        }
        WM_GETMINMAXINFO => {
            let data = &mut *(lp as *mut MINMAXINFO);
            data.ptMinTrackSize = POINT { x: 1080, y: 780 };
            0
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut area: RECT = zeroed();
            GetClientRect(hwnd, &mut area);
            let buffer = CreateCompatibleDC(dc);
            let bitmap = CreateCompatibleBitmap(dc, area.right.max(1), area.bottom.max(1));
            let previous = SelectObject(buffer, bitmap);
            with_app(|app| app.draw(buffer, area));
            BitBlt(dc, 0, 0, area.right, area.bottom, buffer, 0, 0, SRCCOPY);
            SelectObject(buffer, previous);
            DeleteObject(bitmap);
            DeleteDC(buffer);
            EndPaint(hwnd, &ps);
            0
        }
        WM_SIZE => { with_app(|app| app.layout()); 0 }
        WM_TIMER => { with_app(|app| app.tick()); 0 }
        WM_CLOSE => {
            with_app(|app| {
                if app.updates.ready.is_some() { let _ = app.updates.apply(&app.storage, &app.replay_watch.path, false); }
            });
            DestroyWindow(hwnd);
            0
        }
        WM_COMMAND => {
            match wp & 0xffff {
                ID_OVERLAY => with_app(|app| {
                    app.overlay_on = !app.overlay_on;
                    let marker = app.storage.join("overlay-off");
                    if app.overlay_on { let _ = fs::remove_file(marker); }
                    else { let _ = fs::write(marker, []); }
                    app.label_overlay();
                    app.place_overlay();
                }),
                ID_SCAN => with_app(|app| { app.due = Some(Instant::now()); }),
                ID_OVERVIEW => with_app(|app| { app.switch_view(false); }),
                ID_REWARDS => with_app(|app| { app.switch_view(true); }),
                ID_CHECK_UPDATE => with_app(|app| {
                    app.updates.request_check();
                    app.tick();
                }),
                ID_STARTUP => {
                    let mut failure = None;
                    with_app(|app| {
                        let enable = !startup::enabled(&app.storage);
                        failure = startup::configure(&app.storage, &app.replay_watch.path, enable).err();
                        app.label_startup();
                    });
                    if let Some(error) = failure { MessageBoxW(hwnd, wide(&error).as_ptr(), wide("Startup setting").as_ptr(), MB_OK | MB_ICONERROR); }
                }
                ID_DATA => with_app(|app| {
                    ShellExecuteW(hwnd, wide("open").as_ptr(), wide(&app.storage.to_string_lossy()).as_ptr(), null(), null(), SW_SHOWNORMAL);
                }),
                ID_UPDATE => {
                    let mut restart = false;
                    with_app(|app| {
                        restart = app.updates.ready.is_some() && app.updates.apply(&app.storage, &app.replay_watch.path, true).is_ok();
                    });
                    if restart { DestroyWindow(hwnd); }
                    else { MessageBoxW(hwnd, wide("The update could not start. Try again shortly.").as_ptr(), wide("Fortnite Chest Ledger").as_ptr(), MB_OK | MB_ICONERROR); }
                }
                ID_LIST => { InvalidateRect(hwnd, null(), 0); }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, 1);
            with_app(|app| {
                DestroyWindow(app.overlay);
                DeleteObject(app.list_font);
                DeleteObject(app.button_font);
            });
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCHITTEST { return HTTRANSPARENT as isize; }
    if msg != WM_PAINT { return DefWindowProcW(hwnd, msg, wp, lp); }
    let mut ps: PAINTSTRUCT = zeroed();
    let dc = BeginPaint(hwnd, &mut ps);
    fill(dc, rect(0, 0, OVERLAY_W, OVERLAY_H), NIGHT);
    fill(dc, rect(0, 0, 3, OVERLAY_H), GOLD);
    with_app(|app| {
        txt(dc, &format!("{} chests", display(&app.snapshot.total)), 13, 4, 224, 27, 23, 700, GOLD);
        txt(dc, &format!("{} avg / match  ·  post-match", display(&app.snapshot.average)), 14, 33, 228, 18, 13, 400, PALE);
    });
    EndPaint(hwnd, &ps);
    0
}

fn with_app(action: impl FnOnce(&mut App)) {
    STATE.with(|state| {
        if let Ok(mut state) = state.try_borrow_mut() {
            if let Some(app) = state.as_mut() { action(app); }
        }
    });
}

fn display(value: &str) -> &str { if value.is_empty() { "—" } else { value } }
fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain([0]).collect() }
const fn rgb(r: u32, g: u32, b: u32) -> u32 { r | (g << 8) | (b << 16) }
fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT { RECT { left: x, top: y, right: x + w, bottom: y + h } }

fn columns(width: i32) -> [i32; 5] { [12, 144, width - 320, width - 226, width - 131] }

unsafe fn rounded(dc: HDC, r: RECT, color: u32, radius: i32) {
    let brush = CreateSolidBrush(color);
    let pen = CreatePen(PS_SOLID, 1, color);
    let old_brush = SelectObject(dc, brush);
    let old_pen = SelectObject(dc, pen);
    RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
    SelectObject(dc, old_brush);
    SelectObject(dc, old_pen);
    DeleteObject(brush);
    DeleteObject(pen);
}

unsafe fn fill(dc: HDC, region: RECT, color: u32) {
    let brush = CreateSolidBrush(color);
    FillRect(dc, &region, brush);
    DeleteObject(brush);
}

unsafe fn font(name: &str, size: i32, weight: i32) -> HFONT {
    CreateFontW(-size, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 5, 0, wide(name).as_ptr())
}

unsafe fn txt(dc: HDC, value: &str, x: i32, y: i32, w: i32, h: i32, size: i32, weight: i32, color: u32) {
    text(dc, value, rect(x, y, w, h), "Segoe UI", size, weight, color);
}

unsafe fn text(dc: HDC, value: &str, mut region: RECT, face: &str, size: i32, weight: i32, color: u32) {
    let face = font(face, size, weight);
    let old = SelectObject(dc, face);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, color);
    DrawTextW(dc, wide(value).as_mut_ptr(), -1, &mut region, DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS);
    SelectObject(dc, old);
    DeleteObject(face);
}

unsafe fn control(parent: HWND, class: &str, label: &str, id: usize, style: u32) -> HWND {
    let owner_draw = if class == "BUTTON" { BS_OWNERDRAW as u32 } else { 0 };
    CreateWindowExW(0, wide(class).as_ptr(), wide(label).as_ptr(), WS_CHILD | WS_VISIBLE | style | owner_draw,
        0, 0, 0, 0, parent, id as HMENU, GetModuleHandleW(null()), null())
}

unsafe fn register(name: &[u16], procedure: WNDPROC) {
    let mut cls: WNDCLASSW = zeroed();
    cls.lpfnWndProc = procedure;
    cls.hInstance = GetModuleHandleW(null());
    cls.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
    cls.hIcon = app_icon();
    cls.lpszClassName = name.as_ptr();
    RegisterClassW(&cls);
}

unsafe fn app_icon() -> HICON {
    let icon = LoadIconW(GetModuleHandleW(null()), 101usize as *const u16);
    if icon.is_null() { LoadIconW(null_mut(), IDI_APPLICATION) } else { icon }
}

fn main() {
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| OsString::from(".")));
    let mut storage = local.join("FortniteChestLedger");
    let mut replays = local.join("FortniteGame/Saved/Demos");
    let mut overlay_demo = false;
    let mut standby = false;
    let mut auto = false;
    let mut disable_startup = false;
    let mut resumed = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--storage") => { if let Some(value) = args.next() { storage = value.into(); } }
            Some("--replays") => { if let Some(value) = args.next() { replays = value.into(); } }
            Some("--overlay-preview") => overlay_demo = true,
            Some("--standby") => standby = true,
            Some("--auto") => auto = true,
            Some("--disable-startup") => disable_startup = true,
            Some("--resume-standby") => resumed = true,
            _ => {}
        }
    }
    unsafe {
        if disable_startup {
            if startup::remove_for_uninstall(&storage, &replays).is_err() { std::process::exit(1); }
            return;
        }
        if update::in_progress() { return; }
        if standby { startup::standby(storage, replays, resumed); return; }
        let main_class = wide("TreasureLedgerMain");
        let overlay_class = wide("TreasureLedgerOverlay");
        let mutex = CreateMutexW(null(), 0, wide("Local\\FortniteChestLedgerWindow").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let existing = FindWindowW(main_class.as_ptr(), null());
            if !auto && !existing.is_null() { ShowWindow(existing, SW_RESTORE); SetForegroundWindow(existing); }
            CloseHandle(mutex);
            return;
        }
        let com = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
        register(&main_class, Some(main_proc));
        register(&overlay_class, Some(overlay_proc));
        let instance = GetModuleHandleW(null());
        let mut desktop: RECT = zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut desktop as *mut RECT).cast(), 0);
        let width = 1180.min((desktop.right - desktop.left - 24).max(1080));
        let height = 850.min((desktop.bottom - desktop.top - 24).max(740));
        let x = desktop.left + ((desktop.right - desktop.left - width) / 2).max(0);
        let y = desktop.top + ((desktop.bottom - desktop.top - height) / 2).max(0);
        let main = CreateWindowExW(0, main_class.as_ptr(), wide("Fortnite Chest Ledger").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN, x, y, width, height, null_mut(), null_mut(), instance, null());
        let overlay = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOPMOST,
            overlay_class.as_ptr(), wide("Chest Ledger Overlay").as_ptr(), WS_POPUP,
            0, 0, OVERLAY_W, OVERLAY_H, null_mut(), null_mut(), instance, null());
        if !main.is_null() && !overlay.is_null() {
            let dark: i32 = 1;
            DwmSetWindowAttribute(main, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, (&dark as *const i32).cast(), 4);
            SetLayeredWindowAttributes(overlay, 0, 239, LWA_ALPHA);
            let app = App::open(main, overlay, storage, replays, overlay_demo);
            STATE.with(|state| *state.borrow_mut() = Some(app));
            ShowWindow(main, if auto { SW_SHOWMINNOACTIVE } else { SW_SHOW });
            SetTimer(main, 1, 800, None);
            let mut message: MSG = zeroed();
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            STATE.with(|state| *state.borrow_mut() = None);
        }
        if com >= 0 { CoUninitialize(); }
        CloseHandle(mutex);
    }
}
