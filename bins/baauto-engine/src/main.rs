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
    let mut managed = false;
    let mut standalone = false;
    let mut shm = None;
    let mut pipe = None;
    let mut parent = None;
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--standalone") => standalone = true,
            Some("--managed") => managed = true,
            Some("--inject") => {
                let text = args
                    .next()
                    .ok_or("missing injection rule")?
                    .into_string()
                    .map_err(|_| "invalid injection text")?;
                #[cfg(feature = "fault-injection")]
                {
                    let rule = text.parse::<baauto_win_platform::fault::FaultRule>()?;
                    baauto_win_platform::fault::RULE
                        .set(rule)
                        .map_err(|_| "injection already configured")?;
                }
                #[cfg(not(feature = "fault-injection"))]
                {
                    let _ = text;
                    return Err("--inject requires a fault-injection build".into());
                }
            }
            Some("--shm") => {
                shm = Some(
                    args.next()
                        .ok_or("missing shm name")?
                        .into_string()
                        .map_err(|_| "invalid shm name")?,
                )
            }
            Some("--pipe") => {
                pipe = Some(
                    args.next()
                        .ok_or("missing pipe name")?
                        .into_string()
                        .map_err(|_| "invalid pipe name")?,
                )
            }
            Some("--parent") => {
                parent = Some(
                    args.next()
                        .ok_or("missing parent PID")?
                        .into_string()
                        .map_err(|_| "invalid parent PID")?
                        .parse::<u32>()?,
                )
            }
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
                    "baauto-engine --standalone --points <JSON> --keys <JSON> [--settings <settings JSON>] [--target-exe <exe>] [--max-run-secs N] [--dump-ring <CSV>]"
                );
                return Ok(());
            }
            _ => return Err("unknown engine argument".into()),
        }
    }
    if !managed && !standalone {
        // Players who double-click the engine get a pointer to the app instead of an
        // unsupervised session without the emergency-stop hotkey.
        println!("這是 BAauto 的引擎，不用直接開。");
        println!("請改開 BAauto 資料夾裡的 baauto-app.exe。");
        println!();
        println!("按 Enter 關閉。");
        let _ = std::io::stdin().read_line(&mut String::new());
        return Ok(());
    }
    if managed {
        return baauto_win_platform::run_managed(
            &shm.ok_or("missing --shm")?,
            &pipe.ok_or("missing --pipe")?,
            parent.ok_or("missing --parent")?,
            &target,
        )
        .map_err(Into::into);
    }
    let p: PointSet = from_json(&baauto_config::store::decode_json(&std::fs::read(points)?)?)?;
    let k: KeysProfile = from_json(&baauto_config::store::decode_json(&std::fs::read(keys)?)?)?;
    let (h, params) = if let Some(path) = hand {
        baauto_config::store::read_machine(&path)?
    } else {
        (HandSettings::default(), EngineParams::default())
    };
    let clock = baauto_win_platform::Clock::new()?;
    let cfg = compile_with_params(&p, &k, &h, clock.rate, params, ConfigVersion(1)).map_err(
        |errors| {
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        },
    )?;
    baauto_win_platform::run_standalone(Arc::new(cfg), &target, max, dump.as_deref())?;
    Ok(())
}
