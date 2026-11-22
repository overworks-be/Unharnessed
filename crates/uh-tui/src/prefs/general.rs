//! The settings of the General tab, as the extension lays them out
//! (`vscode/webview/Settings.tsx`): compaction, retries, switching
//! providers, usage limits, images; and how a number is shown and typed in
//! its unit.

/// How a number is shown and typed; the file holds counts or milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Count,
    Seconds,
    Hours,
}

impl Unit {
    /// What follows the number.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Unit::Count => "",
            Unit::Seconds => "s",
            Unit::Hours => "h",
        }
    }

    fn scale(self) -> f64 {
        match self {
            Unit::Count => 1.0,
            Unit::Seconds => 1_000.0,
            Unit::Hours => 3_600_000.0,
        }
    }
}

/// What sets a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Switch,
    /// A number in its unit, and the most the file takes.
    Number(Unit, u64),
    /// A model, or `none`; `images`: among those that read images.
    Model {
        none: &'static str,
        images: bool,
    },
}

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub control: Control,
}

pub struct Group {
    pub label: &'static str,
    pub help: &'static str,
    /// The switch whose off state makes the other fields moot.
    pub gate: Option<&'static str>,
    pub fields: &'static [Field],
}

const fn switch(key: &'static str, label: &'static str, help: &'static str) -> Field {
    Field {
        key,
        label,
        help,
        control: Control::Switch,
    }
}

const fn number(
    key: &'static str,
    label: &'static str,
    help: &'static str,
    unit: Unit,
    max: u64,
) -> Field {
    Field {
        key,
        label,
        help,
        control: Control::Number(unit, max),
    }
}

const HOUR: u64 = 3_600_000;

pub const GROUPS: [Group; 5] = [
    Group {
        label: "Compaction",
        help: "Summarizes older messages when the context fills up.",
        gate: Some("compaction.enabled"),
        fields: &[
            switch("compaction.enabled", "Compact automatically", "Before the context is full."),
            Field {
                key: "auxiliaryModel",
                label: "Summary model",
                help: "Writes the summaries.",
                control: Control::Model {
                    none: "Same as the chat model",
                    images: false,
                },
            },
        ],
    },
    Group {
        label: "Retries",
        help: "When a request to the provider fails, it is sent again after a wait that doubles each time.",
        gate: Some("retry.enabled"),
        fields: &[
            switch("retry.enabled", "Retry failed requests", ""),
            number("retry.maxRetries", "Retries", "", Unit::Count, 100),
            number("retry.baseDelayMs", "First wait", "", Unit::Seconds, HOUR),
            number(
                "retry.provider.maxRetryDelayMs",
                "Longest wait",
                "When the provider asks to wait longer, the request fails.",
                Unit::Seconds,
                24 * HOUR,
            ),
        ],
    },
    Group {
        label: "Switching providers",
        help: "When another connected provider serves the same model, a request that keeps failing moves to it. These retries then replace the ones above.",
        gate: Some("retry.failover.enabled"),
        fields: &[
            switch("retry.failover.enabled", "Switch providers", ""),
            number("retry.failover.maxRetries", "Retries per provider", "", Unit::Count, 100),
            number("retry.failover.baseDelayMs", "First wait", "", Unit::Seconds, HOUR),
            number("retry.failover.maxDelayMs", "Longest wait", "", Unit::Seconds, HOUR),
        ],
    },
    Group {
        label: "Usage limits",
        help: "When a plan's usage limit is reached.",
        gate: Some("retry.provider.waitForUsage.pauseUntilReset"),
        fields: &[
            switch(
                "retry.provider.waitForUsage.pauseUntilReset",
                "Wait for the reset",
                "Pause, then resume on its own once the limit resets.",
            ),
            number(
                "retry.provider.waitForUsage.maxPauseMs",
                "Longest pause",
                "Up to 168 hours, a week.",
                Unit::Hours,
                7 * 24 * HOUR,
            ),
            number(
                "retry.provider.waitForUsage.maxParks",
                "Pauses per limit",
                "",
                Unit::Count,
                100,
            ),
        ],
    },
    Group {
        label: "Images",
        help: "",
        gate: None,
        fields: &[
            Field {
                key: "imageModel",
                label: "Image model",
                help: "Reads the images when the chat model can't. Without one, such messages are refused.",
                control: Control::Model {
                    none: "None",
                    images: true,
                },
            },
            switch(
                "images.autoResize",
                "Shrink large images",
                "Large images are scaled down before they are sent (in the terminal: refused).",
            ),
            switch("images.blockImages", "Block images", "Images are never sent to a provider."),
        ],
    },
];

/// The field of that key.
#[must_use]
pub fn field(key: &str) -> Option<&'static Field> {
    GROUPS
        .iter()
        .flat_map(|group| group.fields)
        .find(|field| field.key == key)
}

