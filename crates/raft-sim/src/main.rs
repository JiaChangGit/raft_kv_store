use raft_sim::run_campaign;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut seeds = 1000;
    let mut events = 10000;
    let mut output = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--seeds" => {
                seeds = args
                    .next()
                    .expect("seed count")
                    .parse()
                    .expect("integer seeds")
            }
            "--events" => {
                events = args
                    .next()
                    .expect("event count")
                    .parse()
                    .expect("integer events")
            }
            "--output" => output = Some(PathBuf::from(args.next().expect("output path"))),
            _ => panic!("unknown option {flag}"),
        }
    }
    assert!(seeds > 0 && events > 0, "positive campaign bounds required");
    let report = run_campaign(seeds, events);
    let text = serde_json::to_string_pretty(&report).unwrap();
    if let Some(path) = output {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, &text).unwrap();
    }
    println!("{text}");
    if !report.failures.is_empty() {
        std::process::exit(1);
    }
}
