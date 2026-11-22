//! A conversation recorded over ACP, in the format of the end-to-end traces
//! (`scripts/unharnessed-tui-record.py`): a first line with the command,
//! then one line per frame, `<seconds> <arrow> <json>`, `>` for what the
//! client sent and `<` for what the agent answered.

use serde_json::Value;

/// One frame of the recording.
#[derive(Clone, Debug)]
pub struct Frame {
    /// Seconds since the recording started.
    pub at: f64,
    /// The client sent it; otherwise the agent did.
    pub sent: bool,
    pub json: Value,
}

/// The frames of a recording, in order.
///
/// # Errors
///
/// A line that is not `<seconds> <arrow> <json>`, with the line's number; a
/// recording without any frame.
pub fn parse(text: &str) -> Result<Vec<Frame>, String> {
    let mut frames = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with('$') {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let (Some(at), Some(arrow), Some(json)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(format!(
                "line {number}: expected `<seconds> <arrow> <json>`"
            ));
        };
        let at: f64 = at
            .parse()
            .map_err(|_| format!("line {number}: `{at}` is not a number of seconds"))?;
        let sent = match arrow {
            ">" => true,
            "<" => false,
            _ => return Err(format!("line {number}: `{arrow}` is neither `>` nor `<`")),
        };
        let json = serde_json::from_str(json).map_err(|error| format!("line {number}: {error}"))?;
        frames.push(Frame { at, sent, json });
    }
    if frames.is_empty() {
        return Err("no frame in the recording".to_string());
    }
    Ok(frames)
}
