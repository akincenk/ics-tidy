//! VEVENT fields: date/time values (RFC 5545 3.3.4, 3.3.5), durations
//! (3.3.6) and the event record used for expansion.

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};
use chrono_tz::Tz;

use crate::parse::{Component, Property};

/// A DATE or DATE-TIME value in the three forms RFC 5545 allows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Time {
    /// `VALUE=DATE`, an all-day value.
    Date(NaiveDate),
    /// Local time without a zone: the same wall-clock time everywhere.
    Floating(NaiveDateTime),
    /// UTC (`Z` suffix).
    Utc(NaiveDateTime),
    /// Wall-clock time in an IANA zone (`TZID=`).
    Zoned(NaiveDateTime, Tz),
}

impl Time {
    pub fn is_date(&self) -> bool {
        matches!(self, Time::Date(_))
    }
}

/// Parse one DATE or DATE-TIME string. `tzid` is the TZID parameter, if any.
/// Returns the value and, when the zone is unknown, a warning (the value is
/// then read as floating time).
pub fn parse_time(
    value: &str,
    tzid: Option<&str>,
    value_type: Option<&str>,
) -> Result<(Time, Option<String>), String> {
    let value = value.trim();
    let is_date = value_type.is_some_and(|v| v.eq_ignore_ascii_case("DATE")) || value.len() == 8;
    if is_date {
        let d = NaiveDate::parse_from_str(value, "%Y%m%d")
            .map_err(|_| format!("invalid date {value:?}"))?;
        return Ok((Time::Date(d), None));
    }
    let (body, utc) = match value.strip_suffix(['Z', 'z']) {
        Some(b) => (b, true),
        None => (value, false),
    };
    let dt = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S")
        .map_err(|_| format!("invalid date-time {value:?}"))?;
    if utc {
        return Ok((Time::Utc(dt), None));
    }
    match tzid {
        None => Ok((Time::Floating(dt), None)),
        Some(id) => match lookup_zone(id) {
            Some(tz) => Ok((Time::Zoned(dt, tz), None)),
            None => Ok((
                Time::Floating(dt),
                Some(format!("unknown time zone {id:?}, read as local time")),
            )),
        },
    }
}

/// Find an IANA zone by name. Some producers write a leading slash
/// (`/Europe/Berlin`), which is accepted.
pub fn lookup_zone(id: &str) -> Option<Tz> {
    id.trim().trim_start_matches('/').parse::<Tz>().ok()
}

/// Parse a DURATION value such as `PT1H30M`, `P1D` or `-P2W`.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let bad = || format!("invalid duration {s:?}");
    let s = s.trim();
    let (neg, rest) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let rest = rest.strip_prefix(['P', 'p']).ok_or_else(bad)?;
    let mut total = Duration::zero();
    let mut num = String::new();
    let mut in_time = false;
    let mut any = false;
    for c in rest.chars() {
        match c.to_ascii_uppercase() {
            '0'..='9' => num.push(c),
            'T' if num.is_empty() && !in_time => in_time = true,
            unit => {
                let n: i64 = num.parse().map_err(|_| bad())?;
                num.clear();
                any = true;
                total += match (unit, in_time) {
                    ('W', false) => Duration::weeks(n),
                    ('D', false) => Duration::days(n),
                    ('H', true) => Duration::hours(n),
                    ('M', true) => Duration::minutes(n),
                    ('S', true) => Duration::seconds(n),
                    _ => return Err(bad()),
                };
            }
        }
    }
    if !num.is_empty() || !any {
        return Err(bad());
    }
    Ok(if neg { -total } else { total })
}

/// The parts of a VEVENT that matter for an agenda.
#[derive(Debug, Clone)]
pub struct Event {
    pub uid: Option<String>,
    pub summary: String,
    pub location: String,
    pub start: Time,
    /// DTEND, if given.
    pub end: Option<Time>,
    /// DURATION, if given (and no DTEND).
    pub duration: Option<Duration>,
    pub rrules: Vec<String>,
    pub rdates: Vec<Time>,
    pub exdates: Vec<Time>,
    pub recurrence_id: Option<Time>,
    pub sequence: i64,
    pub cancelled: bool,
    /// LAST-MODIFIED, else DTSTAMP, as written (UTC, compares as text).
    pub modified: String,
    /// Calendar name shown in the agenda (file name without extension).
    pub calendar: String,
}

