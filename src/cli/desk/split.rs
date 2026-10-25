//! Dragging the dividers between a **Terminal Window**'s panes (B28): which split a divider
//! cell belongs to, the ratio a pointer asks for, and sending it off the UI thread.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ratatui::layout::Rect;

use super::server::{self, SplitSlot, TabInfo};
use super::transport::Target;

/// How often a drag sends a ratio (the herdr shell's rate).
const SEND_EVERY: Duration = Duration::from_millis(33);

/// `rect` (in [`TabInfo::area`] cells) scaled into `area`, as `layout_rects` scales panes.
pub(super) fn scaled(tab: &TabInfo, rect: Rect, area: Rect) -> Rect {
    let (aw, ah) = (u32::from(tab.area.0.max(1)), u32::from(tab.area.1.max(1)));
    let scale = |v: u16, total: u32, size: u16| {
        let scaled = (u32::from(v) * u32::from(size) + total / 2) / total;
        u16::try_from(scaled).unwrap_or(size).min(size)
    };
    let left = area.x + scale(rect.x, aw, area.width);
    let top = area.y + scale(rect.y, ah, area.height);
    let right = area.x + scale(rect.x.saturating_add(rect.width), aw, area.width);
    let bottom = area.y + scale(rect.y.saturating_add(rect.height), ah, area.height);
    Rect::new(
        left,
        top,
        right.saturating_sub(left),
        bottom.saturating_sub(top),
    )
}

/// A split being dragged.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SplitDrag {
    pub target: Target,
    pub tab_id: String,
    pub path: Vec<bool>,
    pub across: bool,
    /// The split's area on screen.
    pub rect: Rect,
    /// The split's boundary minus the pointer at the press, so the ratio does not jump.
    pub grab: i32,
    pub sent: Option<(f32, Instant)>,
}

impl SplitDrag {
    /// The split whose divider is nearest `point` (within one cell) among those containing it;
    /// the deepest wins a tie.
    pub fn pick(tab: &TabInfo, area: Rect, point: (u16, u16), target: &Target) -> Option<Self> {
        let pos = ratatui::layout::Position::new(point.0, point.1);
        let mut best: Option<(i32, usize, &SplitSlot, Rect, i32)> = None;
        for split in &tab.splits {
            let rect = scaled(tab, split.rect, area);
            if !rect.contains(pos) {
                continue;
            }
            let boundary = boundary(split, rect);
            let pointer = i32::from(if split.across { point.0 } else { point.1 });
            // The divider cell sits just before the boundary (`layout_rects`).
            let distance = (boundary - 1 - pointer).abs();
            if distance > 1 {
                continue;
            }
            let better = best.as_ref().is_none_or(|(d, depth, ..)| {
                distance < *d || (distance == *d && split.path.len() > *depth)
            });
            if better {
                best = Some((distance, split.path.len(), split, rect, boundary - pointer));
            }
        }
        best.map(|(_, _, split, rect, grab)| Self {
            target: target.clone(),
            tab_id: tab.tab_id.clone(),
            path: split.path.clone(),
            across: split.across,
            rect,
            grab,
            sent: None,
        })
    }

    /// The ratio the pointer asks for (herdr shell `pane_split_ratio`).
    pub fn ratio(&self, point: (u16, u16)) -> f32 {
        let (pointer, origin, length) = if self.across {
            (i32::from(point.0), i32::from(self.rect.x), self.rect.width)
        } else {
            (i32::from(point.1), i32::from(self.rect.y), self.rect.height)
        };
        ((pointer + self.grab - origin) as f32 / f32::from(length.max(1))).clamp(0.1, 0.9)
    }

    /// Send the pointer's ratio when it changed; `last` (the release) skips the rate limit.
    pub fn send(&mut self, point: (u16, u16), last: bool, sender: &RatioSender) {
        let ratio = self.ratio(point);
        let now = Instant::now();
        let due = match self.sent {
            None => true,
            Some((sent, at)) => {
                (sent - ratio).abs() > f32::EPSILON
                    && (last || now.duration_since(at) >= SEND_EVERY)
            }
        };
        if due {
            self.sent = Some((ratio, now));
            sender.submit(Job {
                target: self.target.clone(),
                tab_id: self.tab_id.clone(),
                path: self.path.clone(),
                ratio,
            });
        }
    }
}

/// Screen position where the split's first child ends, along its axis.
fn boundary(split: &SplitSlot, rect: Rect) -> i32 {
    let (origin, length) = if split.across {
        (rect.x, rect.width)
    } else {
        (rect.y, rect.height)
    };
    let offset = (u32::from(length) * u32::from(split.ratio) + 500) / 1000;
    i32::from(origin) + i32::try_from(offset).unwrap_or(0)
}

struct Job {
    target: Target,
    tab_id: String,
    path: Vec<bool>,
    ratio: f32,
}

/// Sends ratios on a worker thread, one call at a time; a newer ratio replaces a waiting one.
#[derive(Default)]
pub(super) struct RatioSender {
    slot: Arc<Mutex<(Option<Job>, bool)>>,
}

impl RatioSender {
    fn submit(&self, job: Job) {
        let Ok(mut slot) = self.slot.lock() else {
            return;
        };
        slot.0 = Some(job);
        if slot.1 {
            return;
        }
        slot.1 = true;
        let shared = Arc::clone(&self.slot);
        std::thread::spawn(move || loop {
            let job = match shared.lock() {
                Ok(mut slot) => match slot.0.take() {
                    Some(job) => job,
                    None => {
                        slot.1 = false;
                        return;
                    }
                },
                Err(_) => return,
            };
            server::set_split_ratio(&job.target, &job.tab_id, job.path, job.ratio);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab() -> TabInfo {
        TabInfo {
            tab_id: "w1:t1".into(),
            workspace_id: "w1".into(),
            label: "one".into(),
            pane_id: "p1".into(),
            terminal_id: "t1".into(),
            panes: Vec::new(),
            area: (120, 40),
            focused_pane_id: "p1".into(),
            splits: vec![
                SplitSlot {
                    path: Vec::new(),
                    across: true,
                    ratio: 500,
                    rect: Rect::new(0, 0, 120, 40),
                },
                SplitSlot {
                    path: vec![false],
                    across: false,
                    ratio: 300,
                    rect: Rect::new(0, 0, 60, 40),
                },
            ],
        }
    }

    #[test]
    fn divider_cells_pick_their_split_and_keep_the_grab() {
        let area = Rect::new(10, 5, 60, 20);
        let tab = tab();
        // Root divider: 60 * 0.5 = 30 → column 10 + 30 - 1 = 39.
        let root = SplitDrag::pick(&tab, area, (39, 15), &Target::Local).unwrap();
        assert!(root.path.is_empty() && root.across);
        assert_eq!(root.grab, 1);
        assert_eq!(root.ratio((39, 15)), 0.5);
        assert!((root.ratio((49, 15)) - 0.6667).abs() < 0.01);
        // The left stack's divider: 20 * 0.3 = 6 → row 5 + 6 - 1 = 10.
        let left = SplitDrag::pick(&tab, area, (20, 10), &Target::Local).unwrap();
        assert_eq!(left.path, [false]);
        assert_eq!(left.ratio((20, 0)), 0.1);
        // Far from any divider.
        assert!(SplitDrag::pick(&tab, area, (60, 20), &Target::Local).is_none());
    }
}
