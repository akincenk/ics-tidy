//! Reading iCalendar text (RFC 5545 section 3.1): line unfolding, content
//! lines and the BEGIN/END component tree.

use std::fmt;

/// One content line, for example `DTSTART;TZID=Europe/Berlin:20261123T090000`.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    /// Upper-cased property name.
    pub name: String,
    /// Parameters as written: upper-cased name, raw value (quotes kept).
    pub params: Vec<(String, String)>,
    /// Raw value, still escaped.
    pub value: String,
}

impl Property {
    /// First value of a parameter, without surrounding quotes.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim_matches('"'))
    }

    /// The value with TEXT escapes (`\n`, `\,`, `\;`, `\\`) resolved.
    pub fn text(&self) -> String {
        unescape_text(&self.value)
    }
}

/// A component such as VCALENDAR, VEVENT or VTIMEZONE.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Component {
    pub name: String,
    pub props: Vec<Property>,
    pub children: Vec<Component>,
}

impl Component {
    pub fn new(name: &str) -> Self {
        Component {
            name: name.to_string(),
            ..Default::default()
        }
    }

    /// First property with this name.
    pub fn prop(&self, name: &str) -> Option<&Property> {
        self.props.iter().find(|p| p.name == name)
    }

    /// All properties with this name, in file order.
    pub fn props_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Property> + 'a {
        self.props.iter().filter(move |p| p.name == name)
    }
}

/// Something in the input that was skipped or read in a forgiving way.
#[derive(Debug, Clone, PartialEq)]
pub struct Warning {
    /// Source name (file name or "-").
    pub source: String,
    /// 1-based line number in the source, 0 when not tied to a line.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "{}:{}: {}", self.source, self.line, self.message)
        } else {
            write!(f, "{}: {}", self.source, self.message)
        }
    }
}

/// Parse iCalendar text into its top-level components (normally one
/// VCALENDAR per file). Problems never abort the parse; they are returned
/// as warnings and the offending line is skipped.
pub fn parse(text: &str, source: &str) -> (Vec<Component>, Vec<Warning>) {
    let mut warnings = Vec::new();
    let mut warn = |line: usize, message: String| {
        warnings.push(Warning {
            source: source.to_string(),
            line,
            message,
        })
    };

    let mut top: Vec<Component> = Vec::new();
    let mut stack: Vec<(Component, usize)> = Vec::new();

    for (line_no, line) in unfold(text) {
        if line.trim().is_empty() {
            continue;
        }
        let prop = match parse_line(&line) {
            Ok(p) => p,
            Err(e) => {
                warn(line_no, format!("{e}, line skipped"));
                continue;
            }
        };
        match prop.name.as_str() {
            "BEGIN" => {
                let name = prop.value.trim().to_ascii_uppercase();
                stack.push((Component::new(&name), line_no));
            }
            "END" => {
                let name = prop.value.trim().to_ascii_uppercase();
                let Some(pos) = stack.iter().rposition(|(c, _)| c.name == name) else {
                    warn(
                        line_no,
                        format!("END:{name} without a matching BEGIN, ignored"),
                    );
                    continue;
                };
                while stack.len() > pos + 1 {
                    let (inner, begin) = stack.pop().expect("stack longer than pos");
                    warn(
                        begin,
                        format!("BEGIN:{} is not closed before END:{name}", inner.name),
                    );
                    attach(&mut stack, &mut top, inner);
                }
                let (done, _) = stack.pop().expect("pos is in the stack");
                attach(&mut stack, &mut top, done);
            }
            _ => match stack.last_mut() {
                Some((c, _)) => c.props.push(prop),
                None => warn(
                    line_no,
                    format!("{} outside of any component, ignored", prop.name),
                ),
            },
        }
    }
    while let Some((c, begin)) = stack.pop() {
        warn(begin, format!("BEGIN:{} is never closed", c.name));
        attach(&mut stack, &mut top, c);
    }
    (top, warnings)
}

