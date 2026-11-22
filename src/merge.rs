//! Merging several calendars into one VCALENDAR without duplicates.

use std::collections::{HashMap, HashSet};

use crate::agenda::{instant_key, normalize};
use crate::event::parse_time;
use crate::parse::{Component, Property, Warning};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct MergeReport {
    pub components: usize,
    pub uid_duplicates: usize,
    pub content_duplicates: usize,
    pub timezones: usize,
}

/// Merge the VCALENDARs read from each source. Components with the same
/// UID and RECURRENCE-ID are kept once (highest SEQUENCE, then most recent
/// LAST-MODIFIED/DTSTAMP, then first read). With `by_content`, a VEVENT with
/// the same title, start, end and rule as an earlier one is dropped too.
pub fn merge(
    calendars: &[(String, Component)],
    by_content: bool,
    warnings: &mut Vec<Warning>,
) -> (Component, MergeReport) {
    let mut out = Component::new("VCALENDAR");
    for (name, value) in [
        ("VERSION", "2.0"),
        ("PRODID", "-//ics-tidy//EN"),
        ("CALSCALE", "GREGORIAN"),
    ] {
        out.props.push(Property {
            name: name.into(),
            params: vec![],
            value: value.into(),
        });
    }

    let mut report = MergeReport::default();
    let mut zones: Vec<Component> = Vec::new();
    let mut zone_source: HashMap<String, (usize, String)> = HashMap::new();
    let mut items: Vec<Component> = Vec::new();
    let mut by_uid: HashMap<(String, String, String), usize> = HashMap::new();
    let mut by_text: HashSet<String> = HashSet::new();

    for (source, cal) in calendars {
        if cal.name != "VCALENDAR" {
            warnings.push(Warning {
                source: source.clone(),
                line: 0,
                message: format!("top-level {} is not a VCALENDAR, skipped", cal.name),
            });
            continue;
        }
        for c in &cal.children {
            if c.name == "VTIMEZONE" {
                let id = c.prop("TZID").map(|p| p.value.clone()).unwrap_or_default();
                match zone_source.get(&id) {
                    Some((i, first)) if zones[*i] != *c => warnings.push(Warning {
                        source: source.clone(),
                        line: 0,
                        message: format!(
                            "VTIMEZONE {id:?} differs from the one in {first}; kept the first"
                        ),
                    }),
                    Some(_) => {}
                    None => {
                        zone_source.insert(id, (zones.len(), source.clone()));
                        zones.push(c.clone());
                    }
                }
                continue;
            }

            report.components += 1;
            let uid_key = c
                .prop("UID")
                .map(|p| p.value.trim().to_string())
                .filter(|u| !u.is_empty())
                .map(|uid| (c.name.clone(), uid, recurrence_key(c)));
            if let Some(&i) = uid_key.as_ref().and_then(|k| by_uid.get(k)) {
                report.uid_duplicates += 1;
                if rank(c) > rank(&items[i]) {
                    items[i] = c.clone();
                }
                continue;
            }
            if by_content && c.name == "VEVENT" && !by_text.insert(content_key(c)) {
                report.content_duplicates += 1;
                continue;
            }
            if let Some(k) = uid_key {
                by_uid.insert(k, items.len());
            }
            items.push(c.clone());
        }
    }
    report.timezones = zones.len();
    out.children = zones;
    out.children.extend(items);
    (out, report)
}

fn rank(c: &Component) -> (i64, String) {
    let seq = c
        .prop("SEQUENCE")
        .and_then(|p| p.value.trim().parse().ok())
        .unwrap_or(0);
    let modified = c
        .prop("LAST-MODIFIED")
        .or_else(|| c.prop("DTSTAMP"))
        .map(|p| p.value.trim().to_string())
        .unwrap_or_default();
    (seq, modified)
}

fn recurrence_key(c: &Component) -> String {
    let Some(p) = c.prop("RECURRENCE-ID") else {
        return String::new();
    };
    match parse_time(&p.value, p.param("TZID"), p.param("VALUE")) {
        Ok((t, _)) => instant_key(t),
        Err(_) => p.value.trim().to_string(),
    }
}

