use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

use chrono::{Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use clap::{Parser, Subcommand, ValueEnum};

use ics_tidy::{agenda, event, events_in, merge, parse, render, write};

#[derive(Parser)]
#[command(
    version,
    about = "Merge iCalendar files, remove duplicates, list events in a date range"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Exit with status 1 when there were any warnings (output is still written).
    #[arg(long, global = true)]
    strict: bool,
}

#[derive(Subcommand)]
enum Command {
    /// List events (recurring ones expanded) from one or more .ics files.
    Agenda {
        /// .ics files to read; "-" reads standard input.
        #[arg(required = true)]
        files: Vec<String>,
        /// First day to show (YYYY-MM-DD). Default: today.
        #[arg(long)]
        from: Option<NaiveDate>,
        /// Last day to show (YYYY-MM-DD), inclusive.
        #[arg(long, conflicts_with = "days")]
        to: Option<NaiveDate>,
        /// Number of days to show.
        #[arg(long, default_value_t = 7)]
        days: u32,
        /// Time zone to show times in (IANA name). Default: this computer's zone.
        #[arg(long)]
        tz: Option<String>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// Keep entries with the same title and time from different calendars.
        #[arg(long)]
        keep_duplicates: bool,
    },
    /// Merge .ics files into one calendar, keeping one copy of each event.
    Merge {
        /// .ics files to read; "-" reads standard input.
        #[arg(required = true)]
        files: Vec<String>,
        /// Write to this file instead of standard output.
        #[arg(short, long)]
        output: Option<String>,
        /// Also drop events with another UID but the same title, start, end and rule.
        #[arg(long)]
        by_content: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Csv,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let strict = cli.strict;
    match run(cli) {
        Ok(warnings) if strict && warnings > 0 => ExitCode::from(1),
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ics-tidy: {e}");
            ExitCode::from(2)
        }
    }
}

/// Read one input. Returns the display name of the source and its calendar
/// name (file name without extension).
fn read_input(
    path: &str,
    warnings: &mut Vec<parse::Warning>,
) -> Result<(String, String, String), String> {
    let mut bytes = Vec::new();
    let (source, calendar) = if path == "-" {
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|e| format!("standard input: {e}"))?;
        ("-".to_string(), "stdin".to_string())
    } else {
        bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let stem = Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        (path.to_string(), stem)
    };
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => {
            warnings.push(parse::Warning {
                source: source.clone(),
                line: 0,
                message: "not valid UTF-8, unreadable characters replaced".into(),
            });
            String::from_utf8_lossy(e.as_bytes()).into_owned()
        }
    };
    Ok((source, calendar, text))
}

/// Read and parse one input. Returns its source name, calendar name and
/// top-level components. Input without any VCALENDAR (an empty file, or an
/// HTML error page saved as .ics) is an error, not an empty calendar.
fn parse_input(
    path: &str,
    warnings: &mut Vec<parse::Warning>,
) -> Result<(String, String, Vec<parse::Component>), String> {
    let (source, calendar, text) = read_input(path, warnings)?;
    let (cals, ws) = parse::parse(&text, &source);
    if !cals.iter().any(|c| c.name == "VCALENDAR") {
        return Err(
            match text
                .trim_start_matches('\u{feff}')
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
            {
                None => format!("{source}: empty input, no VCALENDAR"),
                Some(first) => {
                    let first: String = first.chars().take(60).collect();
                    format!(
                        "{source}: no VCALENDAR found, not an iCalendar file? (first line: {first:?})"
                    )
                }
            },
        );
    }
    warnings.extend(ws);
    Ok((source, calendar, cals))
}

fn display_zone(name: Option<&str>) -> Result<Tz, String> {
    match name {
        Some(n) => event::lookup_zone(n).ok_or_else(|| {
            format!("unknown time zone {n:?} (use an IANA name such as America/Vancouver)")
        }),
        None => Ok(iana_time_zone::get_timezone()
            .ok()
            .and_then(|n| event::lookup_zone(&n))
            .unwrap_or(Tz::UTC)),
    }
}

/// Returns the number of warnings.
fn run(cli: Cli) -> Result<usize, String> {
    let mut warnings = Vec::new();
    match cli.command {
        Command::Agenda {
            files,
            from,
            to,
            days,
            tz,
            format,
            keep_duplicates,
        } => {
            let display = display_zone(tz.as_deref())?;
            let from = from.unwrap_or_else(|| Utc::now().with_timezone(&display).date_naive());
            let end_day = match to {
                Some(t) if t < from => return Err(format!("--to {t} is before --from {from}")),
                Some(t) => t + Duration::days(1),
                None => from + Duration::days(i64::from(days)),
            };
            let range = agenda::Range {
                from: event::midnight(from),
                to: event::midnight(end_day),
            };

            let mut events = Vec::new();
            for f in &files {
                let (source, calendar, cals) = parse_input(f, &mut warnings)?;
                events.extend(events_in(&cals, &source, &calendar, &mut warnings));
            }
            let (list, report) = agenda::agenda(events, range, display, keep_duplicates);
            let out = match format {
                Format::Text => render::text(&list),
                Format::Csv => render::csv(&list),
            };
            print_out(&out)?;

            for w in &warnings {
                eprintln!("warning: {w}");
            }
            for w in &report.warnings {
                eprintln!("warning: {w}");
            }
            eprintln!(
                "ics-tidy: {} files, {} events, {} entries from {from} to {} ({display}); removed {} same-UID copies and {} same title-and-time entries; {} cancelled skipped; {} warnings",
                files.len(),
                report.events,
                list.len(),
                end_day - Duration::days(1),
                report.uid_duplicates,
                report.content_duplicates,
                report.cancelled,
                warnings.len() + report.warnings.len()
            );
            Ok(warnings.len() + report.warnings.len())
        }
        Command::Merge {
            files,
            output,
            by_content,
        } => {
            let mut calendars = Vec::new();
            for f in &files {
                let (source, _, cals) = parse_input(f, &mut warnings)?;
                calendars.extend(cals.into_iter().map(|c| (source.clone(), c)));
            }
            let (cal, report) = merge::merge(&calendars, by_content, &mut warnings);
            let mut out = String::new();
            write::write_component(&mut out, &cal);
            match &output {
                Some(path) => std::fs::write(path, out).map_err(|e| format!("{path}: {e}"))?,
                None => print_out(&out)?,
            }
            for w in &warnings {
                eprintln!("warning: {w}");
            }
            eprintln!(
                "ics-tidy: {} files, {} components in, {} out; removed {} same-UID copies and {} same-content events; {} time zones; {} warnings",
                files.len(),
                report.components,
                report.components - report.uid_duplicates - report.content_duplicates,
                report.uid_duplicates,
                report.content_duplicates,
                report.timezones,
                warnings.len()
            );
            Ok(warnings.len())
        }
    }
}

fn print_out(s: &str) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(s.as_bytes()).and_then(|()| stdout.flush()) {
        Ok(()) => Ok(()),
        // A closed pipe (for example `| head`) is not an error worth reporting.
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(format!("writing output: {e}")),
    }
}
