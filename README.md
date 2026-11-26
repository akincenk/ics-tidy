# ics-tidy

[![CI](https://github.com/akincenk/ics-tidy/actions/workflows/ci.yml/badge.svg)](https://github.com/akincenk/ics-tidy/actions/workflows/ci.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/akincenk/ics-tidy/badge)](https://scorecard.dev/viewer/?uri=github.com/akincenk/ics-tidy)

A Rust command-line tool and library for iCalendar (`.ics`, RFC 5545) files. It reads several calendar exports at once and:

- prints one agenda for a date range, with recurring events expanded and every time shown in the zone you choose,
- writes a CSV of the same entries for a spreadsheet,
- merges the files into one `.ics` with each event kept once.

```console
$ ics-tidy agenda --from 2026-11-26 --to 2026-11-27 --tz America/Vancouver examples/*.ics
Thu 2026-11-26
  06:30-07:30    Gym  [home]
  15:00-16:00    Dentist  [work, home]

Fri 2026-11-27
  07:00-07:15    Warehouse standup (moved for delivery) (Dock 3 office)  [work]
  16:00-17:00    Month-end inventory report due  [work]
  22:00-02:00+1d Night delivery window (help a friend move)  [home]
warning: examples/home.ics: VEVENT B1E3C0DE-0004@home.example: unknown time zone "Pacific Standard Time", read as local time
ics-tidy: 3 files, 15 events, 5 entries from 2026-11-26 to 2026-11-27 (America/Vancouver); removed 1 same-UID copies and 1 same title-and-time entries; 1 cancelled skipped; 1 warnings
```

The calendars in `examples/` are made up. The agenda goes to standard output; warnings and the summary line go to standard error, so `> agenda.txt` keeps only the agenda.

## Who it is for

People who have the same week spread over several calendars (work, home, a shared team calendar) and want one clean list, without signing in to anything: for printing a week, checking a schedule in a script, or loading events into a spreadsheet. Also for anyone who exported a calendar twice and now has every event two times.

## What it does

- **Recurrence**: `RRULE`, `RDATE` and `EXDATE`, expanded with the [rrule](https://crates.io/crates/rrule) crate. Instances changed in place (`RECURRENCE-ID`) replace the original instance; cancelled instances (`STATUS:CANCELLED`) are left out. A `DTSTART` that the rule would not produce (a Friday start on a Tuesday/Thursday rule) is still shown as the first instance and counts toward `COUNT`, as RFC 5545 3.8.5.3 says, with a warning: the RFC calls such a set undefined, so other programs may show it differently.
- **Time zones**: `TZID` values are IANA names (`America/Vancouver`, also `/Europe/Berlin`). Times are converted to the `--tz` zone, which defaults to this computer's zone. Floating times (no zone) and all-day dates are shown as written. A time inside a daylight saving gap uses the offset from before the gap, and a repeated time means the first one, as RFC 5545 section 3.3.5 says.
- **Lenient `UNTIL`**: RFC 5545 wants `UNTIL` in UTC when the start has a zone, but many calendars write `UNTIL=20261231`. Such a date is read as the end of that day in the event's zone instead of dropping the rule.
- **Duplicates, same UID**: events with the same `UID` and `RECURRENCE-ID` (the same event imported twice, or a shared calendar that also appears in your own) are kept once: the highest `SEQUENCE` wins, then the most recent `LAST-MODIFIED` or `DTSTAMP`, then the first file named.
- **Duplicates, same content**: in the agenda, entries with the same title (ignoring case and extra spaces), start and end are shown once with all their calendars listed, as `[work, home]` above. `--keep-duplicates` turns this off. `merge --by-content` drops such events from the merged file too.
- **Forgiving reader**: bad lines, unclosed components, unknown zones and rules that cannot be read become warnings with file and line number where possible; the rest of the file is still used. A file with no `VCALENDAR` is an error (see Exit status).

### Not done yet

These are planned (see [ROADMAP.md](ROADMAP.md)): Windows zone names such as `Pacific Standard Time` (read as local time for now, with a warning), custom `VTIMEZONE` definitions that are not IANA zones, `RANGE=THISANDFUTURE`, to-dos, JSON output and filters.

## Stack

Rust 2024 edition, Rust 1.88 or newer. Dependencies: `rrule` (recurrence), `chrono` and `chrono-tz` (dates and the IANA zone database built into the binary), `clap` (command line), `iana-time-zone` (finding this computer's zone). The iCalendar reader and writer (line unfolding and folding, parameters, escapes, components) are written in this crate.

## How to run

Install Rust from <https://rustup.rs>, then:

```console
git clone https://github.com/akincenk/ics-tidy
cd ics-tidy
cargo install --path .
ics-tidy agenda --from 2026-11-23 --days 7 --tz America/Vancouver examples/*.ics
```

Or without installing: `cargo run -- agenda examples/work.ics`.

### Agenda

```console
ics-tidy agenda [--from YYYY-MM-DD] [--to YYYY-MM-DD | --days N] [--tz ZONE] [--format text|csv] [--keep-duplicates] FILE...
```

- `--from` defaults to today in the `--tz` zone; `--days` defaults to 7; `--to` is inclusive.
- `-` as a file name reads standard input. The calendar name shown in brackets is the file name without `.ics`.
- An entry is listed when any part of it falls inside the range.

CSV columns are `start,end,all_day,summary,location,calendar,uid`. Timed entries use `2026-11-26T15:00` with the end time exclusive; all-day entries use dates with the last day inclusive (a two-day stock take is `2026-11-28,2026-11-29`). Text that a spreadsheet would run as a formula (starting with `=`, `+`, `-` or `@`) gets a leading apostrophe, because calendar files come from other people.

```console
$ ics-tidy agenda --from 2026-11-26 --to 2026-11-26 --tz America/Vancouver --format csv examples/work.ics examples/home.ics
start,end,all_day,summary,location,calendar,uid
2026-11-26T06:30,2026-11-26T07:30,false,Gym,,home,B1E3C0DE-0005@home.example
2026-11-26T15:00,2026-11-26T16:00,false,Dentist,,"work, home",dentist-shared@example.com
```

### Merge

```console
ics-tidy merge [-o OUT.ics] [--by-content] FILE...
```

Writes one `VCALENDAR` with `VERSION`, `PRODID` and `CALSCALE`, one copy of each `VTIMEZONE` (a warning if two files define the same zone differently), then every other component in the order read. All properties of the kept components are written back unchanged, with lines folded at 75 octets. Calendar-level properties of the inputs (`X-WR-CALNAME`, `METHOD`) are not copied.

### Exit status

0 on success, also when there were warnings. 1 with `--strict` when there was at least one warning (the output is still written, so a script can keep it or throw it away). 2 for usage errors, files that cannot be read, and input with no `VCALENDAR` at all, such as an empty file or an HTML login page saved as `.ics`; nothing is written in that case. A calendar that has a `VCALENDAR` but no events is fine.

```sh
ics-tidy agenda --strict --days 1 work.ics home.ics > today.txt || echo "check the warnings"
```

### As a library

```rust
use ics_tidy::{agenda, events_in, parse};

let text = std::fs::read_to_string("examples/work.ics")?;
let (calendars, mut warnings) = parse::parse(&text, "work.ics");
let events = events_in(&calendars, "work.ics", "work", &mut warnings);
let day = |d| chrono::NaiveDate::from_ymd_opt(2026, 11, d).unwrap().and_hms_opt(0, 0, 0).unwrap();
let range = agenda::Range { from: day(23), to: day(30) };
let (entries, report) = agenda::agenda(events, range, chrono_tz::Tz::America__Vancouver, false);
```

Modules: `parse` (reader), `write` (writer), `event` (dates, durations, VEVENT fields), `agenda` (expansion and duplicates), `merge`, `render` (text and CSV).

## Similar projects

- [khal](https://github.com/pimutils/khal) (Python) is a full calendar program with an agenda view over a local calendar folder; ics-tidy is a single binary that works directly on `.ics` exports and also merges them.
- Several small `ics-to-csv` scripts convert one file to CSV, usually without recurrence expansion or duplicate removal.
- Crates [icalendar](https://crates.io/crates/icalendar) and [ical](https://crates.io/crates/ical) read and write iCalendar in Rust; [rrule](https://crates.io/crates/rrule) expands rules and is used here.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Planned work is in [ROADMAP.md](ROADMAP.md), one GitHub issue per item.

## License

MIT, see [LICENSE](LICENSE).
