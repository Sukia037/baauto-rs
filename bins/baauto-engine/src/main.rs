#![forbid(unsafe_code)]
#[cfg(windows)]
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("baauto-engine requires Windows; portable tests run with cargo test.");
}
#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use baauto_config::*;
    use std::{ffi::OsString, path::PathBuf, sync::Arc};
    let mut args = std::env::args_os().skip(1);
    let mut points = PathBuf::from("points/範例.json");
    let mut keys = PathBuf::from("keys/預設按鍵.json");
    let mut hand = None;
    let mut target = OsString::from("BlueArchive.exe");
    let mut max = None;
    let mut dump = None;
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--standalone") => {}
            Some("--points" | "--config") => {
                points = args.next().ok_or("missing points path")?.into()
            }
            Some("--keys") => keys = args.next().ok_or("missing keys path")?.into(),
            Some("--settings") => {
                hand = Some(PathBuf::from(args.next().ok_or("missing settings path")?))
            }
            Some("--target-exe") => target = args.next().ok_or("missing executable name")?,
            Some("--max-run-secs") => {
                max = Some(
                    args.next()
                        .ok_or("missing duration")?
                        .to_str()
                        .ok_or("invalid duration")?
                        .parse::<u64>()?,
                )
            }
            Some("--dump-ring") => {
                dump = Some(PathBuf::from(args.next().ok_or("missing dump path")?))
            }
            Some("--help") => {
                println!(
                    "baauto-engine --standalone --points <JSON> --keys <JSON> [--settings <HandSettings JSON>] [--target-exe <exe>] [--max-run-secs N] [--dump-ring <CSV>]"
                );
                return Ok(());
            }
            _ => return Err("unknown engine argument".into()),
        }
    }
    let p: PointSet = from_json(&std::fs::read_to_string(points)?)?;
    let k: KeysProfile = from_json(&std::fs::read_to_string(keys)?)?;
    let h = if let Some(path) = hand {
        from_json::<HandSettings>(&std::fs::read_to_string(path)?)?
    } else {
        HandSettings::default()
    };
    let clock = baauto_win_platform::Clock::new()?;
    let cfg = compile(&p, &k, &h, clock.rate).map_err(|errors| {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    baauto_win_platform::run_standalone(Arc::new(cfg), &target, max, dump.as_deref())?;
    Ok(())
}
