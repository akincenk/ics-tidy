# Roadmap

Planned work, roughly in order. Each item has a GitHub issue with the label `roadmap`. Estimates are hours of focused work including tests and docs; about 30 hours in total.

1. **Windows time zone names** (3 h). Outlook and Exchange write `TZID:Pacific Standard Time`. Map Windows names to IANA zones with the CLDR `windowsZones.xml` table (bundled, with its license), so these events are converted instead of read as local time with a warning. Found while testing with `examples/home.ics` (the "Car service" event).
2. **Custom VTIMEZONE definitions** (3 h). When a `TZID` is not an IANA or Windows name, build the offsets from the file's own `VTIMEZONE` (`STANDARD`/`DAYLIGHT` with `RRULE` or `RDATE`) and use them for conversion.
3. **`RECURRENCE-ID;RANGE=THISANDFUTURE`** (2 h). An override with this range changes the given instance and all later ones (RFC 5545 3.8.4.4). Today only the single instance is replaced.
4. **DTSTART that does not match its rule** (1.5 h). RFC 5545 3.8.5.3 says DTSTART is always the first instance. With `DTSTART:20261124T100000Z` (a Tuesday) and `RRULE:FREQ=WEEKLY;BYDAY=MO;COUNT=2` the agenda currently shows only the Mondays. Add DTSTART as an instance, check how COUNT should count it, and document the choice.
5. **Cancelled events: honest count and an option to show them** (1.5 h). The summary line counts cancelled events even when they are outside the range (with `--from 2026-11-29` on the examples it still says "1 cancelled skipped"). Count only those in range, and add `--include-cancelled` that shows them marked `[cancelled]`.
6. **`--strict` and empty input** (1.5 h). Input that contains no VCALENDAR at all (for example a HTML error page saved as `.ics`) gives exit 0 and an empty agenda. Make that an error, and add `--strict` that exits 1 when there were any warnings, for use in scripts.
7. **Entries that start before the range** (2 h). An all-day event from Saturday to Sunday, listed with `--from` Sunday, appears under a "Sat" heading that is outside the requested range. Show it under the first day of the range with "(since Sat ...)", and add `--per-day` to repeat multi-day entries on each day they cover.
8. **JSON output** (2 h). `--format json` with one object per entry: start and end with UTC offset, all-day flag, summary, location, calendars, UID, and the source file. Document the schema in the README.
9. **Filters** (2 h). `--calendar NAME` (repeatable), `--search TEXT` on summary and location, `--all-day only|none`.
10. **Tidier merge output** (2.5 h). Sort components by start, `--name` to set `X-WR-CALNAME`, keep `METHOD:PUBLISH` when every input has it, and leave out `VTIMEZONE`s that no kept component uses.
11. **To-dos** (2 h). `--todos` adds `VTODO` items with a `DUE` date in range to the agenda, marked done or open.
12. **Calendar folders** (2 h). Accept a directory and read every `.ics` file in it, including vdir folders with one event per file (the layout used by vdirsyncer and khal). The calendar name is the folder name.
13. **Fuzzing and round-trip properties** (2.5 h). A `cargo-fuzz` target for the reader, and property tests that unfold(fold(line)) is the identity and that parse(write(c)) == c for generated components. Run the fuzz target in CI for a short time on each pull request.
14. **Release binaries** (2.5 h). A release workflow that builds macOS (arm64, x86_64), Linux and Windows binaries with SHA-256 checksums and build provenance, and README install steps for them.