fn content_key(c: &Component) -> String {
    let raw = |name: &str| {
        c.prop(name)
            .map(|p| format!("{}|{}", p.param("TZID").unwrap_or(""), p.value.trim()))
            .unwrap_or_default()
    };
    let summary = c
        .prop("SUMMARY")
        .map(|p| normalize(&p.text()))
        .unwrap_or_default();
    let rules: Vec<String> = c
        .props_named("RRULE")
        .map(|p| p.value.trim().to_string())
        .collect();
    format!(
        "{summary}\n{}\n{}\n{}\n{}",
        raw("DTSTART"),
        raw("DTEND"),
        raw("DURATION"),
        rules.join(";")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    fn cal(body: &str, source: &str) -> (String, Component) {
        let (mut top, w) = parse(
            &format!("BEGIN:VCALENDAR\nVERSION:2.0\n{body}\nEND:VCALENDAR\n"),
            source,
        );
        assert!(w.is_empty(), "{w:?}");
        (source.to_string(), top.remove(0))
    }

    const TZ: &str = "BEGIN:VTIMEZONE\nTZID:Europe/Berlin\nBEGIN:STANDARD\nDTSTART:19701025T030000\nTZOFFSETFROM:+0200\nTZOFFSETTO:+0100\nEND:STANDARD\nEND:VTIMEZONE";

    #[test]
    fn keeps_newest_copy_and_one_timezone() {
        let a = cal(
            &format!(
                "{TZ}\nBEGIN:VEVENT\nUID:1\nSEQUENCE:0\nSUMMARY:Old\nDTSTART:20261123T100000Z\nEND:VEVENT"
            ),
            "a.ics",
        );
        let b = cal(
            &format!(
                "{TZ}\nBEGIN:VEVENT\nUID:1\nSEQUENCE:3\nSUMMARY:New\nDTSTART:20261123T100000Z\nEND:VEVENT"
            ),
            "b.ics",
        );
        let mut w = Vec::new();
        let (out, report) = merge(&[a, b], false, &mut w);
        assert!(w.is_empty());
        assert_eq!(report.uid_duplicates, 1);
        assert_eq!(report.timezones, 1);
        let names: Vec<&str> = out.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["VTIMEZONE", "VEVENT"]);
        assert_eq!(out.children[1].prop("SUMMARY").unwrap().value, "New");
    }

    #[test]
    fn overrides_are_not_duplicates_of_their_master() {
        let a = cal(
            "BEGIN:VEVENT\nUID:r\nDTSTART:20261123T100000Z\nRRULE:FREQ=DAILY\nEND:VEVENT\n\
             BEGIN:VEVENT\nUID:r\nRECURRENCE-ID:20261124T100000Z\nDTSTART:20261124T120000Z\nEND:VEVENT",
            "a.ics",
        );
        let b = cal(
            "BEGIN:VEVENT\nUID:r\nRECURRENCE-ID;TZID=Europe/Berlin:20261124T110000\nDTSTART:20261124T120000Z\nEND:VEVENT",
            "b.ics",
        );
        let (out, report) = merge(&[a, b], false, &mut Vec::new());
        assert_eq!(out.children.len(), 2);
        assert_eq!(
            report.uid_duplicates, 1,
            "same instant written in another zone is a duplicate"
        );
    }

    #[test]
    fn by_content_drops_same_event_with_other_uid() {
        let a = cal(
            "BEGIN:VEVENT\nUID:1\nSUMMARY:Dentist\nDTSTART:20261123T100000Z\nEND:VEVENT",
            "a.ics",
        );
        let b = cal(
            "BEGIN:VEVENT\nUID:2\nSUMMARY:DENTIST\nDTSTART:20261123T100000Z\nEND:VEVENT",
            "b.ics",
        );
        assert_eq!(
            merge(&[a.clone(), b.clone()], false, &mut Vec::new())
                .0
                .children
                .len(),
            2
        );
        let (out, report) = merge(&[a, b], true, &mut Vec::new());
        assert_eq!(out.children.len(), 1);
        assert_eq!(report.content_duplicates, 1);
    }

    #[test]
    fn warns_on_conflicting_timezone() {
        let a = cal(TZ, "a.ics");
        let b = cal(&TZ.replace("+0100", "+0000"), "b.ics");
        let mut w = Vec::new();
        merge(&[a, b], false, &mut w);
        assert_eq!(w.len(), 1);
        assert!(w[0].message.contains("differs from the one in a.ics"));
    }
}
