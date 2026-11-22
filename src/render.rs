//! Agenda output as plain text or CSV.

use chrono::{Duration, NaiveDate};

use crate::agenda::Occurrence;

/// Plain text, one heading per day.
pub fn text(list: &[Occurrence]) -> String {
    let mut out = String::new();
    let mut day: Option<NaiveDate> = None;
    for o in list {
        let d = o.start.date();
        if day != Some(d) {
            if day.is_some() {
                out.push('\n');
            }
            out.push_str(&d.format("%a %Y-%m-%d\n").to_string());
            day = Some(d);
        }
        let last_day = (o.end - Duration::days(1)).date();
        let when = if o.all_day {
            "all day".to_string()
        } else if o.start == o.end {
            o.start.format("%H:%M").to_string()
        } else {
            let days = (o.end.date() - d).num_days();
            let tail = if days > 0 {
                format!("+{days}d")
            } else {
                String::new()
            };
            format!(
                "{}-{}{tail}",
                o.start.format("%H:%M"),
                o.end.format("%H:%M")
            )
        };
        let summary = one_line(&o.summary);
        let title = if summary.is_empty() {
            "(no title)".to_string()
        } else {
            summary
        };
        out.push_str(&format!("  {when:<14} {title}"));
        if o.all_day && last_day > d {
            out.push_str(&format!(" (until {})", last_day.format("%a %Y-%m-%d")));
        }
        if !o.location.is_empty() {
            out.push_str(&format!(" ({})", one_line(&o.location)));
        }
        out.push_str(&format!("  [{}]\n", o.calendar));
    }
    out
}

/// CSV with a header row. Timed entries use `YYYY-MM-DDTHH:MM` with an
/// exclusive end; all-day entries use dates with an inclusive last day.
pub fn csv(list: &[Occurrence]) -> String {
    let mut out = String::from("start,end,all_day,summary,location,calendar,uid\r\n");
    for o in list {
        let (start, end) = if o.all_day {
            (
                o.start.date().to_string(),
                (o.end - Duration::days(1)).date().to_string(),
            )
        } else {
            (
                o.start.format("%Y-%m-%dT%H:%M").to_string(),
                o.end.format("%Y-%m-%dT%H:%M").to_string(),
            )
        };
        let fields = [
            start,
            end,
            o.all_day.to_string(),
            o.summary.clone(),
            o.location.clone(),
            o.calendar.clone(),
            o.uid.clone().unwrap_or_default(),
        ];
        let row: Vec<String> = fields.iter().map(|f| field(f)).collect();
        out.push_str(&row.join(","));
        out.push_str("\r\n");
    }
    out
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Quote a CSV field. Text from calendar files is untrusted: a value that a
/// spreadsheet would run as a formula gets a leading apostrophe.
fn field(s: &str) -> String {
    let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn occ(start: &str, end: &str, all_day: bool, summary: &str) -> Occurrence {
        let p = |s: &str| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        Occurrence {
            all_day,
            start: p(start),
            end: p(end),
            summary: summary.into(),
            location: String::new(),
            calendar: "work".into(),
            uid: Some("u1".into()),
        }
    }

    #[test]
    fn text_groups_by_day() {
        let list = [
            occ("2026-11-23 00:00", "2026-11-25 00:00", true, "Stock take"),
            occ("2026-11-23 22:00", "2026-11-24 06:00", false, "Night shift"),
            occ("2026-11-24 09:00", "2026-11-24 09:00", false, "Reminder"),
        ];
        assert_eq!(
            text(&list),
            "Mon 2026-11-23\n  all day        Stock take (until Tue 2026-11-24)  [work]\n  22:00-06:00+1d Night shift  [work]\n\nTue 2026-11-24\n  09:00          Reminder  [work]\n"
        );
    }

    #[test]
    fn csv_quotes_and_neutralizes_formulas() {
        let list = [occ(
            "2026-11-23 09:00",
            "2026-11-23 10:00",
            false,
            "=HYPERLINK(\"x\"), now",
        )];
        assert_eq!(
            csv(&list),
            "start,end,all_day,summary,location,calendar,uid\r\n2026-11-23T09:00,2026-11-23T10:00,false,\"'=HYPERLINK(\"\"x\"\"), now\",,work,u1\r\n"
        );
    }

    #[test]
    fn csv_all_day_uses_inclusive_last_day() {
        let list = [occ("2026-11-23 00:00", "2026-11-25 00:00", true, "Trip")];
        assert!(csv(&list).contains("2026-11-23,2026-11-24,true,Trip"));
    }
}
