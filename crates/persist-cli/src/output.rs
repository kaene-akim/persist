//! Terminal output helpers.

use console::{style, Term};
use serde::Serialize;

/// Whether stdout is a terminal (affects colour and progress bars).
pub fn is_tty() -> bool {
    Term::stdout().is_term()
}

/// Print a heading.
pub fn heading(text: &str) {
    println!("\n{}", style(text).bold().underlined());
}

/// Print a key/value line.
pub fn kv(key: &str, value: &str) {
    println!("  {:<22} {}", style(key).dim(), value);
}

/// Print a success message.
pub fn success(msg: &str) {
    println!("{} {}", style("✓").green().bold(), msg);
}

/// Print a warning.
pub fn warn(msg: &str) {
    eprintln!("{} {}", style("!").yellow().bold(), msg);
}

/// Print an error.
pub fn error(msg: &str) {
    eprintln!("{} {}", style("✗").red().bold(), msg);
}

/// Print a note.
pub fn note(msg: &str) {
    println!("{}", style(msg).dim());
}

/// Render a simple table.
pub fn table(headers: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() {
        return;
    }
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(display_width(cell));
            }
        }
    }
    let header_line = headers
        .iter()
        .enumerate()
        .map(|(i, h)| pad(h, widths[i]))
        .collect::<Vec<_>>()
        .join("  ");
    println!("{}", style(header_line).bold());
    for row in rows {
        let line = row
            .iter()
            .enumerate()
            .map(|(i, c)| pad(c, widths.get(i).copied().unwrap_or(0)))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{line}");
    }
}

fn display_width(s: &str) -> usize {
    console::measure_text_width(s)
}

fn pad(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

/// Print a value as pretty JSON.
pub fn json<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("failed to serialise JSON: {e}"),
    }
}

/// Format a timestamp for display.
pub fn timestamp(ts: chrono::DateTime<chrono::Utc>) -> String {
    ts.with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}
