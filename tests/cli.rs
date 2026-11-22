use std::process::Command;

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_ics-tidy"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("binary runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

const FILES: [&str; 3] = [
    "examples/work.ics",
    "examples/home.ics",
    "examples/team-shared.ics",
];

#[test]
fn agenda_of_the_example_week() {
    let mut args = vec![
        "agenda",
        "--from",
        "2026-11-23",
        "--days",
        "7",
        "--tz",
        "America/Vancouver",
    ];
    args.extend(FILES);
    let (code, out, err) = run(&args);
    assert_eq!(code, 0, "{err}");
    for line in [
        "Tue 2026-11-24",
        "  08:00-08:45    Supplier call (Izmir plant)  [work]",
        "  06:30-07:30    Gym  [home]",
        "  15:00-16:00    Dentist  [work, home]",
        "  07:00-07:15    Warehouse standup (moved for delivery) (Dock 3 office)  [work]",
        "  22:00-02:00+1d Night delivery window (help a friend move)  [home]",
        "  all day        Quarterly stock take (until Sun 2026-11-29)  [work]",
    ] {
        assert!(out.contains(line), "missing {line:?} in\n{out}");
    }
    assert!(
        !out.contains("Wed 2026-11-25\n  08:30"),
        "EXDATE instance shown"
    );
    assert!(!out.contains("Forklift"), "cancelled event shown");
    assert_eq!(out.matches("Warehouse standup").count(), 2, "{out}");
    assert!(
        err.contains("unknown time zone \"Pacific Standard Time\""),
        "{err}"
    );
    assert!(
        err.contains("removed 1 same-UID copies and 1 same title-and-time entries"),
        "{err}"
    );
}

#[test]
fn csv_output() {
    let (code, out, _) = run(&[
        "agenda",
        "--from",
        "2026-11-26",
        "--to",
        "2026-11-26",
        "--tz",
        "America/Vancouver",
        "--format",
        "csv",
        FILES[0],
        FILES[1],
    ]);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "start,end,all_day,summary,location,calendar,uid\r\n\
         2026-11-26T06:30,2026-11-26T07:30,false,Gym,,home,B1E3C0DE-0005@home.example\r\n\
         2026-11-26T15:00,2026-11-26T16:00,false,Dentist,,\"work, home\",dentist-shared@example.com\r\n"
    );
}

#[test]
fn merge_keeps_one_copy_and_reads_back_the_same() {
    let dir = std::env::temp_dir().join(format!("ics-tidy-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let merged = dir.join("merged.ics");
    let merged = merged.to_str().unwrap();
    let mut args = vec!["merge", "--by-content", "-o", merged];
    args.extend(FILES);
    let (code, _, err) = run(&args);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("15 components in, 13 out"), "{err}");
    let text = std::fs::read_to_string(merged).unwrap();
    assert_eq!(text.matches("BEGIN:VEVENT").count(), 13);
    assert_eq!(text.matches("BEGIN:VTIMEZONE").count(), 1);
    assert!(text.lines().all(|l| l.len() <= 75));
    assert!(text.contains("SEQUENCE:2"), "newer standup kept");

    let (_, from_merged, _) = run(&[
        "agenda",
        "--from",
        "2026-11-23",
        "--days",
        "7",
        "--tz",
        "America/Vancouver",
        merged,
    ]);
    let mut args = vec![
        "agenda",
        "--from",
        "2026-11-23",
        "--days",
        "7",
        "--tz",
        "America/Vancouver",
    ];
    args.extend(FILES);
    let (_, direct, _) = run(&args);
    let strip = |s: &str| {
        s.lines()
            .map(|l| l.split("  [").next().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(&from_merged), strip(&direct));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(run(&["agenda", "missing.ics"]).0, 2);
    assert_eq!(run(&["agenda", "--tz", "Mars/Base", FILES[0]]).0, 2);
    assert_eq!(
        run(&[
            "agenda",
            "--from",
            "2026-11-23",
            "--to",
            "2026-11-01",
            FILES[0]
        ])
        .0,
        2
    );
    assert_eq!(run(&["agenda"]).0, 2);
}
