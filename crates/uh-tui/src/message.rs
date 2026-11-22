//! A message as the user sends it: its text, and the images it carries
//! (the extension's `vscode/webview/attachments.ts`). An image comes from
//! a file whose path was pasted or dropped on the terminal, or from the
//! clipboard; it is told by its first bytes, not by its name. Nothing here
//! shrinks one: an image past what the agent takes is refused, with why.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The most images a message carries (the extension's).
pub const MOST: usize = 10;
/// The base64 payload the agent takes of an image while `images.autoResize`
/// is on (`crates/uh-daemon/src/acp/prompt.rs`).
const LARGEST: usize = 4_500_000;
/// The largest file read as an image, in bytes.
const READ_MOST: u64 = 64 * 1024 * 1024;
/// What an image the conversation gives back without its name is called.
pub const UNNAMED: &str = "image";

/// An image a message carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attached {
    pub name: String,
    pub mime: String,
    /// Its bytes, in base64.
    pub data: Arc<str>,
}

/// A message: what is written, and its images.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub images: Vec<Attached>,
}

impl Message {
    #[must_use]
    pub fn of(text: &str) -> Message {
        Message {
            text: text.to_string(),
            images: Vec::new(),
        }
    }
}

/// What kind of image `bytes` start as, of those the providers read: its
/// media type. `None` for anything else, whatever its file is called.
#[must_use]
pub fn kind(bytes: &[u8]) -> Option<&'static str> {
    let riff = bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP";
    match bytes {
        [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, ..] => Some("image/png"),
        [0xff, 0xd8, 0xff, ..] => Some("image/jpeg"),
        [b'G', b'I', b'F', b'8', b'7' | b'9', b'a', ..] => Some("image/gif"),
        _ if riff => Some("image/webp"),
        _ => None,
    }
}

/// `bytes` in base64, padded.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const SIXES: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut coded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let bits = (0..3).fold(0_u32, |bits, at| {
            (bits << 8) | u32::from(chunk.get(at).copied().unwrap_or(0))
        });
        for at in 0..4 {
            let six = (bits >> (18 - 6 * at)) & 63;
            coded.push(if at <= chunk.len() {
                char::from(SIXES[six as usize])
            } else {
                '='
            });
        }
    }
    coded
}

/// The image `bytes` are, called `name`, for a message to carry.
/// `shrinks`: the agent expects large images shrunk (`images.autoResize`),
/// and refuses one past its limit.
///
/// # Errors
///
/// A sentence to show: the bytes are no image the providers read, or the
/// image is too large.
pub fn attach(name: &str, bytes: &[u8], shrinks: bool) -> Result<Attached, String> {
    let Some(mime) = kind(bytes) else {
        return Err(format!("{name} is not a PNG, JPEG, GIF or WebP image."));
    };
    let data = base64(bytes);
    if shrinks && data.len() > LARGEST {
        let mega = bytes.len() as f64 / 1e6;
        return Err(format!(
            "{name} is too large to send ({mega:.1} MB): shrink it first."
        ));
    }
    Ok(Attached {
        name: name.to_string(),
        mime: mime.to_string(),
        data: data.into(),
    })
}

/// The image in the file at `path`.
///
/// # Errors
///
/// A sentence to show: the file cannot be read, or [`attach`] refused it.
pub fn read(path: &Path, shrinks: bool) -> Result<Attached, String> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let name = crate::text::clean(&name);
    // No image a provider reads is that large: it is not read to find out.
    if std::fs::metadata(path).is_ok_and(|file| file.len() > READ_MOST) {
        return Err(format!("{name} is too large to send."));
    }
    let bytes = std::fs::read(path).map_err(|_| format!("{name} could not be read."))?;
    attach(&name, &bytes, shrinks)
}

/// The image files a paste names, when it names nothing else: what a
/// terminal writes for files dropped on it, or copied in a file manager.
/// Each is a whole path (from `/`, from `~/` in `home`, or a `file://`
/// address) that ends as an image's name does; they stand apart by spaces
/// or line ends, a space of a path quoted or after a backslash. `None`
/// for any other text: it is written in the message.
#[must_use]
pub fn dropped(text: &str, home: Option<&Path>) -> Option<Vec<PathBuf>> {
    let words = words(text)?;
    if words.is_empty() {
        return None;
    }
    words.iter().map(|word| path(word, home)).collect()
}

