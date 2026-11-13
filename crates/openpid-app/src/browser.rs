//! Opening a web page in the system's browser, with the program the
//! platform has for it: no dependency is worth one link (ADR-0002).

use std::process::Command;

/// The program that opens an address on this platform, and what it is
/// given before the address.
fn opener() -> (&'static str, &'static [&'static str]) {
    if cfg!(windows) {
        // not `cmd /c start`: an `&` in an address would end its command
        ("rundll32", &["url.dll,FileProtocolHandler"])
    } else if cfg!(target_os = "macos") {
        ("open", &[])
    } else {
        ("xdg-open", &[])
    }
}

/// The command that opens `url`.
fn command(url: &str) -> Command {
    let (program, before) = opener();
    let mut command = Command::new(program);
    command.args(before).arg(url);
    command
}

/// Opens `url` in the browser. Only addresses of the web are opened; an
/// error means the platform's opener is missing or did not start.
pub fn open(url: &str) -> std::io::Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not an address of the web",
        ));
    }
    command(url).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_is_the_last_argument_of_the_platform_s_opener() {
        let url = "https://example.org/a?b=1&c=2";
        let command = command(url);
        let (program, before) = opener();
        assert_eq!(command.get_program(), program);
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args.len(), before.len() + 1);
        assert_eq!(args.last().copied(), Some(std::ffi::OsStr::new(url)));
        assert!(["rundll32", "open", "xdg-open"].contains(&program));
    }

    #[test]
    fn only_addresses_of_the_web_are_opened() {
        for other in ["file:///etc/passwd", "calc.exe", "", "javascript:alert(1)"] {
            let error = open(other).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{other}");
        }
    }
}
