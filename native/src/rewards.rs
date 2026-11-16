use super::*;

pub const ROW_HEIGHT: u32 = 118;
const COLUMNS: usize = 3;

pub struct Card {
    pub id: String,
    pub name: String,
    pub category: String,
    pub group: String,
    pub count: u32,
    pub latest: String,
}

impl Card {
    fn known(&self) -> bool {
        !self.pending() && self.id != "unidentified" && self.category != "Recorded prize ID"
    }

    fn pending(&self) -> bool {
        self.name.contains("(TBA)") || self.name.starts_with("Week 4 ") || self.name.starts_with("Week 5 ")
    }

    fn status(&self) -> String {
        match self.count {
            0 if self.pending() => "To be announced".into(),
            0 => "Not seen".into(),
            n if !self.known() => format!("{n} recorded"),
            1 => "Found".into(),
            n => format!("Found ×{n}"),
        }
    }
}

pub unsafe fn refresh(list: HWND, cards: &[Card]) {
    let top = SendMessageW(list, LB_GETTOPINDEX, 0, 0).max(0) as usize;
    let selected = SendMessageW(list, LB_GETCURSEL, 0, 0);
    SendMessageW(list, WM_SETREDRAW, 0, 0);
    SendMessageW(list, LB_RESETCONTENT, 0, 0);
    for row in cards.chunks(COLUMNS) {
        let description = row.iter().map(|card| format!("{} ({})", card.name, card.status()))
            .collect::<Vec<_>>().join("; ");
        SendMessageW(list, LB_ADDSTRING, 0, wide(&description).as_ptr() as isize);
    }
    let rows = cards.len().div_ceil(COLUMNS);
    if rows != 0 {
        SendMessageW(list, LB_SETCURSEL, selected as usize, 0);
        SendMessageW(list, LB_SETTOPINDEX, top.min(rows - 1), 0);
    }
    SendMessageW(list, WM_SETREDRAW, 1, 0);
    InvalidateRect(list, null(), 1);
}

pub unsafe fn heading(dc: HDC, x: i32, w: i32, cards: &[Card], unavailable: &str) {
    txt(dc, "Treasure Hunt rewards", x, 31, w, 41, 32, 700, INK);
    let found = cards.iter().filter(|card| card.known() && card.count > 0).count();
    let total = cards.iter().filter(|card| card.known()).count();
    let progress = if cards.is_empty() { "Waiting for a replay scan".into() }
        else { format!("{found} of {total} known rewards found") };
    txt(dc, &progress, x, 81, w, 22, 16, 600, TEAL);
    let detail = if unavailable.is_empty() || unavailable == "0" {
        "Automatically logged from completed replays".into()
    } else {
        format!("Automatically logged from completed replays · {unavailable} saved matches have no reward data")
    };
    txt(dc, &detail, x, 109, w, 20, 13, 400, MUTED);
}

pub unsafe fn draw_row(item: &DRAWITEMSTRUCT, cards: &[Card]) {
    let dc = item.hDC;
    let r = item.rcItem;
    fill(dc, r, PAPER);
    let Some(row) = cards.chunks(COLUMNS).nth(item.itemID as usize) else { return; };
    let width = (r.right - r.left - 23) / COLUMNS as i32;
    for (index, card) in row.iter().enumerate() {
        let x = r.left + index as i32 * (width + 10);
        let y = r.top + 2;
        rounded(dc, rect(x, y, width, 106), WHITE, 12);
        if card.count > 0 { fill(dc, rect(x + 1, y + 12, 2, 82), TEAL); }
        let title_x = x + if draw_art(dc, &card.id, x + 9, y + 14) { 87 } else { 14 };
        let text_w = x + width - title_x - 10;
        txt(dc, &format!("{} · {}", card.group, card.category), title_x, y + 10, text_w, 17, 11, 500, MUTED);
        draw_name(dc, &card.name, rect(title_x, y + 29, text_w, 34));
        let status = card.status();
        txt(dc, &status, title_x, y + 66, text_w, 19, 13, 600, if card.count > 0 { TEAL } else { MUTED });
        if !card.latest.is_empty() {
            txt(dc, &card.latest, title_x, y + 87, text_w, 17, 10, 400, MUTED);
        }
    }
    if item.itemState & ODS_FOCUS != 0 {
        let focus = rect(r.left + 3, r.top + 4, width * row.len() as i32 + 10 * (row.len() as i32 - 1) - 6, 101);
        DrawFocusRect(dc, &focus);
    }
}

unsafe fn draw_name(dc: HDC, value: &str, mut region: RECT) {
    let face = font("Segoe UI", 14, 600);
    let old = SelectObject(dc, face);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, INK);
    DrawTextW(dc, wide(value).as_ptr(), -1, &mut region, DT_WORDBREAK | DT_NOPREFIX | DT_END_ELLIPSIS);
    SelectObject(dc, old);
    DeleteObject(face);
}

unsafe fn draw_art(dc: HDC, id: &str, x: i32, y: i32) -> bool {
    const SIZE: i32 = 76;
    let Some((_, pixels)) = ART.iter().find(|(name, _)| *name == id) else { return false; };
    let mut bitmap: BITMAPINFO = zeroed();
    bitmap.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bitmap.bmiHeader.biWidth = SIZE;
    bitmap.bmiHeader.biHeight = -SIZE;
    bitmap.bmiHeader.biPlanes = 1;
    bitmap.bmiHeader.biBitCount = 32;
    StretchDIBits(dc, x, y, SIZE, SIZE, 0, 0, SIZE, SIZE, pixels.as_ptr().cast(),
        &bitmap, DIB_RGB_COLORS, SRCCOPY);
    true
}

const ART: &[(&str, &[u8])] = include!("reward_art.in");