/// The path a word of a paste is, when it is an image's.
fn path(word: &str, home: Option<&Path>) -> Option<PathBuf> {
    let path = if let Some(rest) = word.strip_prefix("file://") {
        // The address of a file of this machine: no host, or `localhost`.
        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
        PathBuf::from(unescaped(rest).filter(|path| path.starts_with('/'))?)
    } else if let Some(rest) = word.strip_prefix("~/") {
        home?.join(rest)
    } else if word.starts_with('/') {
        PathBuf::from(word)
    } else {
        return None;
    };
    let ends = path.extension()?.to_str()?.to_ascii_lowercase();
    matches!(ends.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp").then_some(path)
}

/// A `file://` address's path with its `%XX` bytes as they are; `None`
/// when they do not make a text.
fn unescaped(path: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(path.len());
    let mut rest = path.as_bytes();
    while let Some((&first, after)) = rest.split_first() {
        let coded = (first == b'%' && after.len() >= 2)
            .then(|| std::str::from_utf8(&after[..2]).ok())
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = coded {
            bytes.push(byte);
            rest = &after[2..];
        } else {
            bytes.push(first);
            rest = after;
        }
    }
    String::from_utf8(bytes).ok()
}

/// The words of a paste as a shell would read them: apart by spaces and
/// line ends, a quote keeping those in, a backslash keeping the next
/// character. `None` when a quote is left open.
fn words(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (Some(open), ch) if ch == open => quote = None,
            (Some(_), ch) => word.push(ch),
            (None, '\\') => {
                word.push(chars.next()?);
                started = true;
            }
            (None, '\'' | '"') => {
                quote = Some(ch);
                started = true;
            }
            (None, ch) if ch.is_whitespace() => {
                if std::mem::take(&mut started) {
                    words.push(std::mem::take(&mut word));
                }
            }
            (None, ch) => {
                word.push(ch);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return None;
    }
    if started {
        words.push(word);
    }
    Some(words)
}

/// The image a `«data PNGf…»` of `AppleScript` holds: its bytes, from the
/// hexadecimal after the four letters of its class.
#[must_use]
pub fn from_hex(said: &str) -> Option<Vec<u8>> {
    let hex = said.trim().strip_prefix("«data ")?.strip_suffix('»')?;
    let hex = hex.get(4..)?.as_bytes();
    if hex.is_empty() || hex.len() % 2 != 0 {
        return None;
    }
    hex.chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // How a paste could be read wrong:
    // - a sentence that ends on an image's name ("see shot.png"), or holds
    //   a path among other words, is taken for images and never written;
    // - a path with a space in it (quoted, or after a backslash) is cut in
    //   two, and neither half is a file;
    // - the space or line end a terminal adds after the path, or between
    //   two paths, makes an empty word that refuses the lot;
    // - `.PNG`, `.jpeg` are not known; `.txt`, `.png.txt`, no extension are;
    // - a `file://` address keeps its `%20`, or one of another machine is
    //   read as a path of this one; `~/` is not put in the home folder;
    // - a quote left open, or a backslash at the end, panics;
    // - nothing but spaces is "images", and empties the paste.
    #[test]
    fn a_paste_is_images_when_it_names_nothing_else() {
        let home = Path::new("/home/me");
        let paths = |text: &str| {
            dropped(text, Some(home)).map(|paths| {
                (paths.iter())
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
            })
        };
        let one = |path: &str| Some(vec![path.to_string()]);
        assert_eq!(paths("/tmp/shot.png"), one("/tmp/shot.png"));
        assert_eq!(paths("/tmp/shot.png \n"), one("/tmp/shot.png"));
        assert_eq!(paths("/tmp/my\\ shot.PNG"), one("/tmp/my shot.PNG"));
        assert_eq!(paths("'/tmp/my shot.jpeg'"), one("/tmp/my shot.jpeg"));
        assert_eq!(paths("\"/tmp/a b.gif\" "), one("/tmp/a b.gif"));
        assert_eq!(paths("~/Desktop/x.webp"), one("/home/me/Desktop/x.webp"));
        assert_eq!(paths("file:///tmp/my%20shot.jpg"), one("/tmp/my shot.jpg"));
        assert_eq!(paths("file://localhost/tmp/a.png"), one("/tmp/a.png"));
        assert_eq!(
            paths("/a/1.png /b/2.jpg\n'/c d/3.gif'"),
            Some(vec![
                "/a/1.png".to_string(),
                "/b/2.jpg".to_string(),
                "/c d/3.gif".to_string()
            ])
        );
        for text in [
            "see shot.png",
            "look at /tmp/shot.png",
            "/tmp/shot.png is wrong",
            "/tmp/notes.txt",
            "/tmp/shot.png.txt",
            "/tmp/shot",
            "shot.png",
            "./shot.png",
            "file://elsewhere/tmp/a.png",
            "'/tmp/open quote.png",
            "/tmp/shot.png\\",
            "   \n",
            "",
        ] {
            assert_eq!(paths(text), None, "{text:?}");
        }
        assert_eq!(dropped("~/x.png", None), None);
    }

    // How a file could be taken for the wrong thing: by its name (a text
    // called `.png`), by too few of its first bytes (anything that starts
    // with `GIF`, any RIFF file: a sound, a video), or a short file panics.
    #[test]
    fn an_image_is_told_by_its_first_bytes() {
        assert_eq!(kind(b"\x89PNG\r\n\x1a\n\0\0"), Some("image/png"));
        assert_eq!(kind(b"\xff\xd8\xff\xe0"), Some("image/jpeg"));
        assert_eq!(kind(b"GIF89a.."), Some("image/gif"));
        assert_eq!(kind(b"GIF87a"), Some("image/gif"));
        assert_eq!(kind(b"RIFF\x10\0\0\0WEBPVP8 "), Some("image/webp"));
        for bytes in [
            &b"RIFF\x10\0\0\0WAVEfmt "[..],
            b"GIFT card",
            b"\x89PNG",
            b"RIFF",
            b"hello",
            b"",
        ] {
            assert_eq!(kind(bytes), None, "{bytes:?}");
        }
        let refused = attach("notes.png", b"hello", true);
        assert_eq!(
            refused,
            Err("notes.png is not a PNG, JPEG, GIF or WebP image.".to_string())
        );
    }

    // How the bytes could be coded wrong: the last group of one or two
    // bytes unpadded, or padded with an `A`; nothing coded as `====`.
    #[test]
    fn bytes_are_coded_in_padded_base64() {
        for (bytes, coded) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"\xff\xfe\xfd", "//79"),
        ] {
            assert_eq!(base64(bytes), coded);
        }
    }

    // How the clipboard's image could be read wrong: the class's four
    // letters taken for bytes; an odd count of digits; what is no data at
    // all (an error's words) read as an empty image.
    #[test]
    fn the_clipboard_image_is_read_from_its_hexadecimal() {
        assert_eq!(
            from_hex("«data PNGf89504E47»\n"),
            Some(vec![0x89, 0x50, 0x4e, 0x47])
        );
        assert_eq!(from_hex("«data PNGf»"), None);
        assert_eq!(from_hex("«data PNGf895»"), None);
        assert_eq!(from_hex("«data PNGfzz»"), None);
        assert_eq!(from_hex("execution error: no image"), None);
        assert_eq!(from_hex(""), None);
    }

    // A large image under `images.autoResize` is refused here, as the
    // agent would refuse it; without the setting it goes as it is.
    #[test]
    fn an_image_past_the_agents_limit_is_refused_while_it_expects_them_shrunk() {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.resize(3_400_000, 0);
        let refused = attach("big.png", &bytes, true).unwrap_err();
        assert!(
            refused.starts_with("big.png is too large to send (3.4 MB)"),
            "{refused}"
        );
        assert!(attach("big.png", &bytes, false).is_ok());
        bytes.truncate(3_000_000);
        assert!(attach("ok.png", &bytes, true).is_ok());
    }
}
