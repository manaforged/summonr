use serde_json::Value;

use super::{Field, FieldKind, TextFormat};

impl Field {
    pub fn check(&self, value: &Value) -> Result<(), String> {
        let title = &self.title;
        match &self.kind {
            FieldKind::Text {
                format,
                min_length,
                max_length,
                pattern,
            } => {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("{title} takes text"))?;
                let length = text.chars().count();
                if let Some(min) = min_length.filter(|min| length < *min) {
                    return Err(format!("{title} is shorter than {min} characters"));
                }
                if let Some(max) = max_length.filter(|max| length > *max) {
                    return Err(format!("{title} is longer than {max} characters"));
                }
                if let Some(pattern) = pattern
                    && regex::Regex::new(pattern).is_ok_and(|pattern| !pattern.is_match(text))
                {
                    return Err(format!("{title} does not match {pattern}"));
                }
                match format {
                    Some(format) if !format.accepts(text) => {
                        Err(format!("{title} is not a valid {}", format.label()))
                    }
                    Some(_) | None => Ok(()),
                }
            }
            FieldKind::Number {
                integer,
                minimum,
                maximum,
            } => {
                let number = value
                    .as_f64()
                    .ok_or_else(|| format!("{title} takes a number"))?;
                if *integer && number.fract() != 0.0 {
                    return Err(format!("{title} takes a whole number"));
                }
                if minimum.is_some_and(|min| number < min)
                    || maximum.is_some_and(|max| number > max)
                {
                    return Err(format!("{title} is out of range"));
                }
                Ok(())
            }
            FieldKind::Boolean => value
                .is_boolean()
                .then_some(())
                .ok_or_else(|| format!("{title} takes yes or no")),
            FieldKind::Choice { options } => {
                let text = value.as_str().unwrap_or_default();
                options
                    .iter()
                    .any(|option| option.value == text)
                    .then_some(())
                    .ok_or_else(|| format!("{title} is not one of the choices"))
            }
            FieldKind::Choices {
                options,
                min_items,
                max_items,
            } => {
                let items = value
                    .as_array()
                    .ok_or_else(|| format!("{title} takes a list"))?;
                if min_items.is_some_and(|min| items.len() < min)
                    || max_items.is_some_and(|max| items.len() > max)
                {
                    return Err(format!("{title} has the wrong number of choices"));
                }
                items
                    .iter()
                    .all(|item| {
                        options
                            .iter()
                            .any(|option| item.as_str() == Some(option.value.as_str()))
                    })
                    .then_some(())
                    .ok_or_else(|| format!("{title} has a choice that is not offered"))
            }
        }
    }
}

impl TextFormat {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Email => "email address",
            Self::Uri => "URI",
            Self::Date => "date (YYYY-MM-DD)",
            Self::DateTime => "date and time (RFC 3339)",
        }
    }

    fn accepts(self, text: &str) -> bool {
        match self {
            Self::Email => text
                .split_once('@')
                .is_some_and(|(user, host)| !user.is_empty() && host.contains('.')),
            Self::Uri => url::Url::parse(text).is_ok(),
            Self::Date => date(text),
            Self::DateTime => text
                .split_once(['T', 't'])
                .is_some_and(|(day, time)| date(day) && clock(time)),
        }
    }
}

fn digits(text: &str, width: usize) -> Option<u32> {
    (text.len() == width && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn date(text: &str) -> bool {
    let mut parts = text.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let (Some(year), Some(month), Some(day)) = (digits(year, 4), digits(month, 2), digits(day, 2))
    else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let last = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1..=12 => 31,
        _other => return false,
    };
    (1..=last).contains(&day)
}

fn clock(text: &str) -> bool {
    let (time, zone) = match text.find(['Z', 'z', '+', '-']) {
        Some(at) => text.split_at(at),
        None => return false,
    };
    let zone_ok = matches!(zone, "Z" | "z")
        || zone.get(1..).is_some_and(|offset| {
            offset.split_once(':').is_some_and(|(hours, minutes)| {
                digits(hours, 2).is_some_and(|hours| hours < 24)
                    && digits(minutes, 2).is_some_and(|minutes| minutes < 60)
            })
        });
    let whole = time.split_once('.').map_or(time, |(whole, fraction)| {
        if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            ""
        } else {
            whole
        }
    });
    let mut parts = whole.split(':');
    let valid = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(hours), Some(minutes), Some(seconds), None) => {
            digits(hours, 2).is_some_and(|hours| hours < 24)
                && digits(minutes, 2).is_some_and(|minutes| minutes < 60)
                && digits(seconds, 2).is_some_and(|seconds| seconds <= 60)
        }
        _other => false,
    };
    valid && zone_ok
}
