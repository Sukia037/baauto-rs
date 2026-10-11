use baauto_config::{TickRate, Ticks};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub type Error = String;
/// CSV quotes, escaped quotes, commas and newlines; reject malformed/truncated quoting.
pub fn csv_rows(text: &str) -> Result<Vec<Vec<String>>, Error> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    let mut quoted = false;
    let mut closed = false;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !closed => quoted = true,
            ',' => {
                row.push(std::mem::take(&mut field));
                closed = false;
            }
            '\n' => {
                row.push(std::mem::take(&mut field));
                if !row.iter().all(String::is_empty) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                closed = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            _ if closed => return Err("characters after closed CSV quote".into()),
            _ => field.push(c),
        }
    }
    if quoted {
        return Err("unterminated CSV quote".into());
    }
    if !field.is_empty() || !row.is_empty() || closed {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}
fn records(text: &str, required: &[&str]) -> Result<Vec<BTreeMap<String, String>>, Error> {
    let mut rows = csv_rows(text)?
        .into_iter()
        .filter(|r| !r[0].starts_with('#'));
    let header = rows.next().ok_or("empty CSV")?;
    if required
        .iter()
        .any(|name| !header.iter().any(|h| h == name))
    {
        return Err(format!(
            "missing CSV columns; required {}",
            required.join(", ")
        ));
    }
    if header
        .iter()
        .enumerate()
        .any(|(i, h)| header[..i].contains(h))
    {
        return Err("duplicate CSV header".into());
    }
    rows.map(|r| {
        if r.len() != header.len() {
            return Err("CSV column count mismatch".into());
        }
        Ok(header.iter().cloned().zip(r).collect())
    })
    .collect()
}
const RING_HEADERS: [&str; 17] = [
    "seq",
    "kind",
    "session",
    "action",
    "stage",
    "hook_enter",
    "enqueue",
    "engine_receive",
    "deadline",
    "send_begin",
    "send_end",
    "inserted",
    "expected",
    "queue_depth",
    "config_version",
    "geometry_version",
    "source_seq",
];
#[derive(Clone, Debug)]
pub struct RingRecord {
    pub seq: u64,
    pub fields: [u64; 16],
}
pub struct RingData {
    pub records: Vec<RingRecord>,
    pub lost: u64,
}
pub fn parse_ring(text: &str) -> Result<RingData, Error> {
    let rows = records(text, &RING_HEADERS)?;
    let mut result = RingData {
        records: Vec::new(),
        lost: 0,
    };
    let mut next = 0;
    for row in rows {
        let mut values = [0; 17];
        for (i, name) in RING_HEADERS.iter().enumerate() {
            values[i] = row[*name]
                .trim()
                .parse()
                .map_err(|_| format!("invalid integer {name}"))?;
        }
        let seq = values[0];
        if seq < next {
            return Err("ring sequence reversed or duplicated".into());
        }
        result.lost = result.lost.saturating_add(seq - next);
        next = seq.checked_add(1).ok_or("ring sequence overflow")?;
        result.records.push(RingRecord {
            seq,
            fields: values[1..].try_into().unwrap(),
        });
    }
    Ok(result)
}
#[derive(Clone, Debug)]
pub struct Frame {
    pub cpu_start: Ticks,
    pub between_ms: f64,
    pub display_ms: Option<f64>,
}
fn finite_number(text: &str) -> Result<f64, Error> {
    let n = text.trim().parse::<f64>().map_err(|_| "invalid float")?;
    if n.is_finite() && n >= 0. {
        Ok(n)
    } else {
        Err("negative or nonfinite number".into())
    }
}
pub fn parse_presentmon(text: &str) -> Result<Vec<Frame>, Error> {
    let mut frames = Vec::new();
    for row in records(
        text,
        &["CPUStartQPC", "MsBetweenPresents", "MsUntilDisplayed"],
    )? {
        let cpu_start = Ticks(
            row["CPUStartQPC"]
                .trim()
                .parse()
                .map_err(|_| "CPUStartQPC must be integer ticks (--qpc_time)")?,
        );
        if frames
            .last()
            .is_some_and(|f: &Frame| f.cpu_start > cpu_start)
        {
            return Err(
                "PresentMon QPC order reversed; filter to the target process/swapchain".into(),
            );
        }
        let display = &row["MsUntilDisplayed"];
        frames.push(Frame {
            cpu_start,
            between_ms: finite_number(&row["MsBetweenPresents"])?,
            display_ms: if display.is_empty() || display.eq_ignore_ascii_case("NA") {
                None
            } else {
                Some(finite_number(display)?)
            },
        });
    }
    Ok(frames)
}
pub fn first_frame_after(send_end: Ticks, frames: &[Frame]) -> Option<usize> {
    let index = frames.partition_point(|f| f.cpu_start <= send_end);
    (index < frames.len()).then_some(index)
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct SoftwareMetrics {
    pub hook_us: Vec<f64>,
    pub wake_us: Vec<f64>,
    pub late_us: Vec<f64>,
    pub inject_us: Vec<f64>,
    pub stop_us: Vec<f64>,
    pub lost: u64,
    pub short_sends: u64,
}
pub fn software_metrics(data: &RingData, rate: TickRate) -> Result<SoftwareMetrics, Error> {
    if rate.0 == 0 {
        return Err("QPC frequency must be positive".into());
    }
    let mut result = SoftwareMetrics {
        lost: data.lost,
        ..Default::default()
    };
    let mut stop = BTreeMap::new();
    let delta = |end: u64, start: u64| -> Result<f64, Error> {
        end.checked_sub(start)
            .map(|d| d as f64 * 1_000_000. / rate.0 as f64)
            .ok_or("QPC timestamps reversed".into())
    };
    for record in &data.records {
        let f = record.fields;
        match f[0] {
            2 | 5 => {
                if f[10] != f[11] {
                    result.short_sends += 1;
                    continue;
                }
                if f[0] == 2 && f[4] != 0 {
                    result.hook_us.push(delta(f[5], f[4])?);
                    result.wake_us.push(delta(f[6], f[5])?);
                }
                if f[8] != 0 {
                    result.late_us.push(delta(f[8].max(f[7]), f[7])?);
                    result.inject_us.push(delta(f[9], f[8])?);
                }
            }
            3 => {
                stop.insert(f[1], f[6]);
            }
            4 => {
                if let Some(begin) = stop.remove(&f[1]) {
                    result.stop_us.push(delta(f[9], begin)?);
                }
            }
            _ => {}
        }
    }
    Ok(result)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub count: usize,
    pub p50: f64,
    pub p95: f64,
    pub p99: Option<f64>,
    pub p999: Option<f64>,
    pub max: f64,
}
pub fn summarize(values: &[f64]) -> Result<Summary, Error> {
    if values.is_empty() || values.iter().any(|v| !v.is_finite() || *v < 0.) {
        return Err("nonempty finite nonnegative samples required".into());
    }
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let q = |p: f64| values[((p * values.len() as f64).ceil() as usize).saturating_sub(1)];
    Ok(Summary {
        count: values.len(),
        p50: q(0.5),
        p95: q(0.95),
        p99: (values.len() >= 1000).then(|| q(0.99)),
        p999: (values.len() >= 10000).then(|| q(0.999)),
        max: *values.last().unwrap(),
    })
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Interval {
    pub lower: f64,
    pub upper: f64,
}
fn z_score(confidence: f64) -> Result<f64, Error> {
    if !(0.0..1.).contains(&confidence) || confidence == 0. {
        return Err("confidence must be between 0 and 1".into());
    }
    if (confidence - 0.95).abs() < 1e-14 {
        return Ok(1.959963984540054);
    }
    let target = (1. + confidence) / 2.;
    let (mut low, mut high) = (0., 10.);
    for _ in 0..80 {
        let z = (low + high) / 2.;
        let x = z / std::f64::consts::SQRT_2;
        let t = 1. / (1. + 0.3275911 * x);
        let erf = 1.
            - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t
                + 0.254829592)
                * t
                * (-x * x).exp();
        if (1. + erf) / 2. < target {
            low = z;
        } else {
            high = z;
        }
    }
    Ok((low + high) / 2.)
}
pub fn wilson(successes: u64, attempts: u64, confidence: f64) -> Result<Interval, Error> {
    if attempts == 0 || successes > attempts {
        return Err("invalid binomial counts".into());
    }
    let z = z_score(confidence)?;
    let n = attempts as f64;
    let p = successes as f64 / n;
    let z2 = z * z;
    let center = (p + z2 / (2. * n)) / (1. + z2 / n);
    let half = z / (1. + z2 / n) * (p * (1. - p) / n + z2 / (4. * n * n)).sqrt();
    Ok(Interval {
        lower: (center - half).max(0.),
        upper: (center + half).min(1.),
    })
}
pub fn minimum_zero_failures(target: f64, confidence: f64) -> Result<u64, Error> {
    if !(0.0..1.).contains(&target) || target == 0. {
        return Err("success threshold must be between 0 and 1".into());
    }
    let z = z_score(confidence)?;
    let n = (z * z * target / (1. - target)).ceil();
    if n > u64::MAX as f64 {
        return Err("sample count overflow".into());
    }
    Ok((n as u64).max(1))
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Confusion {
    pub tp: u64,
    pub fp: u64,
    pub fn_: u64,
    pub tn: u64,
}
impl Confusion {
    pub fn error_rate(self) -> Option<f64> {
        let n = self.tp + self.fp + self.fn_ + self.tn;
        (n > 0).then(|| (self.fp + self.fn_) as f64 / n as f64)
    }
}
/// Pair is (detector result, independent human label).
pub fn confusion(labels: &[(bool, bool)]) -> Confusion {
    let mut c = Confusion::default();
    for pair in labels {
        match pair {
            (true, true) => c.tp += 1,
            (true, false) => c.fp += 1,
            (false, true) => c.fn_ += 1,
            (false, false) => c.tn += 1,
        }
    }
    c
}
pub fn markdown_software(metrics: &SoftwareMetrics, scenario: &str) -> String {
    use std::fmt::Write;
    let mut report = format!(
        "# 軟體路徑量測：{scenario}\n\n遺失 {} 筆；短送 {} 筆。單位 µs；不是遊戲有效操作率。\n\n| 指標 | n | P50 | P95 | P99 | P99.9 | max |\n|---|---:|---:|---:|---:|---:|---:|\n",
        metrics.lost, metrics.short_sends
    );
    for (name, values) in [
        ("Hook", &metrics.hook_us),
        ("喚醒", &metrics.wake_us),
        ("晚醒", &metrics.late_us),
        ("注入", &metrics.inject_us),
        ("停止（請求／缺少請求時觀察→確認）", &metrics.stop_us),
    ] {
        if let Ok(s) = summarize(values) {
            let _ = writeln!(
                report,
                "| {name} | {} | {:.3} | {:.3} | {} | {} | {:.3} |",
                s.count,
                s.p50,
                s.p95,
                s.p99.map_or("—".into(), |v| format!("{v:.3}")),
                s.p999.map_or("—".into(), |v| format!("{v:.3}")),
                s.max
            );
        }
    }
    report
}
