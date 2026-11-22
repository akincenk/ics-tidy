//! Writing components back as iCalendar text: CRLF line ends and lines
//! folded at 75 octets (RFC 5545 section 3.1).

use crate::parse::{Component, Property};

/// Serialize a component and its children.
pub fn write_component(out: &mut String, c: &Component) {
    push_folded(out, &format!("BEGIN:{}", c.name));
    for p in &c.props {
        push_folded(out, &property_line(p));
    }
    for child in &c.children {
        write_component(out, child);
    }
    push_folded(out, &format!("END:{}", c.name));
}

fn property_line(p: &Property) -> String {
    let mut s = p.name.clone();
    for (name, value) in &p.params {
        s.push(';');
        s.push_str(name);
        s.push('=');
        s.push_str(value);
    }
    s.push(':');
    s.push_str(&p.value);
    s
}

/// Append one logical line, folded so no physical line is longer than 75
/// octets. Folds never split a UTF-8 character.
fn push_folded(out: &mut String, line: &str) {
    let mut limit = 75;
    let mut used = 0;
    for ch in line.chars() {
        let len = ch.len_utf8();
        if used + len > limit {
            out.push_str("\r\n ");
            used = 1;
            limit = 75;
        }
        out.push(ch);
        used += len;
    }
    out.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{parse, unfold};

    #[test]
    fn folds_long_lines_on_char_boundaries() {
        let mut out = String::new();
        let long = format!("SUMMARY:{}", "é".repeat(60));
        push_folded(&mut out, &long);
        for line in out.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(line.len() <= 75, "line too long: {}", line.len());
        }
        assert_eq!(unfold(&out)[0].1, long);
    }

    #[test]
    fn round_trips_a_calendar() {
        let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:x\r\nATTENDEE;CN=\"A; B\":mailto:a@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (top, warnings) = parse(text, "t");
        assert!(warnings.is_empty());
        let mut out = String::new();
        write_component(&mut out, &top[0]);
        assert_eq!(out, text);
    }
}
