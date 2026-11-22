//! ics-tidy: merge iCalendar (RFC 5545) files, remove duplicates, expand
//! recurring events into a date range and print an agenda or CSV.
//!
//! ```
//! use ics_tidy::{agenda, events_in, parse};
//! use chrono::NaiveDate;
//!
//! let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Standup\r\n\
//!             DTSTART:20261123T150000Z\r\nDURATION:PT15M\r\nRRULE:FREQ=DAILY;COUNT=3\r\n\
//!             END:VEVENT\r\nEND:VCALENDAR\r\n";
//! let (calendars, mut warnings) = parse::parse(text, "work.ics");
//! let events = events_in(&calendars, "work.ics", "work", &mut warnings);
//! let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(0, 0, 0).unwrap();
//! let range = agenda::Range { from: day(23), to: day(30) };
//! let (list, report) = agenda::agenda(events, range, chrono_tz::Tz::UTC, false);
//! assert_eq!(list.len(), 3);
//! assert_eq!(report.events, 1);
//! ```

pub mod agenda;
pub mod event;
pub mod parse;
pub mod write;

use parse::{Component, Warning};

/// Read every VEVENT inside the given VCALENDARs. Events that cannot be read
/// (for example a missing or invalid DTSTART) are skipped with a warning.
pub fn events_in(
    calendars: &[Component],
    source: &str,
    calendar: &str,
    warnings: &mut Vec<Warning>,
) -> Vec<event::Event> {
    let mut out = Vec::new();
    let mut warn = |message: String| {
        warnings.push(Warning {
            source: source.to_string(),
            line: 0,
            message,
        })
    };
    for cal in calendars {
        for c in cal.children.iter().filter(|c| c.name == "VEVENT") {
            let label = match c.prop("UID") {
                Some(u) => format!("VEVENT {}", u.value.trim()),
                None => "VEVENT without UID".to_string(),
            };
            match event::event_from(c, calendar) {
                Ok((e, ws)) => {
                    for w in ws {
                        warn(format!("{label}: {w}"));
                    }
                    out.push(e);
                }
                Err(e) => warn(format!("{label} skipped: {e}")),
            }
        }
    }
    out
}
