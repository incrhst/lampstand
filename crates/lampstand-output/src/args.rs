//! Command line.

use std::path::PathBuf;

pub const USAGE: &str = "\
usage: lampstand-output [--server URL] [--display N]... [--windowed] [--calibration PATH] [--verbose] [--list]

  --server URL        WebSocket of the Lampstand server (default ws://127.0.0.1:7420/ws)
  --display N         show on display N (repeatable; see --list). Default: every
                      display but the primary; with only one display, a window on it
  --windowed          show each output in an ordinary window, for development
  --calibration PATH  margins file (default $LAMPSTAND_DATA/calibration.json, else
                      ~/Library/Application Support/Lampstand/calibration.json)
  --verbose           log every frame as it reaches the screen, with its latency
  --list              list displays and exit";

#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    pub server: String,
    /// Empty means the default choice of displays.
    pub displays: Vec<usize>,
    pub windowed: bool,
    pub list: bool,
    pub verbose: bool,
    pub calibration: PathBuf,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            server: format!("ws://127.0.0.1:{}/ws", lampstand_show::protocol::DEFAULT_PORT),
            displays: Vec::new(),
            windowed: false,
            list: false,
            verbose: false,
            calibration: default_calibration_path(),
        }
    }
}

/// `$LAMPSTAND_DATA/calibration.json`, else the macOS data directory.
pub fn default_calibration_path() -> PathBuf {
    data_dir().join("calibration.json")
}

fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LAMPSTAND_DATA").filter(|d| !d.is_empty()) {
        return dir.into();
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Library/Application Support/Lampstand")
}

impl Args {
    /// Parse arguments (without the program name).
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
        let mut out = Args::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--server" => out.server = args.next().ok_or("--server needs a URL")?,
                "--display" => {
                    let n = args.next().and_then(|v| v.parse().ok()).ok_or("--display needs a number; see --list")?;
                    out.displays.push(n);
                }
                "--windowed" => out.windowed = true,
                "--list" => out.list = true,
                "--verbose" => out.verbose = true,
                "--calibration" => out.calibration = args.next().ok_or("--calibration needs a path")?.into(),
                "-h" | "--help" => return Err(USAGE.into()),
                other => return Err(format!("unknown argument {other}\n{USAGE}")),
            }
        }
        Ok(out)
    }
}

/// Which displays to use when none are named: every display but the
/// operator's (the primary) when there is more than one. With one display
/// the result is empty, and the caller opens a window on it instead of
/// covering the operator's screen.
pub fn default_displays(count: usize, primary: Option<usize>) -> Vec<usize> {
    if count <= 1 {
        return Vec::new();
    }
    let primary = primary.unwrap_or(0);
    (0..count).filter(|&i| i != primary).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Args, String> {
        Args::parse(s.split_whitespace().map(String::from))
    }

    #[test]
    fn defaults() {
        let a = parse("").unwrap();
        assert_eq!(a.server, "ws://127.0.0.1:7420/ws");
        assert!(a.displays.is_empty() && !a.windowed && !a.list);
        assert!(a.calibration.ends_with("calibration.json"));
    }

    #[test]
    fn repeatable_displays() {
        let a = parse("--display 1 --display 2 --windowed --server ws://x:1/ws --calibration /tmp/c.json").unwrap();
        assert_eq!(a.displays, vec![1, 2]);
        assert!(a.windowed);
        assert_eq!(a.server, "ws://x:1/ws");
        assert_eq!(a.calibration, PathBuf::from("/tmp/c.json"));
        assert!(parse("--display x").is_err());
        assert!(parse("--bogus").is_err());
    }

    #[test]
    fn skips_the_operator_display() {
        assert_eq!(default_displays(1, Some(0)), Vec::<usize>::new());
        assert_eq!(default_displays(2, Some(0)), vec![1]);
        assert_eq!(default_displays(3, Some(1)), vec![0, 2]);
        assert_eq!(default_displays(2, None), vec![1]);
    }
}