fn attach(stack: &mut [(Component, usize)], top: &mut Vec<Component>, c: Component) {
    match stack.last_mut() {
        Some((parent, _)) => parent.children.push(c),
        None => top.push(c),
    }
}

/// Join folded lines (a line starting with a space or tab continues the
/// previous one). Returns each logical line with the number of its first
/// physical line.
pub fn unfold(text: &str) -> Vec<(usize, String)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out: Vec<(usize, String)> = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = raw.strip_prefix([' ', '\t'])
            && let Some((_, last)) = out.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        out.push((i + 1, raw.to_string()));
    }
    out
}

/// Split one unfolded line into name, parameters and value.
pub fn parse_line(line: &str) -> Result<Property, String> {
    let name_end = line
        .find([';', ':'])
        .ok_or_else(|| "no ':' between name and value".to_string())?;
    let name = &line[..name_end];
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("invalid property name {name:?}"));
    }

    let mut params = Vec::new();
    let mut rest = &line[name_end..];
    while let Some(after) = rest.strip_prefix(';') {
        let eq = after
            .find('=')
            .ok_or_else(|| format!("parameter without '=' in {name}"))?;
        let pname = &after[..eq];
        let mut in_quotes = false;
        let mut end = None;
        for (i, c) in after[eq + 1..].char_indices() {
            match c {
                '"' => in_quotes = !in_quotes,
                ';' | ':' if !in_quotes => {
                    end = Some(eq + 1 + i);
                    break;
                }
                _ => {}
            }
        }
        let end = end.ok_or_else(|| format!("no ':' after the parameters of {name}"))?;
        params.push((pname.to_ascii_uppercase(), after[eq + 1..end].to_string()));
        rest = &after[end..];
    }
    let value = rest
        .strip_prefix(':')
        .ok_or_else(|| format!("no ':' after {name}"))?;
    Ok(Property {
        name: name.to_ascii_uppercase(),
        params,
        value: value.to_string(),
    })
}

/// Resolve TEXT escapes (RFC 5545 section 3.3.11).
pub fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfolds_continuation_lines() {
        let lines = unfold("SUMMARY:Long\r\n  title\r\n\t end\r\nUID:1\r\n");
        assert_eq!(lines[0], (1, "SUMMARY:Long title end".to_string()));
        assert_eq!(lines[1], (4, "UID:1".to_string()));
    }

    #[test]
    fn parses_params_with_quotes() {
        let p = parse_line(r#"ATTENDEE;CN="Doe; Jane";ROLE=CHAIR:mailto:j@example.com"#).unwrap();
        assert_eq!(p.name, "ATTENDEE");
        assert_eq!(p.param("cn"), Some("Doe; Jane"));
        assert_eq!(p.param("ROLE"), Some("CHAIR"));
        assert_eq!(p.value, "mailto:j@example.com");
    }

    #[test]
    fn rejects_lines_without_colon() {
        assert!(parse_line("SUMMARY no colon").is_err());
        assert!(parse_line("BAD NAME:x").is_err());
        assert!(parse_line("X;Y:z").is_err());
    }

    #[test]
    fn unescapes_text() {
        assert_eq!(unescape_text(r"a\, b\; c\\d\nnext"), "a, b; c\\d\nnext");
    }

    #[test]
    fn builds_component_tree_and_warns() {
        let text = "BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nUID:a\ngarbage\nEND:VCALENDAR\nSUMMARY:stray\n";
        let (top, warnings) = parse(text, "t.ics");
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].children[0].name, "VEVENT");
        assert_eq!(top[0].children[0].prop("UID").unwrap().value, "a");
        let msgs: Vec<String> = warnings.iter().map(|w| w.to_string()).collect();
        assert_eq!(
            msgs,
            vec![
                "t.ics:5: no ':' between name and value, line skipped",
                "t.ics:3: BEGIN:VEVENT is not closed before END:VCALENDAR",
                "t.ics:7: SUMMARY outside of any component, ignored",
            ]
        );
    }
}
