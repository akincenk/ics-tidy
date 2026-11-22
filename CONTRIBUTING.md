# Contributing

Thanks for taking a look. Issues and pull requests are welcome.

## Before you start

- For anything bigger than a small fix, open an issue first so we can agree on the approach.
- Planned work is in [ROADMAP.md](ROADMAP.md); each item has a GitHub issue.
- A bug report is most useful with a small `.ics` file that shows the problem. Remove real names, addresses and meeting links first.

## Working on the code

You need Rust 1.88 or newer.

```console
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run -- agenda --from 2026-11-23 --tz America/Vancouver examples/*.ics
```

Layout:

- `src/parse.rs`: line unfolding, content lines, parameters, the component tree.
- `src/write.rs`: writing components back with 75-octet folding.
- `src/event.rs`: DATE and DATE-TIME values, durations, VEVENT fields.
- `src/agenda.rs`: recurrence expansion, overrides, duplicate removal.
- `src/merge.rs`: merging calendars.
- `src/render.rs`: text and CSV output.
- `src/main.rs`: the command-line tool. `tests/cli.rs` runs it on the files in `examples/`.

## Tests

- Every behaviour change needs a test. Bug fixes start with a test that fails on the old code.
- Use fixed dates and zones whose rules for those dates are stable across tz database versions (for example America/Toronto or Europe/Berlin in 2026). Do not depend on the current date or the computer's zone; pass `--from` and `--tz`.
- Cite the RFC 5545 section in a comment when a test pins down behaviour the standard defines.

## Pull requests

- One topic per pull request, with a short description of what changed and why.
- CI must be green: tests on Linux, macOS and Windows, the minimum Rust version, clippy, rustfmt, gitleaks and zizmor.
- Do not commit secrets or real personal data. Example calendars use made-up names and `example.com` addresses.

## License

By contributing you agree that your work is released under the MIT license of this project.
