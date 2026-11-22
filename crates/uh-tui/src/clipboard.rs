//! The image on the user's clipboard, for a message to carry: a terminal
//! pastes text only, so the system's own tool is asked for it
//! (`osascript` on macOS, `wl-paste` or `xclip` elsewhere).

use std::process::{Command, Stdio};

use crate::message::from_hex;

/// What a tool wrote, when it ran and did not fail; `None` when it is not
/// there to run.
fn said(program: &str, args: &[&str]) -> Option<Result<Vec<u8>, ()>> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(())
    })
}

/// The bytes of the image on the clipboard.
///
/// # Errors
///
/// A sentence to show: the clipboard holds no image, or nothing here can read it.
pub fn image() -> Result<Vec<u8>, String> {
    let none = || "The clipboard holds no image.".to_string();
    if cfg!(target_os = "macos") {
        // AppleScript writes the data in hexadecimal: `«data PNGf89504E47…»`.
        for class in ["PNGf", "JPEG"] {
            let script = format!("get the clipboard as «class {class}»");
            let Some(written) = said("osascript", &["-e", &script]) else {
                return Err("The clipboard could not be read: osascript is not there.".to_string());
            };
            let bytes = written
                .ok()
                .and_then(|written| from_hex(&String::from_utf8_lossy(&written)));
            if let Some(bytes) = bytes {
                return Ok(bytes);
            }
        }
        return Err(none());
    }
    let tools: [(&str, &[&str]); 2] = [
        ("wl-paste", &["--type", "image/png"]),
        (
            "xclip",
            &["-selection", "clipboard", "-t", "image/png", "-o"],
        ),
    ];
    let mut ran = false;
    for (program, args) in tools {
        match said(program, args) {
            Some(Ok(bytes)) if !bytes.is_empty() => return Ok(bytes),
            Some(_) => ran = true,
            None => {}
        }
    }
    if ran {
        Err(none())
    } else {
        Err("To paste an image, install wl-clipboard or xclip.".to_string())
    }
}
