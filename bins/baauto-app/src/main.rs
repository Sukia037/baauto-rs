#![forbid(unsafe_code)]
// Double-clicking the app must not open a console window; --headless and --help attach to the terminal.
#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(not(windows))]
fn main() {
    if let Err(e) = run_gui() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
#[cfg(windows)]
fn main() {
    let console = std::env::args_os().any(|a| a == "--headless" || a == "--help");
    if console {
        baauto_win_platform::attach_parent_console();
    }
    if std::env::args_os().any(|a| a == "--headless") {
        if let Err(e) = run() {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    // A double-clicked GUI has no console, so failures go to a dialog and logs\app-error.txt.
    let error_log = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("logs").join("app-error.txt")));
    if let Some(path) = error_log.clone() {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !baauto_win_platform::on_hook_thread() {
                baauto_win_platform::append_error_log(&path, &format!("panic: {info}"));
            }
            default(info);
        }));
    }
    let preview = std::env::args_os().any(|a| a == "--preview");
    let engine_found = baauto_win_platform::engine_path().is_ok_and(|p| p.is_file());
    let result = if !console && !preview && !engine_found {
        Err(baauto_win_platform::startup::MISSING_ENGINE.into())
    } else {
        run_gui()
    };
    if let Err(e) = result {
        eprintln!("{e}");
        if let Some(path) = &error_log {
            baauto_win_platform::append_error_log(path, &format!("error: {e}"));
        }
        if !console {
            baauto_win_platform::error_dialog(
                "BAauto 無法開啟",
                &format!("{e}\n\n這段訊息也寫在 logs\\app-error.txt，回報問題時請附上。"),
            );
        }
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
    let data_root = std::env::current_exe()?
        .parent()
        .ok_or("app directory missing")?
        .to_path_buf();
    let (store, warnings) = baauto_config::store::FileStore::initialize(&data_root, WindowsFs)?;
    for warning in warnings {
        eprintln!("{warning}");
    }
    let mut points = store.settings.current_points.clone();
    let mut keys = store.settings.current_keys.clone();
    let mut hand_path = None;
    let mut target = OsString::from("BlueArchive.exe");
    let mut engine = baauto_win_platform::engine_path()?;
    let mut health = 1000;
    let mut recovery = 1000;
    let mut injection = None;
    let mut enable = true;
    let mut mode = baauto_protocol::ipc::RunMode::Normal;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--headless") => {}
            Some("--calibration") => mode = baauto_protocol::ipc::RunMode::Calibration,
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
                    "baauto-app --headless --profile <points JSON> --keys <keys JSON> [--settings <settings JSON>] [--target-exe <exe>] [--engine <path>] [--disarmed] [--health-timeout-ms N] [--recovery-timeout-ms N]\nCommands: enable, stop, profile <path>, keys <path>, settings <path>, rehook, quit"
                );
                return Ok(());
            }
            _ => return Err("unknown app argument".into()),
        }
    }
    let mut p: PointSet = from_json(&baauto_config::store::decode_json(&std::fs::read(
        &points,
    )?)?)?;
    let mut k: KeysProfile =
        from_json(&baauto_config::store::decode_json(&std::fs::read(&keys)?)?)?;
    let (mut h, mut params) = if let Some(path) = hand_path {
        baauto_config::store::read_machine(&path)?
    } else {
        (store.settings.hand, store.settings.engine)
    };
    let clock = Clock::new()?;
    let cfg =
        compile_with_params(&p, &k, &h, clock.rate, params, ConfigVersion(1)).map_err(|e| {
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
        record_hotkey: k.presets["record"]
            .enabled
            .then_some(k.presets["record"].hotkey)
            .flatten(),
        health_timeout_ms: health,
        recovery_timeout_ms: recovery,
        logs: PathBuf::from("logs"),
        injection,
        mode,
    };
    let (tx, rx) = mpsc::channel();
    let (status_tx, status_rx) = mpsc::channel();
    let status_worker = thread::spawn(move || {
        while let Ok(s) = status_rx.recv() {
            let s: SupervisorStatus = s;
            eprintln!("{:?} session={} {}", s.state, s.session.0, s.detail);
            if let Some(warning) = &s.hotkey_warning {
                eprintln!("警告：{warning}");
            }
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
                        let (mut next_p, mut next_k, mut next_h, mut next_params) =
                            (p.clone(), k.clone(), h.clone(), params);
                        match kind {
                            "profile" => {
                                next_p = from_json(&baauto_config::store::decode_json(
                                    &std::fs::read(path)?,
                                )?)?
                            }
                            "keys" => {
                                next_k = from_json(&baauto_config::store::decode_json(
                                    &std::fs::read(path)?,
                                )?)?
                            }
                            "settings" => {
                                (next_h, next_params) = baauto_config::store::read_machine(&path)?
                            }
                            _ => return Err("unknown command".into()),
                        }
                        let next_version = version.checked_add(1).ok_or("version exhausted")?;
                        let mut c = compile_with_params(
                            &next_p,
                            &next_k,
                            &next_h,
                            clock.rate,
                            next_params,
                            ConfigVersion(next_version),
                        )
                        .map_err(|e| {
                            e.iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })?;
                        if next_k.presets["global_stop"].hotkey.is_none() {
                            return Err("emergency and record hotkeys must be assigned".into());
                        }
                        (p, k, h, params) = (next_p, next_k, next_h, next_params);
                        version = next_version;
                        c.version = ConfigVersion(version);
                        Ok(Arc::new(c))
                    })();
                    match result {
                        Ok(c) => {
                            match (k.presets["global_stop"].hotkey, k.presets["record"].hotkey) {
                                (Some(stop_hotkey), record_hotkey) => {
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

fn run_gui() -> Result<(), Box<dyn std::error::Error>> {
    use eframe::egui;
    // winit 0.30.13 event_loop.rs enables PMv2 before creating windows; explicitly set through safe win-platform wrapper first.
    #[cfg(windows)]
    baauto_win_platform::set_dpi_awareness()?;
    let mut root = std::env::current_exe()?
        .parent()
        .ok_or("app directory missing")?
        .to_path_buf();
    let mut preview = !cfg!(windows);
    let mut target = std::ffi::OsString::from("BlueArchive.exe");
    let mut qa_dir = None;
    let mut qa_dark = false;
    let mut qa_scale = 1.0f32;
    let mut qa_tab = 0u8;
    let mut qa_hotkey_warning = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--preview") => preview = true,
            Some("--data-dir") => root = args.next().ok_or("data directory missing")?.into(),
            Some("--target-exe") => target = args.next().ok_or("target executable missing")?,
            Some("--qa-dir") => {
                qa_dir = Some(std::path::PathBuf::from(
                    args.next().ok_or("QA directory missing")?,
                ))
            }
            Some("--qa-dark") => qa_dark = true,
            Some("--qa-hotkey-warning") => qa_hotkey_warning = true,
            Some("--qa-scale") => {
                qa_scale = args
                    .next()
                    .ok_or("scale missing")?
                    .to_str()
                    .ok_or("scale invalid")?
                    .parse()?
            }
            Some("--qa-tab") => {
                qa_tab = args
                    .next()
                    .ok_or("tab missing")?
                    .to_str()
                    .ok_or("tab invalid")?
                    .parse()?
            }
            Some("--help") => {
                println!(
                    "baauto-app [--preview] [--data-dir PATH] [--target-exe EXE]\nUse --headless for console supervision.\nQA: --qa-dir PATH [--qa-dark] [--qa-scale 1|1.5|2] [--qa-tab 0..4] [--qa-hotkey-warning]"
                );
                return Ok(());
            }
            _ => return Err("unknown GUI option; use --headless for console options".into()),
        }
    }
    if qa_tab > 4 || ![1.0, 1.5, 2.0].contains(&qa_scale) {
        return Err("invalid QA tab/scale".into());
    }
    let qa = qa_dir.map(|p| (p, qa_dark, qa_scale, qa_tab));
    let backend = baauto_app::backend::Backend::start(root, preview, target);
    let options = eframe::NativeOptions {
        // persist_window controls saving only; isolate loading too so QA uses the requested size.
        persist_window: qa.is_none(),
        persistence_path: qa
            .as_ref()
            .map(|_| std::env::temp_dir().join(format!("baauto-qa-window-{}", std::process::id()))),
        viewport: egui::ViewportBuilder::default()
            .with_title("BAauto")
            .with_inner_size([1000. * qa_scale, 660. * qa_scale])
            .with_min_inner_size([900., 560.]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "BAauto",
        options,
        Box::new(move |cc| {
            let mut app = baauto_app::gui::App::new(cc, backend, preview, qa);
            if preview && qa_hotkey_warning {
                app.demo_hotkey_warning();
            }
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}
