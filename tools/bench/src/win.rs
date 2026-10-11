//! Bench/file workers may wait and allocate. They never send game input directly.
use crate::*;
use baauto_config::{store::*, *};
use baauto_core::RunMode;
use baauto_protocol::{
    ipc::*,
    shm::{LogData, RingReader},
};
use baauto_win_platform::*;
use std::{
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering::SeqCst},
        mpsc,
    },
    thread,
    time::Duration,
};
struct ManagedBench {
    tx: mpsc::Sender<SupervisorCommand>,
    status: mpsc::Receiver<SupervisorStatus>,
    join: Option<thread::JoinHandle<windows::core::Result<()>>>,
}
impl ManagedBench {
    fn start(options: SupervisorOptions) -> Result<Self, Error> {
        let (tx, rx) = mpsc::channel();
        let (status_tx, status) = mpsc::channel();
        let join = thread::spawn(move || run_supervisor(options, rx, status_tx));
        let managed = Self {
            tx,
            status,
            join: Some(join),
        };
        managed
            .tx
            .send(SupervisorCommand::Enable)
            .map_err(|e| e.to_string())?;
        let mut armed = false;
        for _ in 0..300 {
            if let Ok(s) = managed.status.recv_timeout(Duration::from_millis(100)) {
                if matches!(s.state, baauto_protocol::supervisor::State::Armed(_)) {
                    armed = true;
                    break;
                }
                if s.state == baauto_protocol::supervisor::State::Faulted {
                    return Err(s.detail);
                }
            }
        }
        if !armed {
            return Err("game/receiver must be foreground; Arm timed out".into());
        }
        Ok(managed)
    }
    fn arm(&self) -> Result<(), Error> {
        self.tx
            .send(SupervisorCommand::Enable)
            .map_err(|e| e.to_string())?;
        for _ in 0..300 {
            if let Ok(s) = self.status.recv_timeout(Duration::from_millis(100)) {
                if matches!(s.state, baauto_protocol::supervisor::State::Armed(_)) {
                    return Ok(());
                }
                if s.state == baauto_protocol::supervisor::State::Faulted {
                    return Err(s.detail);
                }
            }
        }
        Err("Arm timed out during stop sampling".into())
    }
    fn inspect(&self) -> Result<(Arc<Mapping>, Arc<Process>), Error> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(SupervisorCommand::Inspect { reply: tx })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())
    }
    fn stop(&self) -> Result<(), Error> {
        self.tx
            .send(SupervisorCommand::Disable)
            .map_err(|e| e.to_string())?;
        for _ in 0..100 {
            if let Ok(s) = self.status.recv_timeout(Duration::from_millis(100)) {
                if s.state == baauto_protocol::supervisor::State::Disarmed {
                    return Ok(());
                }
                if s.state == baauto_protocol::supervisor::State::Faulted {
                    return Err(s.detail);
                }
            }
        }
        Err("stop acknowledgment timed out".into())
    }
}
impl Drop for ManagedBench {
    fn drop(&mut self) {
        let _ = self.tx.send(SupervisorCommand::Quit);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
pub struct BenchOptions {
    pub engine: PathBuf,
    pub target: OsString,
    pub settings: Option<PathBuf>,
    pub presentmon: Option<PathBuf>,
    pub output: PathBuf,
    pub stop_cycles: u32,
}
fn configuration(
    points: &Path,
    keys: &Path,
    settings: Option<&Path>,
    clock: Clock,
) -> Result<
    (
        PointSet,
        KeysProfile,
        HandSettings,
        EngineParams,
        CompiledConfig,
    ),
    Error,
> {
    let p = match read_document(points).map_err(|e| e.to_string())? {
        Document::Points(p) => p,
        _ => return Err("expected point document".into()),
    };
    let k = match read_document(keys).map_err(|e| e.to_string())? {
        Document::Keys(k) => *k,
        _ => return Err("expected key document".into()),
    };
    let (h, params) = if let Some(s) = settings {
        read_machine(s).map_err(|e| e.to_string())?
    } else {
        (HandSettings::default(), EngineParams::default())
    };
    let cfg = compile_with_params(&p, &k, &h, clock.rate, params, ConfigVersion(1))
        .map_err(|e| format!("{e:?}"))?;
    Ok((p, k, h, params, cfg))
}
fn options(
    cfg: CompiledConfig,
    keys: &KeysProfile,
    bench: &BenchOptions,
    mode: RunMode,
) -> Result<SupervisorOptions, Error> {
    Ok(SupervisorOptions {
        cfg: Arc::new(cfg),
        engine: bench.engine.clone(),
        target: bench.target.clone(),
        stop_hotkey: keys.presets["global_stop"]
            .hotkey
            .ok_or("emergency hotkey missing")?,
        record_hotkey: keys.presets["record"]
            .hotkey
            .ok_or("record hotkey missing")?,
        health_timeout_ms: 1000,
        recovery_timeout_ms: 1000,
        logs: bench.output.join("faults"),
        injection: None,
        mode,
    })
}
fn create_file(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())
}
struct PresentMon {
    child: std::process::Child,
    exe: PathBuf,
    session: String,
}
impl PresentMon {
    fn start(exe: &Path, target: &OsStr, output: &Path, seconds: u64) -> Result<Self, Error> {
        let session = format!(
            "BAauto-{}-{}",
            std::process::id(),
            Clock::new().map_err(|e| e.to_string())?.now().0
        );
        let child = std::process::Command::new(exe)
            .args(["--session_name", &session, "--process_name"])
            .arg(target)
            .args(["--qpc_time", "--no_console_stats", "--output_file"])
            .arg(output)
            .args(["--timed", &seconds.to_string(), "--terminate_after_timed"])
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            child,
            exe: exe.to_owned(),
            session,
        })
    }
    fn terminate(&mut self) -> Result<(), Error> {
        let mut stopper = std::process::Command::new(&self.exe)
            .args([
                "--session_name",
                &self.session,
                "--terminate_existing_session",
            ])
            .spawn()
            .map_err(|e| e.to_string())?;
        let clock = Clock::new().map_err(|e| e.to_string())?;
        let deadline = clock.now().after(clock.rate.0 * 5);
        loop {
            if let Some(status) = stopper.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    return Err("PresentMon session termination failed".into());
                }
                break;
            }
            if clock.now() >= deadline {
                let _ = stopper.kill();
                let _ = stopper.wait();
                return Err("PresentMon termination timed out".into());
            }
            wait_control_period(clock, clock.rate.0 / 50).map_err(|e| e.to_string())?;
        }
        while self.child.try_wait().map_err(|e| e.to_string())?.is_none() {
            if clock.now() >= deadline {
                return Err("PresentMon flush timed out".into());
            }
            wait_control_period(clock, clock.rate.0 / 50).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn finish(&mut self) -> Result<(), Error> {
        let status = self.child.wait().map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("PresentMon exited {status}"))
        }
    }
}
impl Drop for PresentMon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
struct ExerciseGuard {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}
impl ExerciseGuard {
    fn finish(&mut self) {
        self.stop.store(true, SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
impl Drop for ExerciseGuard {
    fn drop(&mut self) {
        self.finish();
    }
}
const RING_HEADER: &str = "seq,kind,session,action,stage,hook_enter,enqueue,engine_receive,deadline,send_begin,send_end,inserted,expected,queue_depth,config_version,geometry_version,source_seq\n";
fn drain(
    reader: &mut RingReader,
    mapping: &Mapping,
    rows: &mut Vec<(u64, LogData)>,
    discarded: &mut u64,
) {
    while let Some(row) = reader.read(&mapping.data().ring) {
        if rows.len() < 1_000_000 {
            rows.push(row);
        } else {
            *discarded += 1;
        }
    }
}
fn ring_text(rows: &[(u64, LogData)]) -> String {
    use std::fmt::Write;
    let mut text = RING_HEADER.to_owned();
    for (seq, data) in rows {
        write!(text, "{seq}").unwrap();
        for value in data.0 {
            write!(text, ",{value}").unwrap();
        }
        text.push('\n');
    }
    text
}
pub fn measure(
    points: &Path,
    keys: &Path,
    bench: &BenchOptions,
    seconds: u64,
    scenario: &str,
    experiment: Option<EngineParams>,
    exercise: bool,
) -> Result<AbResult, Error> {
    if seconds == 0 || bench.stop_cycles == 0 || bench.stop_cycles > 100_000 {
        return Err("measurement duration must be positive".into());
    }
    fs::create_dir(&bench.output).map_err(|e| e.to_string())?;
    let clock = Clock::new().map_err(|e| e.to_string())?;
    let (_, k, _, _, mut cfg) = configuration(points, keys, bench.settings.as_deref(), clock)?;
    if let Some(params) = experiment {
        cfg.params = params;
    }
    #[cfg(feature = "test-hooks")]
    let timing = cfg.timing;
    let managed = ManagedBench::start(options(cfg, &k, bench, RunMode::Normal)?)?;
    let mut pmon = bench
        .presentmon
        .as_ref()
        .map(|p| {
            PresentMon::start(
                p,
                &bench.target,
                &bench.output.join("presentmon.csv"),
                seconds,
            )
        })
        .transpose()?;
    let (mapping, process) = managed.inspect()?;
    let cpu_start = process.cpu_100ns().map_err(|e| e.to_string())?;
    let begin = clock.now();
    let stop_exercise = Arc::new(AtomicBool::new(false));
    let thread = if exercise {
        if !bench
            .target
            .to_string_lossy()
            .eq_ignore_ascii_case("baauto-receiver.exe")
        {
            return Err("automated input exercise is restricted to baauto-receiver.exe".into());
        }
        #[cfg(not(feature = "test-hooks"))]
        {
            return Err("receiver exercise requires test-hooks build".into());
        }
        #[cfg(feature = "test-hooks")]
        {
            let stop = stop_exercise.clone();
            Some(thread::spawn(move || {
                while !stop.load(SeqCst) {
                    if target_for_foreground(std::ffi::OsStr::new("baauto-receiver.exe"))
                        .ok()
                        .flatten()
                        .is_some()
                    {
                        let _ = inject_test_key(Key(87), true);
                        let _ = wait_control_period(
                            clock,
                            timing
                                .key_hold
                                .saturating_add(timing.mouse_hold)
                                .saturating_add(timing.key_to_click),
                        );
                        let _ = inject_test_key(Key(87), false);
                    }
                    let _ = wait_control_period(clock, timing.repeat_period);
                }
                let _ = inject_test_key(Key(87), false);
            }))
        }
    } else {
        None
    };
    let mut exerciser = ExerciseGuard {
        stop: stop_exercise,
        join: thread,
    };
    let mut reader = RingReader::default();
    let mut rows = Vec::new();
    let mut discarded = 0;
    let until = begin.after(seconds.saturating_mul(clock.rate.0));
    while clock.now() < until {
        drain(&mut reader, &mapping, &mut rows, &mut discarded);
        if process.exited() {
            return Err("engine exited during measurement".into());
        }
        wait_control_period(clock, clock.rate.0 / 50).map_err(|e| e.to_string())?;
    }
    exerciser.finish();
    let cpu_end = process.cpu_100ns().map_err(|e| e.to_string())?;
    let end = clock.now();
    managed.stop()?;
    for _ in 1..bench.stop_cycles {
        drain(&mut reader, &mapping, &mut rows, &mut discarded);
        managed.arm()?;
        managed.stop()?;
    }
    drain(&mut reader, &mapping, &mut rows, &mut discarded);
    let text = ring_text(&rows);
    create_file(&bench.output.join("ring.csv"), text.as_bytes())?;
    let mut metrics = software_metrics(&parse_ring(&text)?, clock.rate)?;
    metrics.lost = reader.lost + discarded;
    create_file(
        &bench.output.join("software.md"),
        markdown_software(&metrics, scenario).as_bytes(),
    )?;
    create_file(
        &bench.output.join("software.json"),
        serde_json::to_vec_pretty(&metrics)
            .map_err(|e| e.to_string())?
            .as_slice(),
    )?;
    if let Some(pmon) = pmon.as_mut() {
        pmon.finish()?;
    }
    let frame_p99 = if bench.presentmon.is_some() {
        let frames = parse_presentmon(
            &fs::read_to_string(bench.output.join("presentmon.csv")).map_err(|e| e.to_string())?,
        )?;
        summarize(&frames.iter().map(|f| f.between_ms).collect::<Vec<_>>())
            .ok()
            .and_then(|s| s.p99)
    } else {
        None
    };
    let late = summarize(&metrics.late_us).ok().and_then(|s| s.p999);
    let stop = summarize(&metrics.stop_us).ok().and_then(|s| s.p99);
    let elapsed = (end.0 - begin.0) as f64 / clock.rate.0 as f64;
    let cpu = (cpu_end - cpu_start) as f64
        / 10_000_000.
        / elapsed
        / std::thread::available_parallelism().map_or(1, usize::from) as f64
        * 100.;
    let result = AbResult {
        label: scenario.into(),
        tail_us: late.unwrap_or(f64::INFINITY),
        frame_p99_ms: frame_p99.unwrap_or(f64::INFINITY),
        cpu_percent: cpu,
        stop_p99_us: stop.unwrap_or(f64::INFINITY),
        lost: metrics.lost,
        hook_failures: u64::from(process.exited()),
    };
    // JSON cannot encode Infinity; explicit null conveys missing sample evidence.
    create_file(&bench.output.join("measurement.json"),serde_json::to_vec_pretty(&serde_json::json!({"scenario":scenario,"qpc_frequency":clock.rate.0,"degraded_flags":mapping.data().progress.degraded_flags.load(SeqCst),"tail_us":late,"frame_p99_ms":frame_p99,"stop_p99_us":stop,"cpu_percent":cpu,"lost":metrics.lost})).map_err(|e|e.to_string())?.as_slice())?;
    Ok(result)
}
pub fn run_scan(
    plan: &Plan,
    bench: &BenchOptions,
    validation: Option<&DetectorValidation>,
    pilot_count: Option<u64>,
) -> Result<(), Error> {
    plan.validate()?;
    let valid = validation.is_some_and(|v| {
        v.valid
            && v.plan_hash == plan.hash().unwrap()
            && v.confidence == plan.confidence
            && v.error_upper <= plan.max_detector_error
            && v.labels_sha256.len() == 64
    });
    if pilot_count.is_none() && !valid {
        return Err(
            "validate the detector against independent human labels before a full scan".into(),
        );
    }
    fs::create_dir(&bench.output).map_err(|e| e.to_string())?;
    fs::create_dir(bench.output.join("frames")).map_err(|e| e.to_string())?;
    let clock = Clock::new().map_err(|e| e.to_string())?;
    let (p, k, h, _, cfg) =
        configuration(&plan.points, &plan.keys, bench.settings.as_deref(), clock)?;
    if p.mode != plan.mode
        || h.card_ui_side != plan.side
        || sha256(&fs::read(&plan.points).map_err(|e| e.to_string())?) != plan.points_sha256
        || sha256(&fs::read(&plan.keys).map_err(|e| e.to_string())?) != plan.keys_sha256
        || sha256(to_json(&h).map_err(|e| e.to_string())?.as_bytes()) != plan.hand_sha256
    {
        return Err("plan source files/mode/hand settings changed".into());
    }
    let base = cfg.timing;
    let upper_trials = plan.attempts.saturating_mul(10_000);
    let scan_seconds = ((plan.trial_duration_ms + 1100.) / 1000. * upper_trials as f64)
        .ceil()
        .min(u32::MAX as f64) as u64;
    let hash = plan.hash()?;
    create_file(
        &bench.output.join("plan.json"),
        to_json(plan).map_err(|e| e.to_string())?.as_bytes(),
    )?;
    create_file(&bench.output.join("plan.sha256"), hash.as_bytes())?;
    let managed = ManagedBench::start(options(cfg, &k, bench, RunMode::Calibration)?)?;
    let (mapping, process) = managed.inspect()?;
    let mut pmon = bench
        .presentmon
        .as_ref()
        .map(|p| {
            PresentMon::start(
                p,
                &bench.target,
                &bench.output.join("presentmon.csv"),
                scan_seconds,
            )
        })
        .transpose()?;
    let target = target_for_foreground(&bench.target)
        .map_err(|e| e.to_string())?
        .ok_or("target lost foreground")?;
    if target.warning()
        || [target.monitor.width as u32, target.monitor.height as u32] != plan.resolution
    {
        return Err("plan requires matching fullscreen 16:9 monitor resolution".into());
    }
    let mut capture = crate::dxgi::Capture::new(target.monitor).map_err(|e| e.to_string())?;
    let mut latest = None;
    let baseline_deadline = clock.now().after(clock.rate.0 * 5);
    while latest.is_none() && clock.now() < baseline_deadline {
        latest = capture.next_frame(20).map_err(|e| e.to_string())?;
    }
    let mut latest = latest.ok_or("no baseline desktop frame")?;
    let count = pilot_count.unwrap_or(plan.attempts);
    if count == 0 || count > plan.attempts {
        return Err("pilot count must be 1..plan.attempts".into());
    }
    let frame_ticks = ticks_ms(plan.frame_ms, clock.rate).map_err(|e| e.to_string())?;
    let duration = ticks_ms(plan.trial_duration_ms, clock.rate).map_err(|e| e.to_string())?;
    let mut rng = PhaseRng::new(plan.seed);
    let mut id = 0u64;
    let mut trial_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(bench.output.join("trials.csv"))
        .map_err(|e| e.to_string())?;
    trial_file
        .write_all(
            b"id,value_ms,phase_ticks,started,finished,changed_qpc,detected,eligible,label\n",
        )
        .map_err(|e| e.to_string())?;
    let mut samples = Vec::new();
    let mut reader = RingReader::default();
    let mut ring = Vec::new();
    let mut discarded = 0;
    let mut values = plan.coarse_ms.clone();
    let mut fine = false;
    loop {
        for value in &values {
            let mut successes = 0;
            for _ in 0..count {
                if process.exited()
                    || target_for_foreground(&bench.target)
                        .map_err(|e| e.to_string())?
                        .is_none()
                {
                    return Err(
                        "calibration interrupted; output stopped and profile unverified".into(),
                    );
                }
                if let Some(frame) = capture.next_frame(0).map_err(|e| e.to_string())? {
                    latest = frame;
                }
                let before = latest.clone();
                id += 1;
                let phase = rng.next_phase(frame_ticks);
                let start = clock.now().after(clock.rate.0 / 10).after(phase);
                let mut timing = trial_timing(base, &plan.parameter, *value, clock.rate)?;
                if plan.parameter == "escape_gap" {
                    timing.gap_max = duration.max(timing.gap_min);
                }
                let cursor = NormPoint {
                    x: (h.layout(plan.mode).x[0] + h.layout(plan.mode).x[1]) / 2.,
                    y: (h.layout(plan.mode).y[0] + h.layout(plan.mode).y[1]) / 2.,
                };
                let trial = TrialSpec {
                    id,
                    action: plan.action,
                    timing,
                    start,
                    duration,
                    cursor,
                };
                let (tx, rx) = mpsc::channel();
                managed
                    .tx
                    .send(SupervisorCommand::RunTrial {
                        trial: Box::new(trial),
                        reply: tx,
                    })
                    .map_err(|e| e.to_string())?;
                let until = start.after(duration).after(clock.rate.0);
                let mut changed = None;
                let mut completed = None;
                while clock.now() < until {
                    if let Some(frame) = capture.next_frame(20).map_err(|e| e.to_string())? {
                        if frame.present >= start
                            && frame.mean_difference(&before, plan.roi)?
                                >= plan.difference_threshold
                        {
                            changed.get_or_insert(frame.present.0);
                        }
                        latest = frame;
                    }
                    drain(&mut reader, &mapping, &mut ring, &mut discarded);
                    if let Ok(result) = rx.try_recv() {
                        completed = Some(result?);
                    }
                    if completed.is_some() && clock.now() >= start.after(duration) {
                        break;
                    }
                }
                let result = completed.ok_or("trial response timed out")?;
                if result.outcome != TrialOutcome::Complete {
                    return Err(format!(
                        "trial {} did not complete: {:?}; partial trials.csv retained",
                        id, result.outcome
                    ));
                }
                let eligible = true;
                let detected = changed.is_some();
                if eligible && detected {
                    successes += 1;
                }
                writeln!(
                    trial_file,
                    "{},{},{},{},{},{},{},{},",
                    id,
                    value,
                    phase,
                    result.started.0,
                    result.finished.0,
                    changed.map_or(String::new(), |v| v.to_string()),
                    detected,
                    eligible
                )
                .map_err(|e| e.to_string())?;
                trial_file.sync_data().map_err(|e| e.to_string())?;
                if pilot_count.is_some() {
                    create_file(
                        &bench.output.join(format!("frames/{id}-before.bmp")),
                        &before.crop(plan.roi)?.bmp()?,
                    )?;
                    create_file(
                        &bench.output.join(format!("frames/{id}-after.bmp")),
                        &latest.crop(plan.roi)?.bmp()?,
                    )?;
                }
            }
            samples.push(ScanPoint {
                value_ms: *value,
                successes,
                attempts: count,
            });
        }
        if fine || pilot_count.is_some() {
            break;
        }
        values = refine_boundaries(
            &samples,
            plan.min_success,
            plan.confidence,
            plan.fine_step_ms,
        )?;
        if values.is_empty() {
            break;
        }
        fine = true;
    }
    managed.stop()?;
    drain(&mut reader, &mapping, &mut ring, &mut discarded);
    let text = ring_text(&ring);
    create_file(&bench.output.join("ring.csv"), text.as_bytes())?;
    let metrics = software_metrics(&parse_ring(&text)?, clock.rate)?;
    let p999 = summarize(&metrics.late_us)
        .ok()
        .and_then(|s| s.p999)
        .map(|us| us / 1000.);
    let margin = p999.unwrap_or(0.);
    let selected = choose_with_margin(&samples, plan, margin)?.unwrap_or(-1.);
    let measured_fps = if let Some(pm) = pmon.as_mut() {
        pm.terminate()?;
        let csv =
            fs::read_to_string(bench.output.join("presentmon.csv")).map_err(|e| e.to_string())?;
        let frames = parse_presentmon(&csv)?;
        summarize(&frames.iter().map(|f| f.between_ms).collect::<Vec<_>>())
            .ok()
            .map(|s| 1000. / s.p50)
    } else {
        None
    };
    let evidence = TrialEvidence {
        plan_hash: hash,
        identity: plan.identity.clone(),
        min_success: plan.min_success,
        confidence: plan.confidence,
        detector_validated: valid && pilot_count.is_none(),
        detector_error: validation.map_or(1., |v| v.error_upper),
        degraded_flags: mapping.data().progress.degraded_flags.load(SeqCst)
            | u32::from(reader.lost + discarded > 0),
        samples,
        margin_ms: margin,
        selected_ms: selected,
        p999_late_ms: p999,
        measured_fps,
        outcomes_sha256: None,
    };
    create_file(
        &bench.output.join("result.json"),
        to_json(&evidence).map_err(|e| e.to_string())?.as_bytes(),
    )?;
    create_file(
        &bench.output.join("scan.md"),
        markdown_scan(plan, &evidence.samples)?.as_bytes(),
    )?;
    Ok(())
}
