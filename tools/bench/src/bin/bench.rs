#![forbid(unsafe_code)]
use baauto_bench::*;
use baauto_config::{store::*, *};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
fn new_file(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    Ok(from_json(&decode_json(&fs::read(path)?)?)?)
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next().ok_or("use --help")?;
    match command.to_str() {
        Some("plan") => {
            let path = PathBuf::from(args.next().ok_or("output JSON required")?);
            new_file(&path, to_json(&Plan::example())?.as_bytes())?;
            println!("範例計畫：{}；請先填入實際條件，再 seal。", path.display());
        }
        Some("seal") => {
            let draft = PathBuf::from(args.next().ok_or("draft JSON required")?);
            let output = PathBuf::from(args.next().ok_or("sealed output JSON required")?);
            let mut plan: Plan = load(&draft)?;
            plan.points = fs::canonicalize(&plan.points)?;
            plan.keys = fs::canonicalize(&plan.keys)?;
            plan.points_sha256 = sha256(&fs::read(&plan.points)?);
            plan.keys_sha256 = sha256(&fs::read(&plan.keys)?);
            if let Some(settings) = args.next() {
                let (h, _) = read_machine(Path::new(&settings))?;
                if h.card_ui_side != plan.side {
                    return Err("hand side differs from plan".into());
                }
                plan.hand_sha256 = sha256(to_json(&h)?.as_bytes());
            }
            let hash = plan.hash()?;
            new_file(&output, to_json(&plan)?.as_bytes())?;
            new_file(&output.with_extension("sha256"), hash.as_bytes())?;
            println!("計畫 SHA-256：{hash}");
        }
        Some("analyze") => {
            let path = PathBuf::from(args.next().ok_or("ring CSV required")?);
            let rate: u64 = args
                .next()
                .ok_or("QPC frequency required")?
                .to_str()
                .ok_or("invalid frequency")?
                .parse()?;
            let output = PathBuf::from(args.next().ok_or("output Markdown required")?);
            let data = parse_ring(&decode_json(&fs::read(path)?)?)?;
            let metrics = software_metrics(&data, TickRate(rate))?;
            new_file(&output, markdown_software(&metrics, "匯入紀錄").as_bytes())?;
        }
        Some("align") => {
            let pm = PathBuf::from(args.next().ok_or("PresentMon CSV required")?);
            let ring = PathBuf::from(args.next().ok_or("ring CSV required")?);
            let out = PathBuf::from(args.next().ok_or("output CSV required")?);
            let frames = parse_presentmon(&decode_json(&fs::read(pm)?)?)?;
            let ring = parse_ring(&decode_json(&fs::read(ring)?)?)?;
            use std::fmt::Write;
            let mut csv = "seq,send_end,frame_index,CPUStartQPC\n".to_owned();
            for row in ring.records {
                if matches!(row.fields[0], 2 | 5) && row.fields[9] != 0 {
                    let index = first_frame_after(Ticks(row.fields[9]), &frames);
                    writeln!(
                        csv,
                        "{},{},{},{}",
                        row.seq,
                        row.fields[9],
                        index.map_or(String::new(), |i| i.to_string()),
                        index.map_or(String::new(), |i| frames[i].cpu_start.0.to_string())
                    )?;
                }
            }
            new_file(&out, csv.as_bytes())?;
        }
        Some("validate-detector") => {
            let plan: Plan = load(Path::new(&args.next().ok_or("plan JSON required")?))?;
            let labels = decode_json(&fs::read(
                args.next().ok_or("human-labelled CSV required")?,
            )?)?;
            let output = PathBuf::from(args.next().ok_or("output validation JSON required")?);
            let result = validate_labels(&plan, &labels)?;
            new_file(&output, to_json(&result)?.as_bytes())?;
            println!(
                "n={} error upper={:.6}; valid={}",
                result.confusion.tp
                    + result.confusion.fp
                    + result.confusion.fn_
                    + result.confusion.tn,
                result.error_upper,
                result.valid
            );
        }
        Some("finalize") => {
            let plan: Plan = load(Path::new(&args.next().ok_or("sealed plan required")?))?;
            let mut evidence: TrialEvidence =
                load(Path::new(&args.next().ok_or("scan evidence required")?))?;
            if evidence.plan_hash != plan.hash()? {
                return Err("evidence/plan hash differs".into());
            }
            let labels = decode_json(&fs::read(
                args.next().ok_or("independent outcomes CSV required")?,
            )?)?;
            let output = PathBuf::from(args.next().ok_or("new final evidence JSON required")?);
            evidence.samples = classified_samples(&plan, &labels)?;
            evidence.outcomes_sha256 = Some(sha256(labels.as_bytes()));
            evidence.selected_ms =
                choose_with_margin(&evidence.samples, &plan, evidence.margin_ms)?.unwrap_or(-1.);
            println!("verified={}", evidence.verified(&plan)?);
            new_file(&output, to_json(&evidence)?.as_bytes())?;
        }
        Some("apply") => {
            let plan: Plan = load(Path::new(&args.next().ok_or("sealed plan required")?))?;
            let result: TrialEvidence =
                load(Path::new(&args.next().ok_or("evidence JSON required")?))?;
            let keys = PathBuf::from(
                args.next()
                    .unwrap_or_else(|| plan.keys.as_os_str().to_os_string()),
            );
            if sha256(&fs::read(&keys)?) != plan.keys_sha256 {
                return Err("key file changed since plan was sealed; do not overwrite".into());
            }
            let mut profile: KeysProfile = load(&keys)?;
            profile.timing = calibrated_profile(&plan, &result, &profile.timing)?;
            #[cfg(windows)]
            atomic_save(
                &baauto_win_platform::WindowsFs,
                &keys,
                &Document::Keys(Box::new(profile)),
            )?;
            #[cfg(not(windows))]
            atomic_save(&PortableFs, &keys, &Document::Keys(Box::new(profile)))?;
            println!("校準結果已備份並儲存：{}", keys.display());
        }
        Some("measure" | "scan" | "ab") => {
            #[cfg(windows)]
            native(command.to_str().unwrap(), args.collect())?;
            #[cfg(not(windows))]
            return Err("native measurement/capture requires Windows".into());
        }
        Some("compare") => {
            let base: AbResult = load(Path::new(&args.next().ok_or("baseline JSON required")?))?;
            let variant: AbResult = load(Path::new(&args.next().ok_or("variant JSON required")?))?;
            println!("retain={}", keep_experiment(&base, &variant));
        }
        Some("--help" | "help") => println!(
            "baauto-bench plan <draft.json>\nseal <draft.json> <sealed.json> [settings.json]\nanalyze <ring.csv> <QPC frequency> <out.md>\nalign <PresentMon.csv> <ring.csv> <out.csv>\nvalidate-detector <plan.json> <labelled-trials.csv> <validation.json>\nfinalize <plan.json> <result.json> <labelled-trials.csv> <final.json>\napply <plan.json> <result.json> [keys JSON]\nmeasure --points <JSON> --keys <JSON> --out <new directory> [--seconds N] [--settings JSON] [--target-exe EXE] [--engine EXE] [--presentmon EXE] [--exercise-receiver]\nscan --plan <sealed.json> --out <new directory> [--pilot-count N | --validation JSON] [native options]\nab [measure options] [--spin-us N]; requires bench-experiments build\ncompare <baseline.json> <variant.json>"
        ),
        _ => return Err("unknown bench command".into()),
    }
    Ok(())
}
#[cfg(windows)]
fn native(command: &str, args: Vec<std::ffi::OsString>) -> Result<(), Box<dyn std::error::Error>> {
    use baauto_bench::win::*;
    let mut points = PathBuf::from("points/範例.json");
    let mut keys = PathBuf::from("keys/預設按鍵.json");
    let mut plan_path = None;
    let mut validation = None;
    let mut pilot = None;
    let mut seconds = 30u64;
    let mut spin = 100u32;
    let mut exercise = false;
    let mut bench = BenchOptions {
        engine: std::env::current_exe()?.with_file_name("baauto-engine.exe"),
        target: "baauto-receiver.exe".into(),
        settings: None,
        presentmon: None,
        output: PathBuf::from("bench-output"),
        stop_cycles: 1,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--exercise-receiver" {
            exercise = true;
            continue;
        }
        let value = args.next().ok_or("option value missing")?;
        match arg.to_str() {
            Some("--points") => points = value.into(),
            Some("--keys") => keys = value.into(),
            Some("--settings") => bench.settings = Some(value.into()),
            Some("--target-exe") => bench.target = value,
            Some("--engine") => bench.engine = value.into(),
            Some("--presentmon") => bench.presentmon = Some(value.into()),
            Some("--out") => bench.output = value.into(),
            Some("--plan") => plan_path = Some(PathBuf::from(value)),
            Some("--validation") => validation = Some(PathBuf::from(value)),
            Some("--pilot-count") => pilot = Some(value.to_str().ok_or("invalid count")?.parse()?),
            Some("--stop-cycles") => {
                bench.stop_cycles = value.to_str().ok_or("invalid stop count")?.parse()?
            }
            Some("--seconds") => seconds = value.to_str().ok_or("invalid seconds")?.parse()?,
            Some("--spin-us") => spin = value.to_str().ok_or("invalid spin")?.parse()?,
            _ => return Err("unknown native option".into()),
        }
    }
    match command {
        "scan" => {
            let plan: Plan = load(&plan_path.ok_or("--plan required")?)?;
            let validation: Option<DetectorValidation> =
                validation.map(|p| load(&p)).transpose()?;
            run_scan(&plan, &bench, validation.as_ref(), pilot)?;
        }
        "measure" => {
            measure(&points, &keys, &bench, seconds, "default", None, exercise)?;
        }
        "ab" => {
            if !cfg!(feature = "bench-experiments") {
                return Err("A/B requires bench-experiments build".into());
            }
            if bench.stop_cycles == 1 {
                bench.stop_cycles = 1000;
            }
            fs::create_dir(&bench.output)?;
            let root = bench.output.clone();
            let base_params = bench
                .settings
                .as_deref()
                .map(read_machine)
                .transpose()?
                .map(|(_, p)| p)
                .unwrap_or_default();
            let mut results = Vec::new();
            for hook in [true, false] {
                for engine in [false, true] {
                    for tail in [0, spin] {
                        let label = format!(
                            "hook-{}-engine-{}-spin-{tail}",
                            if hook { "critical" } else { "normal" },
                            if engine { "highest" } else { "normal" }
                        );
                        println!("{label}: repeat the same receiver input workload");
                        bench.output = root.join(&label);
                        let params = EngineParams {
                            hook_time_critical: hook,
                            engine_highest: engine,
                            tail_spin_us: tail,
                            ..base_params
                        };
                        results.push(measure(
                            &points,
                            &keys,
                            &bench,
                            seconds,
                            &label,
                            Some(params),
                            exercise,
                        )?);
                    }
                }
            }
            use std::fmt::Write;
            let base = &results[0];
            let mut report="# A/B comparison\n\n| variant | tail µs | frame P99 ms | CPU % | stop P99 µs | retain |\n|---|---:|---:|---:|---:|---|\n".to_owned();
            for r in &results {
                let show = |v: f64| {
                    if v.is_finite() {
                        format!("{v:.3}")
                    } else {
                        "樣本不足".into()
                    }
                };
                writeln!(
                    report,
                    "| {} | {} | {} | {} | {} | {} |",
                    r.label,
                    show(r.tail_us),
                    show(r.frame_p99_ms),
                    show(r.cpu_percent),
                    show(r.stop_p99_us),
                    keep_experiment(base, r)
                )?;
            }
            new_file(&root.join("comparison.md"), report.as_bytes())?;
        }
        _ => unreachable!(),
    }
    Ok(())
}
