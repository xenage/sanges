use std::io::IsTerminal;

#[derive(Debug, Clone, Copy)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
pub struct Cell {
    plain: String,
    rendered: String,
    align: Align,
}

impl Cell {
    pub fn plain(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            plain: value.clone(),
            rendered: value,
            align: Align::Left,
        }
    }

    pub fn rendered(plain: impl Into<String>, rendered: impl Into<String>, align: Align) -> Self {
        Self {
            plain: plain.into(),
            rendered: rendered.into(),
            align,
        }
    }

    pub fn right(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            plain: value.clone(),
            rendered: value,
            align: Align::Right,
        }
    }

    fn width(&self) -> usize {
        self.plain.chars().count()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    color: bool,
}

impl Theme {
    pub fn stdout() -> Self {
        Self {
            color: std::io::stdout().is_terminal(),
        }
    }

    pub fn title(&self, value: impl AsRef<str>) -> String {
        self.paint(value.as_ref(), "1;38;5;45")
    }

    pub fn heading(&self, value: impl AsRef<str>) -> String {
        self.paint(value.as_ref(), "1;38;5;81")
    }

    pub fn subheading(&self, value: impl AsRef<str>) -> String {
        self.paint(value.as_ref(), "1;38;5;111")
    }

    pub fn dim(&self, value: impl AsRef<str>) -> String {
        self.paint(value.as_ref(), "2;38;5;246")
    }

    pub fn muted(&self, value: impl AsRef<str>) -> String {
        self.paint(value.as_ref(), "38;5;244")
    }

    pub fn code(&self, value: impl AsRef<str>) -> String {
        self.paint(value.as_ref(), "38;5;186")
    }

    pub fn badge(&self, label: &str, style: BadgeStyle) -> String {
        let code = match style {
            BadgeStyle::Info => "1;30;48;5;117",
            BadgeStyle::Success => "1;30;48;5;119",
            BadgeStyle::Warning => "1;30;48;5;221",
            BadgeStyle::Danger => "1;37;48;5;160",
            BadgeStyle::Accent => "1;30;48;5;183",
            BadgeStyle::Muted => "1;37;48;5;240",
        };
        self.paint(label, code)
    }

    pub fn table(&self, headers: Vec<Cell>, rows: Vec<Vec<Cell>>) -> String {
        if headers.is_empty() {
            return String::new();
        }
        let mut widths = headers.iter().map(Cell::width).collect::<Vec<_>>();
        for row in &rows {
            for (index, cell) in row.iter().enumerate() {
                if let Some(width) = widths.get_mut(index) {
                    *width = (*width).max(cell.width());
                }
            }
        }

        let border = |left: char, mid: char, right: char| {
            let mut line = String::new();
            line.push(left);
            for (index, width) in widths.iter().enumerate() {
                line.push_str(&"─".repeat(width.saturating_add(2)));
                line.push(if index + 1 == widths.len() {
                    right
                } else {
                    mid
                });
            }
            line
        };

        let mut output = String::new();
        output.push_str(&self.muted(border('┌', '┬', '┐')));
        output.push('\n');
        output.push_str(&render_row(&headers, &widths, '│', |value| {
            self.subheading(value)
        }));
        output.push('\n');
        output.push_str(&self.muted(border('├', '┼', '┤')));
        for row in rows {
            output.push('\n');
            output.push_str(&render_row(&row, &widths, '│', |value| value.to_string()));
        }
        output.push('\n');
        output.push_str(&self.muted(border('└', '┴', '┘')));
        output
    }

    fn paint(&self, value: &str, code: &str) -> String {
        if self.color {
            format!("\u{1b}[{code}m{value}\u{1b}[0m")
        } else {
            value.to_string()
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum BadgeStyle {
    Info,
    Success,
    Warning,
    Danger,
    Accent,
    Muted,
}

fn render_row(
    cells: &[Cell],
    widths: &[usize],
    sep: char,
    style: impl Fn(&str) -> String,
) -> String {
    let mut row = String::new();
    row.push(sep);
    for (index, cell) in cells.iter().enumerate() {
        row.push(' ');
        row.push_str(&style(&pad(
            &cell.rendered,
            widths[index],
            cell.align,
            cell.width(),
        )));
        row.push(' ');
        row.push(sep);
    }
    row
}

fn pad(value: &str, width: usize, align: Align, measured_width: usize) -> String {
    let pad = width.saturating_sub(measured_width);
    match align {
        Align::Left => format!("{value}{}", " ".repeat(pad)),
        Align::Right => format!("{}{value}", " ".repeat(pad)),
        Align::Center => {
            let left = pad / 2;
            let right = pad - left;
            format!("{}{}{}", " ".repeat(left), value, " ".repeat(right))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Align, BadgeStyle, Cell, Theme};

    #[test]
    fn badge_without_color_has_no_extra_padding() {
        let theme = Theme { color: false };

        assert_eq!(theme.badge("STOPPED", BadgeStyle::Warning), "STOPPED");
    }

    #[test]
    fn table_keeps_rendered_cells_aligned() {
        let theme = Theme { color: false };
        let table = theme.table(
            vec![Cell::plain("STATUS")],
            vec![vec![Cell::rendered(
                "STOPPED",
                theme.badge("STOPPED", BadgeStyle::Warning),
                Align::Center,
            )]],
        );

        assert!(table.contains("│ STATUS  │"));
        assert!(table.contains("│ STOPPED │"));
        assert!(!table.contains("│  STOPPED  │"));
    }
}
