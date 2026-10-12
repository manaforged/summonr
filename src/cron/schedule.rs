use jiff::civil::{Date, DateTime};
use jiff::tz::{AmbiguousOffset, AmbiguousZoned, TimeZone};
use jiff::{SignedDuration, Timestamp, Zoned};

use crate::error::{Error, Result};

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const WEEKDAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const MACROS: [(&str, &str); 6] = [
    ("@hourly", "0 * * * *"),
    ("@daily", "0 0 * * *"),
    ("@weekly", "0 0 * * 0"),
    ("@monthly", "0 0 1 * *"),
    ("@yearly", "0 0 1 1 *"),
    ("@annually", "0 0 1 1 *"),
];
const UNITS: [(char, u64); 3] = [('m', 60), ('h', 3600), ('d', 86_400)];
const SEARCH_DAYS: usize = 366 * 28;
const EVERY_STEPS: usize = 1441;
const FOLD_SECS: i64 = 7200;
const EPOCH: DateTime = DateTime::constant(1970, 1, 1, 0, 0, 0, 0);
const EVERY: &str = "every ";
const AT: &str = "at ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fields {
    minute: u64,
    hour: u64,
    dom: u64,
    month: u64,
    dow: u64,
    dom_any: bool,
    dow_any: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Schedule {
    Cron(Fields),
    Every(u64),
    At(Timestamp),
}

struct Range {
    min: u32,
    max: u32,
    names: &'static [&'static str],
    names_from: u32,
}

const MINUTE: Range = Range {
    min: 0,
    max: 59,
    names: &[],
    names_from: 0,
};
const HOUR: Range = Range {
    min: 0,
    max: 23,
    names: &[],
    names_from: 0,
};
const DOM: Range = Range {
    min: 1,
    max: 31,
    names: &[],
    names_from: 0,
};
const MONTH: Range = Range {
    min: 1,
    max: 12,
    names: &MONTHS,
    names_from: 1,
};
const DOW: Range = Range {
    min: 0,
    max: 7,
    names: &WEEKDAYS,
    names_from: 0,
};

pub(crate) fn parse(text: &str) -> Result<Schedule> {
    let text = text.trim().to_ascii_lowercase();
    if let Some(rest) = text.strip_prefix(EVERY) {
        return every(rest.trim()).map(Schedule::Every);
    }
    if let Some(rest) = text.strip_prefix(AT) {
        return rest
            .trim()
            .parse::<Timestamp>()
            .map(Schedule::At)
            .map_err(|error| bad(&text, &error.to_string()));
    }
    let expr = MACROS
        .iter()
        .find(|(name, _)| *name == text)
        .map_or(text.as_str(), |(_, expr)| expr);
    let parts: Vec<&str> = expr.split_whitespace().collect();
    let [minute, hour, dom, month, dow] = parts[..] else {
        return Err(bad(
            &text,
            "expected five fields: minute hour day month weekday",
        ));
    };
    let mut dow_bits = field(dow, &DOW)?;
    if dow_bits & (1 << 7) != 0 {
        dow_bits = (dow_bits & !(1 << 7)) | 1;
    }
    Ok(Schedule::Cron(Fields {
        minute: field(minute, &MINUTE)?,
        hour: field(hour, &HOUR)?,
        dom: field(dom, &DOM)?,
        month: field(month, &MONTH)?,
        dow: dow_bits,
        dom_any: dom.starts_with('*'),
        dow_any: dow.starts_with('*'),
    }))
}

pub(crate) fn every_text(value: &str) -> Result<String> {
    every(value.trim())?;
    Ok(format!("{EVERY}{}", value.trim()))
}

pub(crate) fn at_text(value: &str, tz: &TimeZone) -> Result<String> {
    let value = value.trim();
    let stamp = match value.parse::<Timestamp>() {
        Ok(stamp) => stamp,
        Err(_) => DateTime::strptime("%Y-%m-%d %H:%M", value)
            .and_then(|civil| civil.to_zoned(tz.clone()))
            .map(|zoned| zoned.timestamp())
            .map_err(|error| bad(value, &error.to_string()))?,
    };
    Ok(format!("{AT}{stamp}"))
}

pub(crate) fn zone(name: Option<&str>) -> Result<TimeZone> {
    match name {
        Some(name) => TimeZone::get(name)
            .map_err(|error| Error::Config(format!("unknown time zone `{name}`: {error}"))),
        None => Ok(TimeZone::system()),
    }
}

fn every(text: &str) -> Result<u64> {
    let unit = text.chars().last().unwrap_or(' ');
    let scale = UNITS
        .iter()
        .find(|(name, _)| *name == unit)
        .map(|(_, secs)| *secs)
        .ok_or_else(|| bad(text, "use a number with m, h or d, like 30m"))?;
    let count: u64 = text[..text.len() - 1].parse().map_err(|error| {
        bad(
            text,
            &format!("use a number with m, h or d, like 30m ({error})"),
        )
    })?;
    match count.checked_mul(scale) {
        Some(secs) if secs > 0 => Ok(secs),
        Some(_) | None => Err(bad(text, "the interval must be above zero")),
    }
}

fn field(text: &str, range: &Range) -> Result<u64> {
    text.split(',')
        .try_fold(0_u64, |bits, part| Ok(bits | part_bits(part, range)?))
}

