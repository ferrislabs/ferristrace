use std::fmt::Write;

pub const MAX_ROWS: usize = 200;
pub const MAX_BYTES: usize = 64 * 1024;
const NOTE_RESERVE: usize = 256;
const MAX_CELL_CHARS: usize = 300;
const MAX_HEADER_CHARS: usize = 600;

#[derive(Debug)]
enum Block {
    Bullets(Vec<String>),
    Table {
        title: Option<String>,
        columns: &'static [&'static str],
        rows: Vec<Vec<String>>,
    },
}

#[derive(Debug)]
pub struct Report {
    header: String,
    blocks: Vec<Block>,
}

pub fn clean(text: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max_chars + 3));
    for (kept, character) in text.chars().enumerate() {
        if kept == max_chars {
            out.push('…');
            return out;
        }
        out.push(match character {
            '\n' | '\r' | '\t' => ' ',
            '|' => '/',
            other => other,
        });
    }
    out
}

impl Report {
    pub fn new(header: impl AsRef<str>) -> Self {
        Self {
            header: clean(header.as_ref(), MAX_HEADER_CHARS),
            blocks: Vec::new(),
        }
    }

    pub fn bullets(mut self, lines: Vec<String>) -> Self {
        self.blocks.push(Block::Bullets(lines));
        self
    }

    pub fn table(
        mut self,
        title: Option<&str>,
        columns: &'static [&'static str],
        rows: Vec<Vec<String>>,
    ) -> Self {
        self.blocks.push(Block::Table {
            title: title.map(ToOwned::to_owned),
            columns,
            rows,
        });
        self
    }

    pub fn rows(&self) -> usize {
        self.blocks
            .iter()
            .map(|block| match block {
                Block::Bullets(lines) => lines.len(),
                Block::Table { rows, .. } => rows.len(),
            })
            .sum()
    }

    pub fn render(&self) -> String {
        let total = self.rows();
        let mut out = String::new();
        out.push_str(&self.header);
        out.push('\n');
        let mut shown = 0;
        let mut full = false;
        for block in &self.blocks {
            match block {
                Block::Bullets(lines) => {
                    out.push('\n');
                    for line in lines {
                        let text = format!("- {}", clean(line, MAX_CELL_CHARS));
                        if !admit(&mut out, &text, &mut shown, &mut full) {
                            break;
                        }
                    }
                }
                Block::Table {
                    title,
                    columns,
                    rows,
                } => {
                    out.push('\n');
                    if let Some(title) = title {
                        out.push_str(&clean(title, MAX_CELL_CHARS));
                        out.push('\n');
                    }
                    if rows.is_empty() {
                        out.push_str("none\n");
                        continue;
                    }
                    out.push_str(&columns.join(" | "));
                    out.push('\n');
                    for row in rows {
                        let text = row
                            .iter()
                            .map(|value| clean(value, MAX_CELL_CHARS))
                            .collect::<Vec<_>>()
                            .join(" | ");
                        if !admit(&mut out, &text, &mut shown, &mut full) {
                            break;
                        }
                    }
                }
            }
        }
        if full {
            let _ = writeln!(
                out,
                "\n[truncated: showing {shown} of {total} rows; narrow the window or add filters]"
            );
        }
        out
    }
}

fn admit(out: &mut String, line: &str, shown: &mut usize, full: &mut bool) -> bool {
    if *full || *shown >= MAX_ROWS || out.len() + line.len() + 1 > MAX_BYTES - NOTE_RESERVE {
        *full = true;
        return false;
    }
    out.push_str(line);
    out.push('\n');
    *shown += 1;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLUMNS: &[&str] = &["name", "n"];

    fn rows(count: usize) -> Vec<Vec<String>> {
        (0..count)
            .map(|i| vec![format!("row{i}"), i.to_string()])
            .collect()
    }

    #[test]
    fn a_small_table_is_header_columns_and_rows() {
        let text = Report::new("2 things in the last 1h")
            .table(None, COLUMNS, rows(2))
            .render();
        assert_eq!(
            text,
            "2 things in the last 1h\n\nname | n\nrow0 | 0\nrow1 | 1\n"
        );
    }

    #[test]
    fn bullets_are_prefixed() {
        let text = Report::new("head")
            .bullets(vec!["a: 1".to_owned(), "b: 2".to_owned()])
            .render();
        assert_eq!(text, "head\n\n- a: 1\n- b: 2\n");
    }

    #[test]
    fn an_empty_table_says_none() {
        let text = Report::new("head")
            .table(Some("Pods"), COLUMNS, Vec::new())
            .render();
        assert_eq!(text, "head\n\nPods\nnone\n");
    }

    #[test]
    fn rows_are_capped_and_the_cut_is_announced() {
        let report = Report::new("many").table(None, COLUMNS, rows(MAX_ROWS + 50));
        let text = report.render();
        assert_eq!(
            text.lines().filter(|l| l.starts_with("row")).count(),
            MAX_ROWS
        );
        assert!(text.contains("row199 | 199"));
        assert!(!text.contains("row200"));
        assert!(
            text.trim_end().ends_with(
                "[truncated: showing 200 of 250 rows; narrow the window or add filters]"
            ),
            "{text}"
        );
    }

    #[test]
    fn exactly_the_cap_is_not_truncated() {
        let text = Report::new("exact")
            .table(None, COLUMNS, rows(MAX_ROWS))
            .render();
        assert!(!text.contains("truncated"));
    }

    #[test]
    fn the_cap_spans_every_block_of_a_report() {
        let report = Report::new("two blocks")
            .table(Some("A"), COLUMNS, rows(150))
            .table(Some("B"), COLUMNS, rows(150));
        let text = report.render();
        assert!(text.contains("showing 200 of 300 rows"));
        assert_eq!(
            text.lines().filter(|l| l.starts_with("row")).count(),
            MAX_ROWS
        );
        let second = text.split("\nB\n").nth(1).unwrap();
        assert_eq!(second.lines().filter(|l| l.starts_with("row")).count(), 50);
    }

    #[test]
    fn bytes_are_capped_even_when_rows_are_few() {
        let fat = "x".repeat(MAX_CELL_CHARS * 2);
        let many = (0..MAX_ROWS)
            .map(|_| vec![fat.clone(), fat.clone(), fat.clone(), fat.clone()])
            .collect();
        let text = Report::new("fat")
            .table(None, &["a", "b", "c", "d"], many)
            .render();
        assert!(text.len() <= MAX_BYTES, "{}", text.len());
        assert!(
            text.contains("[truncated: showing"),
            "{}",
            &text[text.len() - 120..]
        );
        assert!(text.contains("of 200 rows"));
    }

    #[test]
    fn cells_stay_on_one_line_and_never_break_the_columns() {
        let text = Report::new("h")
            .table(
                None,
                COLUMNS,
                vec![vec!["a|b\nc\td".to_owned(), "1".to_owned()]],
            )
            .render();
        assert!(text.contains("a/b c d | 1"), "{text}");
    }

    #[test]
    fn long_cells_are_cut_on_a_character_boundary() {
        let long = "é".repeat(MAX_CELL_CHARS + 10);
        let cut = clean(&long, MAX_CELL_CHARS);
        assert_eq!(cut.chars().count(), MAX_CELL_CHARS + 1);
        assert!(cut.ends_with('…'));
        assert_eq!(clean("short", 10), "short");
    }
}
