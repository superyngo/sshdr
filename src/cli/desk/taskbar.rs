//! **Taskbar** layout: one button per **Task** after the fixed buttons, clipped to the bar.

use super::text_width;

/// Widest Task label shown before it is cut with `…`.
const MAX_LABEL: u16 = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Button {
    pub x: u16,
    pub width: u16,
    pub text: String,
}

/// Button text for a **Task**: label (cut to [`MAX_LABEL`]) and its window count.
pub(super) fn task_text(label: &str, windows: usize) -> String {
    let mut cut = String::new();
    for ch in label.chars() {
        if text_width(&cut) + text_width(ch.encode_utf8(&mut [0; 4])) > MAX_LABEL - 1 {
            cut.push('…');
            break;
        }
        cut.push(ch);
    }
    format!(" {cut} ·{windows} ")
}

/// Lays out `texts` left to right from `x` with one gap column; stops at `right`.
pub(super) fn layout(x: u16, right: u16, texts: impl IntoIterator<Item = String>) -> Vec<Button> {
    let mut buttons = Vec::new();
    let mut x = x;
    for text in texts {
        let width = text_width(&text);
        if x.saturating_add(width) > right {
            break;
        }
        buttons.push(Button { x, width, text });
        x = x.saturating_add(width + 1);
    }
    buttons
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_labels_are_cut_and_counted() {
        assert_eq!(task_text("api", 2), " api ·2 ");
        let text = task_text("a-very-long-workspace-name", 1);
        assert!(text.contains('…'));
        assert!(text_width(&text) <= MAX_LABEL + 5);
    }

    #[test]
    fn buttons_stop_at_the_bar_edge() {
        let buttons = layout(
            10,
            30,
            ["12345678".to_string(), "12345678".into(), "xxxx".into()],
        );
        assert_eq!(buttons.len(), 2);
        assert_eq!(buttons[1].x, 19);
        assert_eq!(layout(0, 3, ["toolong".to_string()]), vec![]);
    }
}