fn part_bits(part: &str, range: &Range) -> Result<u64> {
    let (base, step) = match part.split_once('/') {
        Some((base, step)) => (base, step_of(part, step)?),
        None => (part, 1),
    };
    let (low, high) = bounds(base, step, range)?;
    if low > high {
        return Err(bad(part, "the range runs backward"));
    }
    Ok((low..=high)
        .step_by(usize::try_from(step).unwrap_or(usize::MAX))
        .fold(0, |bits, at| bits | (1 << at)))
}

fn step_of(part: &str, step: &str) -> Result<u32> {
    step.parse()
        .ok()
        .filter(|step| *step > 0)
        .ok_or_else(|| bad(part, "the step must be a number above zero"))
}

fn bounds(base: &str, step: u32, range: &Range) -> Result<(u32, u32)> {
    if base == "*" {
        return Ok((range.min, range.max));
    }
    match base.split_once('-') {
        Some((low, high)) => Ok((value(low, range)?, value(high, range)?)),
        None => {
            let low = value(base, range)?;
            Ok((low, if step > 1 { range.max } else { low }))
        }
    }
}

fn value(text: &str, range: &Range) -> Result<u32> {
    let found = range
        .names
        .iter()
        .position(|name| *name == text)
        .and_then(|index| u32::try_from(index).ok())
        .map(|index| index + range.names_from);
    match found.or_else(|| text.parse().ok()) {
        Some(at) if (range.min..=range.max).contains(&at) => Ok(at),
        Some(_) | None => Err(bad(
            text,
            &format!("expected {}-{} or a name", range.min, range.max),
        )),
    }
}

fn bad(text: &str, why: &str) -> Error {
    Error::Config(format!("bad schedule `{text}`: {why}"))
}

fn has(bits: u64, at: i8) -> bool {
    u32::try_from(at).is_ok_and(|at| at < 64 && bits & (1 << at) != 0)
}

impl Fields {
    fn day(&self, date: Date) -> bool {
        if !has(self.month, date.month()) {
            return false;
        }
        let dom = has(self.dom, date.day());
        let dow = has(self.dow, date.weekday().to_sunday_zero_offset());
        if self.dom_any || self.dow_any {
            dom && dow
        } else {
            dom || dow
        }
    }

    fn next_after(&self, after: &Zoned) -> Option<Zoned> {
        let mut date = after.date();
        for _ in 0..SEARCH_DAYS {
            if let Some(found) = self.day(date).then(|| self.on_day(date, after)).flatten() {
                return Some(found);
            }
            date = date.tomorrow().ok()?;
        }
        None
    }

    fn on_day(&self, date: Date, after: &Zoned) -> Option<Zoned> {
        let tz = after.time_zone();
        let floor = after
            .datetime()
            .checked_sub(SignedDuration::from_secs(FOLD_SECS))
            .ok()?;
        let mut best: Option<Zoned> = None;
        for civil in self.slots(date).filter(|civil| *civil > floor) {
            let Some((zoned, first)) = first_after(tz.to_ambiguous_zoned(civil), after) else {
                continue;
            };
            best = Some(match best {
                Some(seen) if seen.timestamp() <= zoned.timestamp() => seen,
                Some(_) | None => zoned,
            });
            if first {
                break;
            }
        }
        best
    }

    fn slots(&self, date: Date) -> impl Iterator<Item = DateTime> + '_ {
        (0..24_i8)
            .filter(|hour| has(self.hour, *hour))
            .flat_map(move |hour| {
                (0..60_i8)
                    .filter(|minute| has(self.minute, *minute))
                    .map(move |minute| date.at(hour, minute, 0, 0))
            })
    }
}

fn first_after(ambiguous: AmbiguousZoned, after: &Zoned) -> Option<(Zoned, bool)> {
    let offset = ambiguous.offset();
    if matches!(offset, AmbiguousOffset::Gap { .. }) {
        return None;
    }
    let earlier = ambiguous.clone().earlier().ok()?;
    if earlier.timestamp() > after.timestamp() {
        return Some((earlier, true));
    }
    if !matches!(offset, AmbiguousOffset::Fold { .. }) || after.datetime() >= ambiguous.datetime() {
        return None;
    }
    ambiguous
        .later()
        .ok()
        .filter(|later| later.timestamp() > after.timestamp())
        .map(|later| (later, false))
}

fn resolve(ambiguous: AmbiguousZoned) -> Option<Zoned> {
    match ambiguous.offset() {
        AmbiguousOffset::Gap { .. } => ambiguous.later().ok(),
        AmbiguousOffset::Fold { .. } | AmbiguousOffset::Unambiguous { .. } => {
            ambiguous.earlier().ok()
        }
    }
}

fn every_after(secs: u64, after: &Zoned) -> Option<Zoned> {
    let secs = i64::try_from(secs).ok()?;
    let civil = after.datetime().duration_since(EPOCH).as_secs();
    let mut slot = civil.div_euclid(secs).checked_add(1)?;
    for _ in 0..EVERY_STEPS {
        let at = EPOCH
            .checked_add(SignedDuration::from_secs(slot.checked_mul(secs)?))
            .ok()?;
        let zoned = resolve(after.time_zone().to_ambiguous_zoned(at))?;
        if zoned.timestamp() > after.timestamp() {
            return Some(zoned);
        }
        slot = slot.checked_add(1)?;
    }
    None
}

impl Schedule {
    pub(crate) fn next_after(&self, after: Zoned) -> Option<Zoned> {
        match self {
            Self::Cron(fields) => fields.next_after(&after),
            Self::Every(secs) => every_after(*secs, &after),
            Self::At(stamp) => {
                (*stamp > after.timestamp()).then(|| stamp.to_zoned(after.time_zone().clone()))
            }
        }
    }
}