/// A stored number in its unit: up to three decimals, no zeros at its end.
#[must_use]
pub fn shown(stored: u64, unit: Unit) -> String {
    let text = format!("{:.3}", stored as f64 / unit.scale());
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// What `typed` stores, in a unit, the file taking `max` at the most:
/// nothing written is `None` (the default applies).
///
/// # Errors
///
/// What is written is no number the setting takes: what it takes.
pub fn stored(typed: &str, unit: Unit, max: u64) -> Result<Option<u64>, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Ok(None);
    }
    let most = shown(max, unit);
    let refusal = || match unit {
        Unit::Count => format!("A whole number from 0 to {most}."),
        Unit::Seconds => format!("A number of seconds from 0 to {most}."),
        Unit::Hours => format!("A number of hours from 0 to {most}."),
    };
    // Digits and one point: no sign, no exponent, no `inf`.
    let plain = typed
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
        && typed.bytes().filter(|byte| *byte == b'.').count() <= 1
        && typed.bytes().any(|byte| byte.is_ascii_digit());
    let number: f64 = (plain.then(|| typed.parse().ok()).flatten()).ok_or_else(refusal)?;
    if unit == Unit::Count && number.fract() != 0.0 {
        return Err(refusal());
    }
    let stored = (number * unit.scale()).round();
    if !stored.is_finite() || stored > max as f64 {
        return Err(refusal());
    }
    Ok(Some(stored as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    // How a number could be stored wrong:
    // - seconds written to the file as they are typed, a thousand times
    //   too short; hours likewise;
    // - `1.5` retries stored as 1, or 2; `-1`, `1e3`, `inf`, `NaN`, `0x10`,
    //   `1,5`, words taken for a number;
    // - a number past what the file takes sent to be refused later (or,
    //   huge, wrapped into a small one);
    // - nothing written stored as 0 instead of putting the default back;
    // - 0 refused though the file takes it; spaces around it refused;
    // - a stored 2500 ms shown as `2.500`, 3 as `3.000`, 1 ms as `0`.
    #[test]
    fn a_number_is_stored_in_what_the_file_counts() {
        let hour = 3_600_000;
        for (typed, unit, max, stored_as) in [
            ("5", Unit::Count, 100, Ok(Some(5))),
            (" 0 ", Unit::Count, 100, Ok(Some(0))),
            ("100", Unit::Count, 100, Ok(Some(100))),
            ("", Unit::Count, 100, Ok(None)),
            ("   ", Unit::Seconds, hour, Ok(None)),
            ("2", Unit::Seconds, hour, Ok(Some(2_000))),
            ("2.5", Unit::Seconds, hour, Ok(Some(2_500))),
            (".5", Unit::Seconds, hour, Ok(Some(500))),
            ("3600", Unit::Seconds, hour, Ok(Some(hour))),
            ("24", Unit::Hours, 168 * hour, Ok(Some(24 * hour))),
            ("0.5", Unit::Hours, 168 * hour, Ok(Some(hour / 2))),
            ("168", Unit::Hours, 168 * hour, Ok(Some(168 * hour))),
        ] {
            assert_eq!(stored(typed, unit, max), stored_as, "{typed:?}");
        }
        for (typed, unit, max) in [
            ("1.5", Unit::Count, 100),
            ("101", Unit::Count, 100),
            ("-1", Unit::Count, 100),
            ("1e3", Unit::Seconds, hour),
            ("inf", Unit::Seconds, hour),
            ("NaN", Unit::Seconds, hour),
            ("0x10", Unit::Count, 100),
            ("1,5", Unit::Seconds, hour),
            ("1.2.3", Unit::Seconds, hour),
            (".", Unit::Seconds, hour),
            ("soon", Unit::Seconds, hour),
            ("3601", Unit::Seconds, hour),
            ("169", Unit::Hours, 168 * hour),
            ("99999999999999999999999999", Unit::Count, 100),
        ] {
            assert!(stored(typed, unit, max).is_err(), "{typed:?}");
        }
        assert_eq!(
            stored("1.5", Unit::Count, 100),
            Err("A whole number from 0 to 100.".to_string())
        );
        assert_eq!(
            stored("9999", Unit::Hours, 168 * hour),
            Err("A number of hours from 0 to 168.".to_string())
        );
        assert_eq!(shown(2_500, Unit::Seconds), "2.5");
        assert_eq!(shown(3, Unit::Count), "3");
        assert_eq!(shown(60_000, Unit::Seconds), "60");
        assert_eq!(shown(1, Unit::Seconds), "0.001");
        assert_eq!(shown(86_400_000, Unit::Hours), "24");
        assert_eq!(shown(0, Unit::Seconds), "0");
    }

    // Every setting of the tab is one the agent knows under that key, once.
    #[test]
    fn each_field_has_its_own_key() {
        let keys: Vec<&str> = (GROUPS.iter().flat_map(|group| group.fields))
            .map(|field| field.key)
            .collect();
        let mut unique = keys.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), keys.len());
        for group in &GROUPS {
            let gate = group.gate.map(|gate| group.fields[0].key == gate);
            assert_ne!(gate, Some(false), "{}", group.label);
        }
    }
}
