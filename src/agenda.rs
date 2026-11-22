//! Turning events into a dated agenda: duplicate removal by UID, recurrence
//! expansion (RRULE, RDATE, EXDATE, RECURRENCE-ID overrides) and removal
//! of identical occurrences that come from different calendars.

use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveDateTime, Offset, TimeZone};
use chrono_tz::Tz;
use rrule::{RRule, RRuleSet, Unvalidated};

use crate::event::{Event, Time, midnight};

type RTz = rrule::Tz;

/// One entry in the agenda, in the display time zone.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    pub all_day: bool,
    pub start: NaiveDateTime,
    /// Exclusive end. For all-day entries this is midnight after the last day.
    pub end: NaiveDateTime,
    pub summary: String,
    pub location: String,
    /// Calendar names, joined with ", " when the same entry came from several.
    pub calendar: String,
    pub uid: Option<String>,
}

/// Wall-clock range `[from, to)` in the display zone.
#[derive(Debug, Clone, Copy)]
pub struct Range {
    pub from: NaiveDateTime,
    pub to: NaiveDateTime,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Report {
    pub events: usize,
    pub uid_duplicates: usize,
    pub content_duplicates: usize,
    pub cancelled: usize,
    pub warnings: Vec<String>,
}

/// Most occurrences one event may produce inside the range.
const MAX_PER_EVENT: u16 = u16::MAX;

/// Build the agenda for `range`, shown in `display`.
pub fn agenda(
    events: Vec<Event>,
    range: Range,
    display: Tz,
    keep_duplicates: bool,
) -> (Vec<Occurrence>, Report) {
    let mut report = Report {
        events: events.len(),
        ..Default::default()
    };
    let events = dedupe_by_uid(events, &mut report);

    // Instances replaced or cancelled by a RECURRENCE-ID override, per UID.
    let mut overridden: HashMap<&str, Vec<DateTime<RTz>>> = HashMap::new();
    for e in &events {
        if let (Some(uid), Some(rid)) = (&e.uid, e.recurrence_id) {
            overridden.entry(uid.as_str()).or_default().push(to_rr(rid));
        }
    }

    let mut out = Vec::new();
    for e in &events {
        if e.cancelled {
            report.cancelled += 1;
            continue;
        }
        let skip: &[DateTime<RTz>] = match (&e.uid, e.recurrence_id) {
            (Some(uid), None) => overridden
                .get(uid.as_str())
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            _ => &[],
        };
        out.extend(expand(e, skip, range, display, &mut report.warnings));
    }
    out.sort_by(|a, b| {
        (a.start.date(), !a.all_day, a.start, &a.summary).cmp(&(
            b.start.date(),
            !b.all_day,
            b.start,
            &b.summary,
        ))
    });
    if !keep_duplicates {
        out = dedupe_by_content(out, &mut report);
    }
    (out, report)
}

/// Keep one event per (UID, RECURRENCE-ID): highest SEQUENCE, then the most
/// recent LAST-MODIFIED/DTSTAMP, then the first one read.
fn dedupe_by_uid(events: Vec<Event>, report: &mut Report) -> Vec<Event> {
    let mut index: HashMap<(String, Option<String>), usize> = HashMap::new();
    let mut kept: Vec<Event> = Vec::new();
    for e in events {
        let Some(uid) = e.uid.clone() else {
            kept.push(e);
            continue;
        };
        let key = (uid, e.recurrence_id.map(instant_key));
        match index.get(&key) {
            Some(&i) => {
                report.uid_duplicates += 1;
                if (e.sequence, &e.modified) > (kept[i].sequence, &kept[i].modified) {
                    kept[i] = e;
                }
            }
            None => {
                index.insert(key, kept.len());
                kept.push(e);
            }
        }
    }
    kept
}

/// Drop entries with the same title, start and end as an earlier one; the
/// kept entry lists every calendar it came from.
fn dedupe_by_content(list: Vec<Occurrence>, report: &mut Report) -> Vec<Occurrence> {
    let mut index: HashMap<(String, NaiveDateTime, NaiveDateTime, bool), usize> = HashMap::new();
    let mut kept: Vec<Occurrence> = Vec::new();
    for o in list {
        let key = (normalize(&o.summary), o.start, o.end, o.all_day);
        match index.get(&key) {
            Some(&i) => {
                report.content_duplicates += 1;
                let k = &mut kept[i];
                if !k.calendar.split(", ").any(|c| c == o.calendar) {
                    k.calendar = format!("{}, {}", k.calendar, o.calendar);
                }
            }
            None => {
                index.insert(key, kept.len());
                kept.push(o);
            }
        }
    }
    kept
}

/// Lower case with runs of whitespace collapsed, for comparing titles.
pub fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A key that is equal for two values naming the same instant (or the same
/// date / floating time).
pub fn instant_key(t: Time) -> String {
    match t {
        Time::Date(d) => d.to_string(),
        Time::Floating(n) => format!("floating {n}"),
        _ => to_rr(t).naive_utc().to_string(),
    }
}

/// Convert to the rrule crate's type. Dates and floating times are carried
/// as UTC wall-clock values so recurrence works on the wall clock.
fn to_rr(t: Time) -> DateTime<RTz> {
    match t {
        Time::Date(d) => RTz::UTC.from_utc_datetime(&midnight(d)),
        Time::Floating(n) | Time::Utc(n) => RTz::UTC.from_utc_datetime(&n),
        Time::Zoned(n, z) => resolve_local(z, n),
    }
}

/// Local time to instant per RFC 5545 3.3.5: a repeated time means the first
/// one; a time inside a gap uses the UTC offset from before the gap.
fn resolve_local(z: Tz, n: NaiveDateTime) -> DateTime<RTz> {
    let rz: RTz = z.into();
    if let Some(dt) = rz.from_local_datetime(&n).earliest() {
        return dt;
    }
    let before = z.offset_from_utc_datetime(&(n - Duration::hours(3))).fix();
    rz.from_utc_datetime(&(n - before))
}

/// Wall-clock time of an occurrence in the display zone.
fn shown(kind: Time, at: DateTime<RTz>, display: Tz) -> NaiveDateTime {
    match kind {
        Time::Date(_) | Time::Floating(_) => at.naive_utc(),
        Time::Utc(_) | Time::Zoned(..) => at.with_timezone(&display).naive_local(),
    }
}

fn length(e: &Event) -> Duration {
    let d = match (e.end, e.duration) {
        (Some(end), _) => match (e.start, end) {
            (Time::Date(s), Time::Date(t)) => t - s,
            (Time::Floating(s), Time::Floating(t)) => t - s,
            _ => to_rr(end) - to_rr(e.start),
        },
        (None, Some(d)) => d,
        (None, None) if e.start.is_date() => Duration::days(1),
        (None, None) => Duration::zero(),
    };
    if e.start.is_date() && d <= Duration::zero() {
        Duration::days(1)
    } else {
        d.max(Duration::zero())
    }
}

/// RFC 5545 wants UNTIL in UTC when DTSTART has a time zone, but many
/// calendars write a plain date (`UNTIL=20261231`) or a local time. Rewrite
/// such values to the UTC instant they mean (a date means the end of that
/// day) so the rule is not rejected.
fn until_as_utc(rule: &str, start: Time) -> String {
    let parts: Vec<String> = rule
        .split(';')
        .map(|part| {
            let Some((key, value)) = part.split_once('=') else {
                return part.to_string();
            };
            if !key.trim().eq_ignore_ascii_case("UNTIL") || value.ends_with(['Z', 'z']) {
                return part.to_string();
            }
            let local = match crate::event::parse_time(value, None, None) {
                Ok((Time::Date(d), _)) => d.and_hms_opt(23, 59, 59).expect("valid time"),
                Ok((Time::Floating(n), _)) => n,
                _ => return part.to_string(),
            };
            let utc = match start {
                Time::Zoned(_, z) => resolve_local(z, local).naive_utc(),
                _ => local,
            };
            format!("{key}={}", utc.format("%Y%m%dT%H%M%SZ"))
        })
        .collect();
    parts.join(";")
}

/// All occurrences of one event that overlap `range`.
fn expand(
    e: &Event,
    skip: &[DateTime<RTz>],
    range: Range,
    display: Tz,
    warnings: &mut Vec<String>,
) -> Vec<Occurrence> {
    let start = to_rr(e.start);
    let len = length(e);
    let label = || match &e.uid {
        Some(uid) => format!("{}: event {:?} (UID {uid})", e.calendar, e.summary),
        None => format!("{}: event {:?}", e.calendar, e.summary),
    };

    let starts: Vec<DateTime<RTz>> =
        if e.recurrence_id.is_some() || (e.rrules.is_empty() && e.rdates.is_empty()) {
            vec![start]
        } else {
            let mut set = RRuleSet::new(start);
            let mut any_rule = false;
            for r in &e.rrules {
                match until_as_utc(r, e.start)
                    .parse::<RRule<Unvalidated>>()
                    .and_then(|r| r.validate(start))
                {
                    Ok(v) => {
                        set = set.rrule(v);
                        any_rule = true;
                    }
                    Err(err) => warnings.push(format!("{}: RRULE {r:?} ignored: {err}", label())),
                }
            }
            if !any_rule {
                set = set.rdate(start);
            }
            for d in &e.rdates {
                set = set.rdate(to_rr(*d));
            }
            for d in &e.exdates {
                set = set.exdate(to_rr(*d));
            }
            // Wide window on purpose: the exact overlap test happens below.
            let lo = RTz::UTC.from_utc_datetime(&(range.from - len - Duration::days(2)));
            let hi = RTz::UTC.from_utc_datetime(&(range.to + Duration::days(2)));
            let result = set.after(lo).before(hi).all(MAX_PER_EVENT);
            if result.limited && result.dates.len() == usize::from(MAX_PER_EVENT) {
                warnings.push(format!(
                    "{}: more than {MAX_PER_EVENT} occurrences in range, rest skipped",
                    label()
                ));
            }
            result.dates
        };

    starts
        .into_iter()
        .filter(|s| !skip.contains(s))
        .filter_map(|s| {
            let (begin, end) = (shown(e.start, s, display), shown(e.start, s + len, display));
            let overlaps = if begin == end {
                begin >= range.from && begin < range.to
            } else {
                begin < range.to && end > range.from
            };
            overlaps.then(|| Occurrence {
                all_day: e.start.is_date(),
                start: begin,
                end,
                summary: e.summary.clone(),
                location: e.location.clone(),
                calendar: e.calendar.clone(),
                uid: e.uid.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::event_from;
    use crate::parse::parse;

    fn events(ics: &str, calendar: &str) -> Vec<Event> {
        let text = format!("BEGIN:VCALENDAR\n{ics}\nEND:VCALENDAR\n");
        let (top, w) = parse(&text, calendar);
        assert!(w.is_empty(), "{w:?}");
        top[0]
            .children
            .iter()
            .map(|c| event_from(c, calendar).unwrap().0)
            .collect()
    }

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn range(a: &str, b: &str) -> Range {
        Range {
            from: dt(a),
            to: dt(b),
        }
    }

    fn starts(list: &[Occurrence]) -> Vec<String> {
        list.iter()
            .map(|o| o.start.format("%Y-%m-%d %H:%M").to_string())
            .collect()
    }

    #[test]
    fn weekly_rule_keeps_wall_clock_across_dst() {
        let ev = events(
            "BEGIN:VEVENT\nUID:w\nSUMMARY:Standup\nDTSTART;TZID=America/Toronto:20261026T090000\nDTEND;TZID=America/Toronto:20261026T091500\nRRULE:FREQ=WEEKLY;COUNT=3\nEND:VEVENT",
            "work",
        );
        let (list, report) = agenda(
            ev,
            range("2026-10-01 00:00", "2026-12-01 00:00"),
            Tz::America__Toronto,
            false,
        );
        assert_eq!(
            starts(&list),
            ["2026-10-26 09:00", "2026-11-02 09:00", "2026-11-09 09:00"]
        );
        assert_eq!(list[0].end, dt("2026-10-26 09:15"));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn converts_to_display_zone() {
        let ev = events(
            "BEGIN:VEVENT\nUID:c\nSUMMARY:Call\nDTSTART;TZID=Europe/Berlin:20261124T170000\nDURATION:PT30M\nEND:VEVENT",
            "work",
        );
        let (list, _) = agenda(
            ev,
            range("2026-11-24 00:00", "2026-11-25 00:00"),
            Tz::America__Toronto,
            false,
        );
        assert_eq!(starts(&list), ["2026-11-24 11:00"]);
        assert_eq!(list[0].end, dt("2026-11-24 11:30"));
    }

    #[test]
    fn exdate_and_override_replace_instances() {
        let ev = events(
            "BEGIN:VEVENT\nUID:r\nSUMMARY:Review\nDTSTART:20261123T150000Z\nDTEND:20261123T160000Z\nRRULE:FREQ=DAILY;COUNT=4\nEXDATE:20261124T150000Z\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:r\nRECURRENCE-ID:20261125T150000Z\nSUMMARY:Review (moved)\nDTSTART:20261125T180000Z\nDTEND:20261125T190000Z\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:r\nRECURRENCE-ID:20261126T150000Z\nSTATUS:CANCELLED\nSUMMARY:Review\nDTSTART:20261126T150000Z\nEND:VEVENT",
            "work",
        );
        let (list, report) = agenda(
            ev,
            range("2026-11-20 00:00", "2026-11-30 00:00"),
            Tz::UTC,
            false,
        );
        assert_eq!(starts(&list), ["2026-11-23 15:00", "2026-11-25 18:00"]);
        assert_eq!(list[1].summary, "Review (moved)");
        assert_eq!(report.cancelled, 1);
    }

    #[test]
    fn all_day_events_stay_on_their_date() {
        let ev = events(
            "BEGIN:VEVENT\nUID:h\nSUMMARY:Inventory day\nDTSTART;VALUE=DATE:20261127\nRRULE:FREQ=MONTHLY;BYMONTHDAY=27;COUNT=2\nEND:VEVENT",
            "store",
        );
        let (list, _) = agenda(
            ev,
            range("2026-11-01 00:00", "2027-01-01 00:00"),
            Tz::Pacific__Auckland,
            false,
        );
        assert_eq!(starts(&list), ["2026-11-27 00:00", "2026-12-27 00:00"]);
        assert!(list[0].all_day);
        assert_eq!(list[0].end, dt("2026-11-28 00:00"));
    }

    #[test]
    fn keeps_highest_sequence_for_same_uid() {
        let mut ev = events(
            "BEGIN:VEVENT\nUID:s\nSEQUENCE:1\nSUMMARY:Old\nDTSTART:20261123T100000Z\nEND:VEVENT",
            "a",
        );
        ev.extend(events(
            "BEGIN:VEVENT\nUID:s\nSEQUENCE:2\nSUMMARY:New\nDTSTART:20261123T110000Z\nEND:VEVENT",
            "b",
        ));
        let (list, report) = agenda(
            ev,
            range("2026-11-23 00:00", "2026-11-24 00:00"),
            Tz::UTC,
            false,
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].summary, "New");
        assert_eq!(report.uid_duplicates, 1);
    }

    #[test]
    fn merges_same_entry_from_two_calendars() {
        let mut ev = events(
            "BEGIN:VEVENT\nUID:1\nSUMMARY:Dentist\nDTSTART:20261123T100000Z\nDTEND:20261123T110000Z\nEND:VEVENT",
            "home",
        );
        ev.extend(events("BEGIN:VEVENT\nUID:2\nSUMMARY:dentist \nDTSTART:20261123T100000Z\nDTEND:20261123T110000Z\nEND:VEVENT", "work"));
        let (list, report) = agenda(
            ev.clone(),
            range("2026-11-23 00:00", "2026-11-24 00:00"),
            Tz::UTC,
            false,
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].calendar, "home, work");
        assert_eq!(report.content_duplicates, 1);
        let (list, _) = agenda(
            ev,
            range("2026-11-23 00:00", "2026-11-24 00:00"),
            Tz::UTC,
            true,
        );
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn bad_rule_is_reported_and_start_kept() {
        let ev = events(
            "BEGIN:VEVENT\nUID:b\nSUMMARY:X\nDTSTART:20261123T100000Z\nRRULE:FREQ=SOMETIMES\nEND:VEVENT",
            "a",
        );
        let (list, report) = agenda(
            ev,
            range("2026-11-23 00:00", "2026-11-24 00:00"),
            Tz::UTC,
            false,
        );
        assert_eq!(list.len(), 1);
        assert!(
            report.warnings[0].contains("FREQ=SOMETIMES"),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn date_until_is_read_as_end_of_that_day() {
        let ev = events(
            "BEGIN:VEVENT\nUID:g\nSUMMARY:Gym\nDTSTART;TZID=America/Vancouver:20261201T063000\nRRULE:FREQ=DAILY;UNTIL=20261203\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:p\nSUMMARY:Payday\nDTSTART;VALUE=DATE:20261201\nRRULE:FREQ=DAILY;UNTIL=20261202\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:l\nSUMMARY:Lunch\nDTSTART;TZID=America/Vancouver:20261201T120000\nRRULE:FREQ=DAILY;UNTIL=20261202T120000\nEND:VEVENT",
            "home",
        );
        let (list, report) = agenda(
            ev,
            range("2026-12-01 00:00", "2026-12-10 00:00"),
            Tz::America__Vancouver,
            false,
        );
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let summaries: Vec<&str> = list.iter().map(|o| o.summary.as_str()).collect();
        assert_eq!(
            summaries,
            ["Payday", "Gym", "Lunch", "Payday", "Gym", "Lunch", "Gym"]
        );
    }

    #[test]
    fn time_in_dst_gap_uses_offset_before_gap() {
        // 02:30 does not exist in Toronto on 2027-03-14; RFC 5545 reads it as 03:30.
        let at = resolve_local(Tz::America__Toronto, dt("2027-03-14 02:30"));
        assert_eq!(at.naive_utc(), dt("2027-03-14 07:30"));
    }
}
