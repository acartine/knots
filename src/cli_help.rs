use std::io::IsTerminal;

use clap::CommandFactory;

use crate::cli::Cli;

pub(crate) const COMMON_COMMANDS: &[&str] = &[
    "claim", "init", "ls", "new", "poll", "q", "sync", "update", "upgrade",
];

pub fn is_toplevel_help(args: &[String]) -> bool {
    let non_global: Vec<&str> = args[1..]
        .iter()
        .map(|s| s.as_str())
        .filter(|a| !a.starts_with("-d") && !a.starts_with("--db") && !a.starts_with("-C"))
        .collect();
    matches!(non_global.as_slice(), [] | ["--help"] | ["-h"])
}

pub fn print_custom_help() {
    let color = std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
    let p = Paint { color };
    let cmd = Cli::command();

    let subs: Vec<_> = cmd.get_subcommands().collect();
    let (mut common, mut other): (Vec<_>, Vec<_>) = subs
        .into_iter()
        .partition(|c| COMMON_COMMANDS.contains(&c.get_name()));
    common.sort_by_key(|c| c.get_name());
    other.sort_by_key(|c| c.get_name());

    let pad = cmd
        .get_subcommands()
        .map(|c| c.get_name().len())
        .max()
        .unwrap_or(0);
    let ver = cmd.get_version().unwrap_or("?");
    let about = cmd.get_about().map(|s| s.to_string()).unwrap_or_default();

    println!("{} {ver} — {about}\n", p.cyan_bold("kno"));
    println!(
        "{} {} [OPTIONS] <COMMAND>\n",
        p.yellow_bold("Usage:"),
        p.green_bold("kno")
    );

    println!("{}", p.cyan_bold("Common Commands:"));
    for c in &common {
        print_cmd_row(c, pad, &p);
    }

    println!("\n{}", p.cyan_bold("Other Commands:"));
    for c in &other {
        if c.get_name() != "help" {
            print_cmd_row(c, pad, &p);
        }
    }

    println!("\n{}", p.cyan_bold("Options:"));
    for arg in cmd.get_arguments() {
        if arg.is_hide_set() {
            continue;
        }
        let mut names = Vec::new();
        if let Some(s) = arg.get_short() {
            names.push(format!("-{s}"));
        }
        if let Some(l) = arg.get_long() {
            names.push(format!("--{l}"));
        }
        let flag = names.join(", ");
        let help = arg.get_help().map(|s| s.to_string()).unwrap_or_default();
        println!("  {}  {help}", p.green_bold(&format!("{flag:<20}")));
    }
    println!(
        "  {}  Print help",
        p.green_bold(&format!("{:<20}", "-h, --help"))
    );
    println!(
        "  {}  Print version",
        p.green_bold(&format!("{:<20}", "-V, --version"))
    );
}

fn print_cmd_row(cmd: &clap::Command, pad: usize, p: &Paint) {
    let name = cmd.get_name();
    let about = cmd.get_about().map(|s| s.to_string()).unwrap_or_default();
    println!("  {}  {about}", p.green_bold(&format!("{name:<pad$}")));
}

struct Paint {
    color: bool,
}

impl Paint {
    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
    fn cyan_bold(&self, t: &str) -> String {
        self.paint("1;36", t)
    }
    fn yellow_bold(&self, t: &str) -> String {
        self.paint("1;33", t)
    }
    fn green_bold(&self, t: &str) -> String {
        self.paint("1;32", t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toplevel_help_detection() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_toplevel_help(&args(&["kno"])));
        assert!(is_toplevel_help(&args(&["kno", "--help"])));
        assert!(is_toplevel_help(&args(&["kno", "-h"])));
        assert!(!is_toplevel_help(&args(&["kno", "ls"])));
        assert!(!is_toplevel_help(&args(&["kno", "ls", "--help"])));
        assert!(!is_toplevel_help(&args(&["kno", "--version"])));
    }

    #[test]
    fn paint_no_color() {
        let p = Paint { color: false };
        assert_eq!(p.cyan_bold("hi"), "hi");
        assert_eq!(p.yellow_bold("hi"), "hi");
        assert_eq!(p.green_bold("hi"), "hi");
    }

    #[test]
    fn paint_with_color() {
        let p = Paint { color: true };
        assert!(p.cyan_bold("hi").contains("\x1b[1;36m"));
        assert!(p.yellow_bold("hi").contains("\x1b[1;33m"));
        assert!(p.green_bold("hi").contains("\x1b[1;32m"));
        assert!(p.cyan_bold("hi").ends_with("\x1b[0m"));
    }

    #[test]
    fn print_custom_help_does_not_panic() {
        // Redirect via NO_COLOR to avoid terminal detection issues.
        let prev = std::env::var_os("NO_COLOR");
        unsafe { std::env::set_var("NO_COLOR", "1") };
        print_custom_help();
        if let Some(val) = prev {
            unsafe { std::env::set_var("NO_COLOR", val) };
        } else {
            unsafe { std::env::remove_var("NO_COLOR") };
        }
    }

    #[test]
    fn print_cmd_row_formats_name_and_about() {
        let cmd = clap::Command::new("test-cmd").about("A test.");
        let p = Paint { color: false };
        // Just verify it doesn't panic; output goes to stdout.
        print_cmd_row(&cmd, 12, &p);
    }

    #[test]
    fn common_commands_list_is_sorted() {
        let mut sorted = COMMON_COMMANDS.to_vec();
        sorted.sort();
        assert_eq!(COMMON_COMMANDS, sorted.as_slice());
    }

    #[test]
    fn toplevel_help_ignores_global_flags() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Flags starting with -d/--db/-C are stripped; their values are not.
        assert!(is_toplevel_help(&args(&["kno", "-dfoo.db"])));
        assert!(is_toplevel_help(&args(&["kno", "--db=foo.db"])));
        assert!(is_toplevel_help(&args(&["kno", "-C/tmp"])));
        assert!(is_toplevel_help(&args(&["kno", "-dfoo.db", "--help"])));
    }
}
