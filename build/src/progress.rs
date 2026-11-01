//! Show a build's progress: a live line per running step with a bar, its time and what it is doing,
//! above them the steps that finished. Without a terminal only the finished steps are printed.

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::time::Duration;

/// A running step as the display shows it.
pub struct Row<'a> {
    pub name: &'a str,
    pub elapsed: Duration,
    /// How long the step took last time, if it ever finished.
    pub expected: Option<Duration>,
    /// The step's log, which tells what it is doing.
    pub log: &'a Path,
}

/// The live part of the terminal.
pub struct Progress {
    live: bool,
    width: usize,
    drawn: usize,
}

impl Progress {
    /// Start a display on standard output, live if it is a terminal.
    pub fn new() -> Progress {
        let live = std::io::stdout().is_terminal();
        Progress { live, width: if live { terminal_width() } else { 0 }, drawn: 0 }
    }

    /// Print a line that stays, above the live lines.
    ///
    /// # Arguments
    ///
    /// * `text` - the line
    pub fn line(&mut self, text: &str) {
        self.clear();
        println!("{text}");
    }

    /// Redraw the live lines of the running steps.
    ///
    /// # Arguments
    ///
    /// * `rows` - the running steps
    pub fn draw(&mut self, rows: &[Row]) {
        if !self.live {
            return;
        }
        self.clear();
        let mut out = std::io::stdout().lock();
        for row in rows {
            let line = format!("{:14} {} {:>6.1} s  {}", row.name, bar(row.elapsed, row.expected), row.elapsed.as_secs_f64(), activity(row.log));
            let _ = writeln!(out, "{}", line.chars().take(self.width.saturating_sub(1)).collect::<String>());
        }
        let _ = out.flush();
        self.drawn = rows.len();
    }

    /// Erase the live lines.
    fn clear(&mut self) {
        if self.drawn > 0 {
            print!("\x1b[{}A\x1b[J", self.drawn);
            self.drawn = 0;
        }
    }
}

/// Draw a bar of the elapsed time against the expected one.
///
/// # Arguments
///
/// * `elapsed` - how long the step runs
/// * `expected` - how long it took last time
///
/// # Returns
///
/// The bar, or a moving marker when there is nothing to compare with.
fn bar(elapsed: Duration, expected: Option<Duration>) -> String {
    const WIDTH: usize = 20;
    match expected.filter(|e| !e.is_zero()) {
        Some(expected) => {
            let share = (elapsed.as_secs_f64() / expected.as_secs_f64()).min(0.99);
            let full = (share * WIDTH as f64) as usize;
            format!("[{}{}] {:>3.0}%", "█".repeat(full), "░".repeat(WIDTH - full), share * 100.0)
        }
        None => {
            let at = (elapsed.as_millis() / 200) as usize % WIDTH;
            format!("[{}█{}]  …  ", "░".repeat(at), "░".repeat(WIDTH - 1 - at))
        }
    }
}

/// Tell what a step is doing from the end of its log: the package Buildroot is at, the crate cargo
/// compiles, the file the kernel build compiles, or else the last line that is not make's own.
///
/// # Arguments
///
/// * `log` - the step's log
///
/// # Returns
///
/// One line, without colours.
fn activity(log: &Path) -> String {
    let text = tail(log, 16 * 1024);
    let lines: Vec<String> = text.lines().map(plain)
        .filter(|l| !l.is_empty() && !l.starts_with("make[") && !l.starts_with("make:"))
        .collect();
    let last = |found: &dyn Fn(&str) -> bool| lines.iter().rev().find(|l| found(l)).cloned();
    last(&|l| l.starts_with(">>>"))
        .or_else(|| last(&|l| l.starts_with("Compiling ")))
        .or_else(|| last(&|l| l.split_whitespace().next().is_some_and(|w| w.len() <= 8 && w.chars().all(|c| c.is_ascii_uppercase())) && l.contains('/')))
        .or_else(|| lines.last().cloned())
        .unwrap_or_default()
}

/// Read the end of a file.
///
/// # Arguments
///
/// * `path` - the file
/// * `bytes` - how much of its end to read
///
/// # Returns
///
/// The text, empty if the file cannot be read.
fn tail(path: &Path, bytes: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return String::new() };
    let length = file.metadata().map_or(0, |m| m.len());
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(bytes)));
    let mut buffer = Vec::new();
    let _ = file.read_to_end(&mut buffer);
    String::from_utf8_lossy(&buffer).into_owned()
}

/// Remove terminal colour codes and surrounding space from a line.
///
/// # Arguments
///
/// * `line` - the line
///
/// # Returns
///
/// The plain text.
fn plain(line: &str) -> String {
    let mut text = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            chars.by_ref().find(|c| c.is_ascii_alphabetic());
        } else if c != '\r' {
            text.push(c);
        }
    }
    text.trim().to_string()
}

/// Ask the terminal how wide it is.
///
/// # Returns
///
/// Its width in columns, 100 if it cannot tell (a container's terminal may say 0).
fn terminal_width() -> usize {
    std::process::Command::new("stty").arg("size").stdin(std::process::Stdio::inherit()).output().ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().nth(1)?.parse().ok())
        .filter(|width| *width >= 40)
        .unwrap_or(100)
}
