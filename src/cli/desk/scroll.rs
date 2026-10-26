//! Overflow scrolling for **Native Windows** (B32): scrollbars on both axes and their drags.
//! The vertical math is herdr's pane scrollbar (`crate::ui::scrollbar_*`); a horizontal bar runs
//! the same math with columns in place of rows.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

use crate::pane::ScrollMetrics;

const TRACK: Color = Color::Indexed(240);
const THUMB: Color = Color::Indexed(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Axis {
    Y,
    X,
}

/// `content` cells seen `viewport` at a time, the first shown at `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Extent {
    pub offset: usize,
    pub content: usize,
    pub viewport: usize,
}

impl Extent {
    pub fn new(offset: usize, content: usize, viewport: usize) -> Self {
        Self {
            offset,
            content,
            viewport,
        }
    }

    pub fn max(self) -> usize {
        self.content.saturating_sub(self.viewport)
    }

    pub fn overflows(self) -> bool {
        self.max() > 0
    }

    fn metrics(self) -> ScrollMetrics {
        ScrollMetrics {
            offset_from_bottom: self.max().saturating_sub(self.offset),
            max_offset_from_bottom: self.max(),
            viewport_rows: self.viewport,
        }
    }

    fn offset_of(self, offset_from_bottom: usize) -> usize {
        self.max().saturating_sub(offset_from_bottom)
    }
}

/// The track as herdr's vertical one: along-axis position in `y`, length in `height`.
fn along(axis: Axis, track: Rect) -> Rect {
    match axis {
        Axis::Y => track,
        Axis::X => Rect::new(track.y, track.x, track.height, track.width),
    }
}

fn pos(axis: Axis, point: (u16, u16)) -> u16 {
    match axis {
        Axis::Y => point.1,
        Axis::X => point.0,
    }
}

/// Thumb start and length along the axis.
fn thumb(axis: Axis, track: Rect, extent: Extent) -> Option<(u16, u16)> {
    crate::ui::scrollbar_thumb(extent.metrics(), along(axis, track)).map(|t| (t.top, t.len))
}

/// Draw the bar when the content overflows.
pub(super) fn draw(buf: &mut Buffer, axis: Axis, track: Rect, extent: Extent) {
    let Some((start, len)) = thumb(axis, track, extent) else {
        return;
    };
    let (line, block) = match axis {
        Axis::Y => ("▕", "▐"),
        Axis::X => ("▁", "▄"),
    };
    let cells: Vec<(u16, u16)> = match axis {
        Axis::Y => (track.y..track.bottom()).map(|y| (track.x, y)).collect(),
        Axis::X => (track.x..track.right()).map(|x| (x, track.y)).collect(),
    };
    for (x, y) in cells {
        let on = (start..start + len).contains(&pos(axis, (x, y)));
        let cell = &mut buf[(x, y)];
        cell.set_symbol(if on { block } else { line });
        cell.set_style(Style::new().fg(if on { THUMB } else { TRACK }));
    }
}

/// A press on a bar, followed through the drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BarDrag {
    pub axis: Axis,
    track: Rect,
    extent: Extent,
    grab: u16,
}

impl BarDrag {
    /// Press at `point`: on the thumb it keeps the grab point; elsewhere the thumb centers there.
    /// Returns the drag and the new offset.
    pub fn press(axis: Axis, track: Rect, extent: Extent, point: (u16, u16)) -> (Self, usize) {
        let t = along(axis, track);
        let at = pos(axis, point);
        let metrics = extent.metrics();
        let offset = match crate::ui::scrollbar_thumb_grab_offset(metrics, t, at) {
            Some(_) => extent.offset,
            None => extent.offset_of(crate::ui::scrollbar_offset_from_row(metrics, t, at)),
        };
        let moved = Extent { offset, ..extent };
        let grab = thumb(axis, track, moved).map_or(0, |(start, len)| {
            at.saturating_sub(start).min(len.saturating_sub(1))
        });
        let drag = Self {
            axis,
            track,
            extent: moved,
            grab,
        };
        (drag, offset)
    }

    /// The offset for the pointer at `point`.
    pub fn to(&self, point: (u16, u16)) -> usize {
        let row = crate::ui::scrollbar_offset_from_drag_row(
            self.extent.metrics(),
            along(self.axis, self.track),
            pos(self.axis, point),
            self.grab,
        );
        self.extent.offset_of(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_jump_and_drag_on_both_axes() {
        for axis in [Axis::Y, Axis::X] {
            let track = match axis {
                Axis::Y => Rect::new(10, 0, 1, 10),
                Axis::X => Rect::new(0, 10, 10, 1),
            };
            let at = |p: u16| match axis {
                Axis::Y => (10, p),
                Axis::X => (p, 10),
            };
            let extent = Extent::new(0, 100, 10);
            // Top thumb is grabbed in place.
            let (drag, offset) = BarDrag::press(axis, track, extent, at(0));
            assert_eq!(offset, 0, "{axis:?}");
            assert_eq!(drag.to(at(9)), 90, "{axis:?}");
            assert_eq!(drag.to(at(0)), 0, "{axis:?}");
            // The track end jumps to the end.
            let (_, offset) = BarDrag::press(axis, track, extent, at(9));
            assert_eq!(offset, 90, "{axis:?}");
        }
    }

    #[test]
    fn bars_only_draw_on_overflow() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 5));
        draw(
            &mut buf,
            Axis::X,
            Rect::new(0, 4, 5, 1),
            Extent::new(0, 5, 5),
        );
        assert_eq!(buf[(0, 4)].symbol(), " ");
        draw(
            &mut buf,
            Axis::X,
            Rect::new(0, 4, 5, 1),
            Extent::new(5, 10, 5),
        );
        assert_eq!(buf[(4, 4)].symbol(), "▄");
        assert_eq!(buf[(0, 4)].symbol(), "▁");
        assert!(!Extent::new(0, 5, 5).overflows() && Extent::new(0, 6, 5).overflows());
    }
}
