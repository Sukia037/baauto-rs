use crate::*;
use baauto_config::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    pub points: PathBuf,
    pub keys: PathBuf,
    pub points_sha256: String,
    pub keys_sha256: String,
    pub hand_sha256: String,
    pub mode: Mode,
    pub side: CardUiSide,
    pub action: ActionSpec,
    pub parameter: String,
    pub coarse_ms: Vec<f64>,
    pub fine_step_ms: f64,
    pub attempts: u64,
    pub roi: HandRect,
    pub difference_threshold: f64,
    pub max_detector_error: f64,
    pub min_success: f64,
    pub confidence: f64,
    pub identity: ProfileIdentity,
    pub resolution: [u32; 2],
    pub refresh_hz: f64,
    pub frame_ms: f64,
    pub seed: u64,
    pub trial_duration_ms: f64,
}
impl Plan {
    pub fn example() -> Self {
        Self {
            schema: "baauto-rs-calibration-v1".into(),
            points: "points/範例.json".into(),
            keys: "keys/預設按鍵.json".into(),
            points_sha256: sha256(include_bytes!("../../../points/範例.json")),
            keys_sha256: sha256(include_bytes!("../../../keys/預設按鍵.json")),
            hand_sha256: sha256(to_json(&HandSettings::default()).unwrap().as_bytes()),
            mode: Mode::Six,
            side: CardUiSide::Right,
            action: ActionSpec::DoubleEsc { slow: false },
            parameter: "frame_fast.esc_gap_ms".into(),
            coarse_ms: vec![10., 20., 30., 40., 50., 60.],
            fine_step_ms: 1.,
            attempts: 1000,
            roi: HandRect {
                x: [0.3, 0.7],
                y: [0.2, 0.6],
                calibrated: false,
            },
            difference_threshold: 15.,
            max_detector_error: 0.01,
            min_success: 0.995,
            confidence: 0.95,
            identity: ProfileIdentity {
                window_mode: "fullscreen_16_9".into(),
                conditions: "EXAMPLE ONLY; replace before running".into(),
                ..Default::default()
            },
            resolution: [2560, 1440],
            refresh_hz: 60.,
            frame_ms: 1000. / 60.,
            seed: 1,
            trial_duration_ms: 500.,
        }
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.schema != "baauto-rs-calibration-v1"
            || self.coarse_ms.is_empty()
            || self.coarse_ms.len() > 10000
            || self.coarse_ms.iter().any(|v| !v.is_finite() || *v < 0.)
            || self.coarse_ms.windows(2).any(|w| w[0] >= w[1])
            || !self.fine_step_ms.is_finite()
            || self.fine_step_ms <= 0.
            || !self.roi.valid()
            || !self.difference_threshold.is_finite()
            || !(0.0..=255.).contains(&self.difference_threshold)
            || !self.max_detector_error.is_finite()
            || !(0.0..1.).contains(&self.max_detector_error)
            || self.max_detector_error == 0.
            || self.attempts == 0
            || self.attempts > 10_000_000
            || !self.refresh_hz.is_finite()
            || self.refresh_hz <= 0.
            || !self.frame_ms.is_finite()
            || self.frame_ms <= 0.
            || !self.trial_duration_ms.is_finite()
            || self.trial_duration_ms <= 0.
            || self.resolution.contains(&0)
            || template_keys().timing.gap_mut(&self.parameter).is_none()
        {
            return Err("invalid calibration plan".into());
        }
        if [&self.points_sha256, &self.keys_sha256, &self.hand_sha256]
            .iter()
            .any(|h| h.len() != 64 || !h.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Err("invalid source SHA-256".into());
        }
        minimum_zero_failures(self.min_success, self.confidence)?;
        Ok(())
    }
    pub fn hash(&self) -> Result<String, Error> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        Ok(sha256(&bytes))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanPoint {
    pub value_ms: f64,
    pub successes: u64,
    pub attempts: u64,
}
pub fn reliable_ranges(
    samples: &[ScanPoint],
    target: f64,
    confidence: f64,
) -> Result<Vec<[f64; 2]>, Error> {
    minimum_zero_failures(target, confidence)?;
    let mut samples = samples.to_vec();
    samples.sort_by(|a, b| a.value_ms.total_cmp(&b.value_ms));
    if samples
        .iter()
        .any(|p| !p.value_ms.is_finite() || p.value_ms < 0.)
        || samples.windows(2).any(|w| w[0].value_ms == w[1].value_ms)
    {
        return Err("invalid or duplicate scan points".into());
    }
    let mut ranges = Vec::new();
    let mut start = None;
    let mut end = 0.;
    for point in samples {
        if wilson(point.successes, point.attempts, confidence)?.lower >= target {
            start.get_or_insert(point.value_ms);
            end = point.value_ms;
        } else if let Some(first) = start.take() {
            ranges.push([first, end]);
        }
    }
    if let Some(first) = start {
        ranges.push([first, end]);
    }
    Ok(ranges)
}
pub fn refine_boundaries(
    samples: &[ScanPoint],
    target: f64,
    confidence: f64,
    step: f64,
) -> Result<Vec<f64>, Error> {
    if !step.is_finite() || step <= 0. {
        return Err("positive fine step required".into());
    }
    reliable_ranges(samples, target, confidence)?;
    let mut samples = samples.to_vec();
    samples.sort_by(|a, b| a.value_ms.total_cmp(&b.value_ms));
    let mut result = Vec::new();
    for pair in samples.windows(2) {
        let a = wilson(pair[0].successes, pair[0].attempts, confidence)?.lower >= target;
        let b = wilson(pair[1].successes, pair[1].attempts, confidence)?.lower >= target;
        if a != b {
            let count = ((pair[1].value_ms - pair[0].value_ms) / step).ceil() as usize;
            if count > 10000 {
                return Err("fine grid too large".into());
            }
            for i in 1..count {
                let value = pair[0].value_ms + i as f64 * step;
                if value < pair[1].value_ms {
                    result.push(value);
                }
            }
        }
    }
    Ok(result)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialEvidence {
    pub plan_hash: String,
    pub identity: ProfileIdentity,
    pub min_success: f64,
    pub confidence: f64,
    pub detector_validated: bool,
    pub detector_error: f64,
    pub degraded_flags: u32,
    pub samples: Vec<ScanPoint>,
    pub margin_ms: f64,
    pub selected_ms: f64,
    pub p999_late_ms: Option<f64>,
    pub measured_fps: Option<f64>,
    pub outcomes_sha256: Option<String>,
}
impl TrialEvidence {
    pub fn verified(&self, plan: &Plan) -> Result<bool, Error> {
        plan.validate()?;
        let identity_complete = [
            &plan.identity.game_version,
            &plan.identity.fps,
            &plan.identity.sync,
            &plan.identity.machine,
            &plan.identity.conditions,
        ]
        .iter()
        .all(|s| !s.trim().is_empty());
        let coarse_complete = plan.coarse_ms.iter().all(|value| {
            self.samples
                .iter()
                .any(|s| s.value_ms == *value && s.attempts >= plan.attempts)
        });
        let ranges = reliable_ranges(&self.samples, plan.min_success, plan.confidence)?;
        let safe_choice = self.selected_ms.is_finite()
            && self.samples.iter().any(|s| s.value_ms == self.selected_ms)
            && self.margin_ms.is_finite()
            && self.margin_ms >= 0.
            && ranges.iter().any(|r| {
                self.selected_ms - self.margin_ms >= r[0]
                    && self.selected_ms + self.margin_ms <= r[1]
            });
        Ok(self.plan_hash == plan.hash()?
            && self.identity == plan.identity
            && identity_complete
            && coarse_complete
            && safe_choice
            && self.min_success == plan.min_success
            && self.confidence == plan.confidence
            && self
                .outcomes_sha256
                .as_ref()
                .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            && self.detector_validated
            && self.detector_error.is_finite()
            && self.detector_error <= plan.max_detector_error
            && self.measured_fps.is_some_and(|v| v.is_finite() && v > 0.)
            && self.degraded_flags == 0
            && self
                .p999_late_ms
                .is_some_and(|v| v.is_finite() && v >= 0. && self.margin_ms >= v))
    }
}
pub fn markdown_scan(plan: &Plan, samples: &[ScanPoint]) -> Result<String, Error> {
    use std::fmt::Write;
    let mut report = format!(
        "# 校準掃描（未驗證）\n\n計畫 SHA-256：{}\n\n| {} ms | 成功／有效嘗試 | Wilson 下界 | Wilson 上界 |\n|---:|---:|---:|---:|\n",
        plan.hash()?,
        plan.parameter
    );
    for p in samples {
        let ci = wilson(p.successes, p.attempts, plan.confidence)?;
        let _ = writeln!(
            report,
            "| {} | {} / {} | {:.6} | {:.6} |",
            p.value_ms, p.successes, p.attempts, ci.lower, ci.upper
        );
    }
    Ok(report)
}
pub struct PhaseRng(u64);
impl PhaseRng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    pub fn next_phase(&mut self, frame_ticks: u64) -> u64 {
        if frame_ticks == 0 {
            return 0;
        }
        let limit = u64::MAX - u64::MAX % frame_ticks;
        loop {
            let n = self.next();
            if n < limit {
                return n % frame_ticks;
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AbResult {
    pub label: String,
    pub tail_us: f64,
    pub frame_p99_ms: f64,
    pub cpu_percent: f64,
    pub stop_p99_us: f64,
    pub lost: u64,
    pub hook_failures: u64,
}
pub fn keep_experiment(base: &AbResult, variant: &AbResult) -> bool {
    [
        base.tail_us,
        base.frame_p99_ms,
        base.cpu_percent,
        base.stop_p99_us,
        variant.tail_us,
        variant.frame_p99_ms,
        variant.cpu_percent,
        variant.stop_p99_us,
    ]
    .iter()
    .all(|v| v.is_finite() && *v >= 0.)
        && variant.tail_us < base.tail_us
        && variant.frame_p99_ms <= base.frame_p99_ms
        && variant.cpu_percent <= base.cpu_percent
        && variant.stop_p99_us <= base.stop_p99_us
        && variant.lost <= base.lost
        && variant.hook_failures <= base.hook_failures
}
#[derive(Clone, Debug)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub present: Ticks,
}
impl Pixels {
    pub fn valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && (self.width as usize)
                .checked_mul(self.height as usize)
                .and_then(|n| n.checked_mul(4))
                == Some(self.bgra.len())
    }
    pub fn mean_difference(&self, baseline: &Self, roi: HandRect) -> Result<f64, Error> {
        if !self.valid()
            || !baseline.valid()
            || self.width != baseline.width
            || self.height != baseline.height
            || !roi.valid()
        {
            return Err("incompatible frames or invalid ROI".into());
        }
        let left = (roi.x[0] * self.width as f64).floor() as u32;
        let right = (roi.x[1] * self.width as f64).ceil() as u32;
        let top = (roi.y[0] * self.height as f64).floor() as u32;
        let bottom = (roi.y[1] * self.height as f64).ceil() as u32;
        let mut total = 0u64;
        let mut count = 0;
        for y in top..bottom {
            for x in left..right {
                let start = (y as usize * self.width as usize + x as usize) * 4;
                for c in 0..3 {
                    total += u64::from(self.bgra[start + c].abs_diff(baseline.bgra[start + c]));
                    count += 1;
                }
            }
        }
        if count == 0 {
            return Err("empty ROI".into());
        }
        Ok(total as f64 / count as f64)
    }
    pub fn bmp(&self) -> Result<Vec<u8>, Error> {
        if !self.valid() || self.width > i32::MAX as u32 || self.height > i32::MAX as u32 {
            return Err("invalid bitmap".into());
        }
        let size = u32::try_from(self.bgra.len()).map_err(|_| "bitmap too large")?;
        let mut b = Vec::with_capacity(54 + self.bgra.len());
        b.extend(b"BM");
        b.extend(
            size.checked_add(54)
                .ok_or("bitmap too large")?
                .to_le_bytes(),
        );
        b.extend([0; 4]);
        b.extend(54u32.to_le_bytes());
        b.extend(40u32.to_le_bytes());
        b.extend((self.width as i32).to_le_bytes());
        b.extend((-(self.height as i32)).to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(32u16.to_le_bytes());
        b.extend([0; 4]);
        b.extend(size.to_le_bytes());
        b.extend([0; 16]);
        b.extend(&self.bgra);
        Ok(b)
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest.iter() {
        write!(out, "{b:02x}").unwrap();
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Observation {
    Valid,
    Missed,
    Misoperation,
    WrongOrder,
    Duplicate,
    Ineligible,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrialRow {
    pub id: u64,
    pub value_ms: f64,
    pub phase_ticks: u64,
    pub started: u64,
    pub finished: u64,
    pub changed_qpc: Option<u64>,
    pub detected: bool,
    pub eligible: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectorValidation {
    pub plan_hash: String,
    pub labels_sha256: String,
    pub confusion: Confusion,
    pub error_upper: f64,
    pub confidence: f64,
    pub valid: bool,
}
pub fn validate_labels(plan: &Plan, labels_text: &str) -> Result<DetectorValidation, Error> {
    let rows = csv_rows(labels_text)?;
    let h = rows.first().ok_or("empty labels CSV")?;
    let col = |s: &str| {
        h.iter()
            .position(|c| c == s)
            .ok_or_else(|| format!("missing {s} label column"))
    };
    let detected = col("detected")?;
    let label = col("label")?;
    let mut labels = Vec::new();
    for row in rows.iter().skip(1) {
        if row.len() != h.len() {
            return Err("labels column mismatch".into());
        }
        let truth = match row[label].as_str() {
            "Valid" => true,
            "Ineligible" => continue,
            "Missed" | "Misoperation" | "WrongOrder" | "Duplicate" => false,
            _ => return Err("unknown human label".into()),
        };
        let detector = match row[detected].as_str() {
            "true" => true,
            "false" => false,
            _ => return Err("detected must be true/false".into()),
        };
        labels.push((detector, truth));
    }
    let c = confusion(&labels);
    let n = c.tp + c.fp + c.fn_ + c.tn;
    let error_upper = wilson(c.fp + c.fn_, n, plan.confidence)?.upper;
    Ok(DetectorValidation {
        plan_hash: plan.hash()?,
        labels_sha256: sha256(labels_text.as_bytes()),
        confusion: c,
        error_upper,
        confidence: plan.confidence,
        valid: error_upper <= plan.max_detector_error,
    })
}
pub fn trial_timing(
    base: TickTiming,
    param: &str,
    value_ms: f64,
    rate: TickRate,
) -> Result<TickTiming, Error> {
    let value = ticks_ms(value_ms, rate).map_err(|e| e.to_string())?;
    let mut t = base;
    *t.gap_mut(param).ok_or("unknown timing parameter")? = value;
    Ok(t)
}
pub fn choose_with_margin(
    samples: &[ScanPoint],
    plan: &Plan,
    margin: f64,
) -> Result<Option<f64>, Error> {
    if !margin.is_finite() || margin < 0. {
        return Err("invalid margin".into());
    }
    let ranges = reliable_ranges(samples, plan.min_success, plan.confidence)?;
    Ok(ranges
        .iter()
        .flat_map(|r| {
            samples
                .iter()
                .filter(move |s| s.value_ms - margin >= r[0] && s.value_ms + margin <= r[1])
        })
        .min_by(|a, b| a.value_ms.total_cmp(&b.value_ms))
        .map(|s| s.value_ms))
}
pub fn calibrated_profile(
    plan: &Plan,
    evidence: &TrialEvidence,
    old: &TimingProfile,
) -> Result<TimingProfile, Error> {
    if !evidence.verified(plan)? {
        return Err("calibration gates incomplete; profile remains unverified".into());
    }
    // Only the calibrated gap is written back; every other set stays as it was.
    let mut profile = old.clone();
    *profile
        .gap_mut(&plan.parameter)
        .ok_or("unknown parameter")? = evidence.selected_ms;
    profile.identity = evidence.identity.clone();
    profile.identity.fps = format!("{:.3} (PresentMon median)", evidence.measured_fps.unwrap());
    profile.identity.conditions = format!(
        "{}; mode={:?}; action={:?}; parameter={}; plan={}",
        profile.identity.conditions, plan.mode, plan.action, plan.parameter, evidence.plan_hash
    );
    profile.source = ProfileSource::Calibrated;
    profile.verified = true;
    Ok(profile)
}
impl Pixels {
    pub fn crop(&self, roi: HandRect) -> Result<Self, Error> {
        if !self.valid() || !roi.valid() {
            return Err("invalid crop".into());
        }
        let x0 = (roi.x[0] * self.width as f64).floor() as usize;
        let x1 = (roi.x[1] * self.width as f64).ceil() as usize;
        let y0 = (roi.y[0] * self.height as f64).floor() as usize;
        let y1 = (roi.y[1] * self.height as f64).ceil() as usize;
        let mut bgra = Vec::new();
        for y in y0..y1 {
            bgra.extend_from_slice(
                &self.bgra[(y * self.width as usize + x0) * 4..(y * self.width as usize + x1) * 4],
            );
        }
        Ok(Self {
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
            bgra,
            present: self.present,
        })
    }
}

/// Independent manual outcomes preserve all failure categories and exclude only game-ineligible trials.
pub fn classified_samples(plan: &Plan, csv: &str) -> Result<Vec<ScanPoint>, Error> {
    plan.validate()?;
    let rows = csv_rows(csv)?;
    let header = rows.first().ok_or("empty trials CSV")?;
    let column = |name: &str| {
        header
            .iter()
            .position(|h| h == name)
            .ok_or_else(|| format!("missing {name}"))
    };
    let (id, value, label) = (column("id")?, column("value_ms")?, column("label")?);
    let mut ids = std::collections::BTreeSet::new();
    let mut samples: Vec<ScanPoint> = Vec::new();
    for row in rows.iter().skip(1) {
        if row.len() != header.len() {
            return Err("trial column mismatch".into());
        }
        let trial_id: u64 = row[id].parse().map_err(|_| "invalid trial id")?;
        if trial_id == 0 || !ids.insert(trial_id) {
            return Err("duplicate/zero trial id".into());
        }
        let v: f64 = row[value].parse().map_err(|_| "invalid scan value")?;
        if !v.is_finite() || v < 0. {
            return Err("invalid scan value".into());
        }
        let success = match row[label].as_str() {
            "Valid" => true,
            "Ineligible" => continue,
            "Missed" | "Misoperation" | "WrongOrder" | "Duplicate" => false,
            _ => return Err("every trial requires an independent human outcome".into()),
        };
        let index = if let Some(i) = samples.iter().position(|s| s.value_ms == v) {
            i
        } else {
            samples.push(ScanPoint {
                value_ms: v,
                successes: 0,
                attempts: 0,
            });
            samples.len() - 1
        };
        samples[index].attempts += 1;
        samples[index].successes += u64::from(success);
    }
    samples.sort_by(|a, b| a.value_ms.total_cmp(&b.value_ms));
    Ok(samples)
}