/// Read one VEVENT. Returns the event plus warnings about values that were
/// read in a forgiving way. A VEVENT without a usable DTSTART is an error.
pub fn event_from(c: &Component, calendar: &str) -> Result<(Event, Vec<String>), String> {
    let mut warnings = Vec::new();
    let mut time_of = |p: &Property| -> Result<Time, String> {
        let (t, w) = parse_time(&p.value, p.param("TZID"), p.param("VALUE"))?;
        if let Some(w) = w
            && !warnings.contains(&w)
        {
            warnings.push(w);
        }
        Ok(t)
    };

    let start = time_of(c.prop("DTSTART").ok_or("VEVENT without DTSTART")?)?;
    let end = c.prop("DTEND").map(&mut time_of).transpose()?;
    let duration = match (end, c.prop("DURATION")) {
        (None, Some(p)) => Some(parse_duration(&p.value)?),
        _ => None,
    };
    let recurrence_id = c.prop("RECURRENCE-ID").map(&mut time_of).transpose()?;

    let mut list = |name: &str| -> Result<Vec<Time>, String> {
        let mut out = Vec::new();
        for p in c.props_named(name) {
            for v in p.value.split(',') {
                // RDATE;VALUE=PERIOD:start/end -> only the start matters here.
                let v = v.split('/').next().unwrap_or(v);
                let vt = p
                    .param("VALUE")
                    .filter(|v| !v.eq_ignore_ascii_case("PERIOD"));
                let (t, w) = parse_time(v, p.param("TZID"), vt)?;
                if let Some(w) = w
                    && !warnings.contains(&w)
                {
                    warnings.push(w);
                }
                out.push(t);
            }
        }
        Ok(out)
    };
    let rdates = list("RDATE")?;
    let exdates = list("EXDATE")?;

    let text = |name: &str| c.prop(name).map(|p| p.text()).unwrap_or_default();
    let event = Event {
        uid: c
            .prop("UID")
            .map(|p| p.value.trim().to_string())
            .filter(|u| !u.is_empty()),
        summary: text("SUMMARY"),
        location: text("LOCATION"),
        start,
        end,
        duration,
        rrules: c
            .props_named("RRULE")
            .map(|p| p.value.trim().to_string())
            .collect(),
        rdates,
        exdates,
        recurrence_id,
        sequence: c
            .prop("SEQUENCE")
            .and_then(|p| p.value.trim().parse().ok())
            .unwrap_or(0),
        cancelled: c
            .prop("STATUS")
            .is_some_and(|p| p.value.trim().eq_ignore_ascii_case("CANCELLED")),
        modified: c
            .prop("LAST-MODIFIED")
            .or_else(|| c.prop("DTSTAMP"))
            .map(|p| p.value.trim().to_string())
            .unwrap_or_default(),
        calendar: calendar.to_string(),
    };
    Ok((event, warnings))
}

/// Midnight of a date, for all-day arithmetic.
pub fn midnight(d: NaiveDate) -> NaiveDateTime {
    d.and_time(NaiveTime::MIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    #[test]
    fn parses_the_three_time_forms() {
        assert_eq!(
            parse_time("20261123", None, None).unwrap().0,
            Time::Date(NaiveDate::from_ymd_opt(2026, 11, 23).unwrap())
        );
        assert_eq!(
            parse_time("20261123T090000Z", None, None).unwrap().0,
            Time::Utc(dt("2026-11-23 09:00"))
        );
        assert_eq!(
            parse_time("20261123T090000", None, None).unwrap().0,
            Time::Floating(dt("2026-11-23 09:00"))
        );
        assert_eq!(
            parse_time("20261123T090000", Some("/Europe/Berlin"), None)
                .unwrap()
                .0,
            Time::Zoned(dt("2026-11-23 09:00"), Tz::Europe__Berlin)
        );
    }

    #[test]
    fn unknown_zone_is_floating_with_warning() {
        let (t, w) = parse_time("20261123T090000", Some("Pacific Standard Time"), None).unwrap();
        assert_eq!(t, Time::Floating(dt("2026-11-23 09:00")));
        assert!(w.unwrap().contains("Pacific Standard Time"));
    }

    #[test]
    fn rejects_bad_times() {
        assert!(parse_time("20260231", None, None).is_err());
        assert!(parse_time("20261123T250000", None, None).is_err());
        assert!(parse_time("tomorrow", None, None).is_err());
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("PT1H30M").unwrap(), Duration::minutes(90));
        assert_eq!(parse_duration("P1D").unwrap(), Duration::days(1));
        assert_eq!(parse_duration("-P2W").unwrap(), Duration::weeks(-2));
        assert_eq!(parse_duration("P1DT12H").unwrap(), Duration::hours(36));
        for bad in ["", "P", "PT", "1H", "PT5", "P1H", "PT1D"] {
            assert!(parse_duration(bad).is_err(), "{bad} should fail");
        }
    }
}
