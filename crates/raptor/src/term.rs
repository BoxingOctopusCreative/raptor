use std::io::{self, IsTerminal, Write};

use anstyle::{AnsiColor, Color, Style};
use clap::ColorChoice;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn init(choice: ColorChoice) {
    let enabled = match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => {
            std::env::var("NO_COLOR").is_err()
                && (io::stdout().is_terminal() || io::stderr().is_terminal())
        }
    };
    ENABLED.store(enabled, Ordering::Relaxed);
}

fn on() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn style(color: AnsiColor, bold: bool) -> Style {
    let mut s = Style::new().fg_color(Some(Color::Ansi(color)));
    if bold {
        s = s.bold();
    }
    s
}

fn styled(text: &str, style: Style) -> String {
    if on() {
        format!("{style}{text}{}", Style::new().render_reset())
    } else {
        text.to_string()
    }
}

fn dim(text: &str) -> String {
    styled(
        text,
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::BrightBlack))),
    )
}

fn println_line(line: String) {
    println!("{line}");
}

pub fn error_line(msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    if on() {
        let _ = writeln!(
            io::stderr(),
            "{} {msg}",
            styled("E:", style(AnsiColor::Red, true))
        );
    } else {
        eprintln!("E: {msg}");
    }
}

pub fn warn_line(msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    if on() {
        let _ = writeln!(
            io::stderr(),
            "{} {msg}",
            styled("W:", style(AnsiColor::Yellow, true))
        );
    } else {
        eprintln!("W: {msg}");
    }
}

pub fn note_line(msg: impl AsRef<str>) {
    println_line(styled(msg.as_ref(), style(AnsiColor::Cyan, false)));
}

pub fn success_line(msg: impl AsRef<str>) {
    println_line(styled(msg.as_ref(), style(AnsiColor::Green, false)));
}

pub fn hit(msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    if on() {
        println_line(format!(
            "{} {msg}",
            styled("Hit:1", style(AnsiColor::Cyan, true))
        ));
    } else {
        println!("Hit:1 {msg}");
    }
}

pub fn get(msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    if on() {
        println_line(format!(
            "{} {msg}",
            styled("Get:1", style(AnsiColor::Cyan, true))
        ));
    } else {
        println!("Get:1 {msg}");
    }
}

pub fn done(msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    if on() {
        println_line(format!(
            "{msg} {}",
            styled("Done", style(AnsiColor::Green, true))
        ));
    } else {
        println!("{msg} Done");
    }
}

pub fn plan_install(package: &str, version: &str, path: &str) {
    if on() {
        println_line(format!(
            "{} {} {} {}",
            styled("Inst", style(AnsiColor::Green, true)),
            styled(package, style(AnsiColor::Cyan, true)),
            dim(version),
            dim(&format!("[{path}]"))
        ));
    } else {
        println!("Inst {package} {version} [{path}]");
    }
}

pub fn plan_upgrade(package: &str, version: &str, path: &str) {
    if on() {
        println_line(format!(
            "{} {} {} {}",
            styled("Upgr", style(AnsiColor::Yellow, true)),
            styled(package, style(AnsiColor::Cyan, true)),
            dim(version),
            dim(&format!("[{path}]"))
        ));
    } else {
        println!("Upgr {package} {version} [{path}]");
    }
}

pub fn plan_remove(package: &str, version: &str) {
    if on() {
        println_line(format!(
            "{} {} {}",
            styled("Remv", style(AnsiColor::Red, true)),
            styled(package, style(AnsiColor::Cyan, true)),
            dim(version)
        ));
    } else {
        println!("Remv {package} {version}");
    }
}

pub fn setting_up(package: &str, version: &str) {
    if on() {
        println_line(format!(
            "Setting up {} ({}) ...",
            styled(package, style(AnsiColor::Green, true)),
            dim(version)
        ));
    } else {
        println!("Setting up {package} ({version}) ...");
    }
}

pub fn removing(package: &str, version: &str) {
    if on() {
        println_line(format!(
            "Removing {} ({}) ...",
            styled(package, style(AnsiColor::Red, true)),
            dim(version)
        ));
    } else {
        println!("Removing {package} ({version}) ...");
    }
}

pub fn search_result(package: &str, description: &str) {
    if on() {
        println_line(format!(
            "{} - {}",
            styled(package, style(AnsiColor::Cyan, true)),
            description
        ));
    } else {
        println!("{package} - {description}");
    }
}

pub fn info_field(label: &str, value: &str) {
    if on() {
        println_line(format!(
            "{}: {}",
            styled(label, style(AnsiColor::BrightWhite, true)),
            value
        ));
    } else {
        println!("{label}: {value}");
    }
}

pub fn installed_pkg(name: &str, version: &str, arch: &str, status: &str) {
    if on() {
        println_line(format!(
            "{}/{} {} {}",
            styled(name, style(AnsiColor::Cyan, true)),
            dim(version),
            dim(arch),
            dim(status)
        ));
    } else {
        println!("{name}/{version} {arch} {status}");
    }
}

pub fn confirm_read_line() -> io::Result<String> {
    let prompt = if on() {
        format!(
            "{} ",
            styled(
                "Do you want to continue? [Y/n]",
                style(AnsiColor::BrightWhite, true)
            )
        )
    } else {
        "Do you want to continue? [Y/n] ".to_string()
    };

    #[cfg(unix)]
    if let Ok(line) = read_line_from_tty(&prompt) {
        return Ok(line);
    }

    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line)
}

#[cfg(unix)]
fn read_line_from_tty(prompt: &str) -> io::Result<String> {
    use std::fs::OpenOptions;
    use std::io::{BufRead, Write};

    let mut tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
    write!(tty, "{prompt}")?;
    tty.flush()?;
    let mut line = String::new();
    std::io::BufReader::new(tty).read_line(&mut line)?;
    Ok(line)
}
