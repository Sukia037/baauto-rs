#![forbid(unsafe_code)]
#[cfg(not(windows))]
fn main() {
    eprintln!("baauto-app engine supervision requires Windows.");
}
#[cfg(windows)]
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use baauto_config::*;
    use baauto_win_platform::*;
    use std::{
        ffi::OsString,
        io::{self, BufRead},
        path::PathBuf,
        sync::{Arc, mpsc},
        thread,
    };
    let mut points = PathBuf::from("points/範例.json");
    let mut keys = PathBuf::from("keys/預設按鍵.json");
    let mut hand_path = None;
    let mut target = OsString::from("BlueArchive.exe");
    let mut engine = std::env::current_exe()?.with_file_name("baauto-engine.exe");
    let mut health = 1000;
    let mut recovery = 1000;
    let mut injection = None;
    let mut enable = true;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--headless") => {}
            Some("--profile" | "--points") => {
                points = args.next().ok_or("missing points path")?.into()
            }
            Some("--keys") => keys = args.next().ok_or("missing keys path")?.into(),
            Some("--settings") => {
                hand_path = Some(PathBuf::from(args.next().ok_or("missing settings path")?))
            }
            Some("--target-exe") => target = args.next().ok_or("missing target executable")?,
            Some("--engine") => engine = args.next().ok_or("missing engine path")?.into(),
            Some("--health-timeout-ms") => {
                health = args
                    .next()
                    .ok_or("missing health timeout")?
                    .into_string()
                    .map_err(|_| "invalid timeout")?
                    .parse()?
            }
            Some("--recovery-timeout-ms") => {
                recovery = args
                    .next()
                    .ok_or("missing recovery timeout")?
                    .into_string()
                    .map_err(|_| "invalid timeout")?
                    .parse()?
            }
            Some("--inject") => {
                injection = Some(
                    args.next()
                        .ok_or("missing injection rule")?
                        .into_string()
                        .map_err(|_| "invalid injection")?,
                )
            }
            Some("--disarmed") => enable = false,
            Some("--help") => {
                println!(
                    "baauto-app --headless --profile <points JSON> --keys <keys JSON> [--settings <HandSettings JSON>] [--target-exe <exe>] [--engine <path>] [--disarmed] [--health-timeout-ms N] [--recovery-timeout-ms N]\nCommands: enable, stop, profile <path>, keys <path>, settings <path>, rehook, quit"
                );
                return Ok(());
            }
            _ => return Err("unknown app argument".into()),
        }
    }
    let mut p: PointSet = from_json(&std::fs::read_to_string(&points)?)?;
    let mut k: KeysProfile = from_json(&std::fs::read_to_string(&keys)?)?;
    let mut h = if let Some(path) = hand_path {
        from_json::<HandSettings>(&std::fs::read_to_string(path)?)?
    } else {
        HandSettings::default()
    };
    let clock = Clock::new()?;
    let cfg = compile(&p, &k, &h, clock.rate).map_err(|e| {
        e.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let options = SupervisorOptions {
        cfg: Arc::new(cfg),
        engine,
        target,
        stop_hotkey: k.presets["global_stop"]
            .hotkey
            .ok_or("missing emergency hotkey")?,
        record_hotkey: k.presets["record"].hotkey.ok_or("missing record hotkey")?,
        health_timeout_ms: health,
        recovery_timeout_ms: recovery,
        logs: PathBuf::from("logs"),
        injection,
    };
    let (tx, rx) = mpsc::channel();
    let (status_tx, status_rx) = mpsc::channel();
    let status_worker = thread::spawn(move || {
        while let Ok(s) = status_rx.recv() {
            let s: SupervisorStatus = s;
            eprintln!("{:?} session={} {}", s.state, s.session.0, s.detail);
        }
    });
    if enable {
        tx.send(SupervisorCommand::Enable)?;
    }
    let stdin_sender = tx.clone();
    thread::spawn(move || {
        let mut version = 1u32;
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            let text = line.trim();
            let command = match text {
                "enable" | "arm" => Some(SupervisorCommand::Enable),
                "stop" => Some(SupervisorCommand::Disable),
                "rehook" => Some(SupervisorCommand::Rehook),
                "quit" => {
                    let _ = stdin_sender.send(SupervisorCommand::Quit);
                    return;
                }
                _ => {
                    let result = (|| -> Result<Arc<CompiledConfig>, Box<dyn std::error::Error>> {
                        let (kind, path) = text
                            .split_once(' ')
                            .ok_or("expected command and file path")?;
                        let path = PathBuf::from(path.trim().trim_matches('"'));
                        match kind {
                            "profile" => p = from_json(&std::fs::read_to_string(path)?)?,
                            "keys" => k = from_json(&std::fs::read_to_string(path)?)?,
                            "settings" => h = from_json(&std::fs::read_to_string(path)?)?,
                            _ => return Err("unknown command".into()),
                        }
                        version = version.checked_add(1).ok_or("version exhausted")?;
                        let mut c = compile(&p, &k, &h, clock.rate).map_err(|e| {
                            e.iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })?;
                        c.version = ConfigVersion(version);
                        Ok(Arc::new(c))
                    })();
                    match result {
                        Ok(c) => {
                            match (k.presets["global_stop"].hotkey, k.presets["record"].hotkey) {
                                (Some(stop_hotkey), Some(record_hotkey)) => {
                                    Some(SupervisorCommand::Apply {
                                        cfg: c,
                                        stop_hotkey,
                                        record_hotkey,
                                    })
                                }
                                _ => {
                                    eprintln!("emergency and record hotkeys must be assigned");
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("{e}");
                            None
                        }
                    }
                }
            };
            if let Some(command) = command
                && stdin_sender.send(command).is_err()
            {
                return;
            }
        }
        let _ = stdin_sender.send(SupervisorCommand::Quit);
    });
    let outcome = run_supervisor(options, rx, status_tx);
    drop(tx);
    let _ = status_worker.join();
    outcome.map_err(Into::into)
}
