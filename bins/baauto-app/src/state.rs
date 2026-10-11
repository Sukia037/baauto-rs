//! UI-independent state. File operations are sequenced only after stop acknowledgement.
use baauto_config::{store::FileKind, *};
use std::path::PathBuf;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Idle,
    AskUnsaved,
    Stop,
    Save(FileKind),
    Load(FileKind, PathBuf),
    Apply,
    WaitReleased,
    Arm,
    Failed,
}
#[derive(Clone, Copy, Debug)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}
#[derive(Default)]
pub struct FileFlow {
    pub points_dirty: bool,
    pub keys_dirty: bool,
    pub armed: bool,
    pending: Option<(FileKind, PathBuf)>,
    save: bool,
    phase: u8,
    resume: bool,
}
impl FileFlow {
    pub fn request(&mut self, kind: FileKind, path: PathBuf) -> Step {
        if self.phase != 0 {
            return Step::Idle;
        }
        self.pending = Some((kind, path));
        self.resume = self.armed;
        if match kind {
            FileKind::Points => self.points_dirty,
            FileKind::Keys => self.keys_dirty,
        } {
            self.phase = 1;
            Step::AskUnsaved
        } else {
            self.phase = 2;
            Step::Stop
        }
    }
    pub fn choose(&mut self, choice: UnsavedChoice, can_save: bool) -> Step {
        if self.phase != 1 {
            return Step::Idle;
        }
        match choice {
            UnsavedChoice::Cancel => {
                self.pending = None;
                self.phase = 0;
                Step::Idle
            }
            UnsavedChoice::Save if !can_save => Step::AskUnsaved,
            _ => {
                self.save = matches!(choice, UnsavedChoice::Save);
                self.phase = 2;
                Step::Stop
            }
        }
    }
    pub fn stopped(&mut self) -> Step {
        if self.phase != 2 {
            return Step::Idle;
        }
        self.armed = false;
        self.phase = 3;
        let (k, p) = self.pending.clone().unwrap();
        if self.save {
            Step::Save(k)
        } else {
            Step::Load(k, p)
        }
    }
    pub fn saved(&mut self, success: bool) -> Step {
        if self.phase != 3 {
            return Step::Idle;
        }
        if !success {
            self.phase = 0;
            self.pending = None;
            return Step::Failed;
        }
        let (k, p) = self.pending.clone().unwrap();
        match k {
            FileKind::Points => self.points_dirty = false,
            FileKind::Keys => self.keys_dirty = false,
        }
        Step::Load(k, p)
    }
    pub fn loaded(&mut self, success: bool) -> Step {
        if self.phase != 3 {
            return Step::Idle;
        }
        if !success {
            self.pending = None;
            self.phase = 0;
            return Step::Failed;
        }
        let k = self.pending.as_ref().unwrap().0;
        match k {
            FileKind::Points => self.points_dirty = false,
            FileKind::Keys => self.keys_dirty = false,
        }
        self.phase = 4;
        Step::Apply
    }
    pub fn installed(&mut self) -> Step {
        if self.phase == 4 {
            self.phase = 5;
            Step::WaitReleased
        } else {
            Step::Idle
        }
    }
    pub fn keys_released(&mut self) -> Step {
        if self.phase != 5 {
            return Step::Idle;
        }
        self.phase = 0;
        self.pending = None;
        if self.resume { Step::Arm } else { Step::Idle }
    }
}
pub fn selected_item(p: &PointSet, id: u32) -> Option<&PointItem> {
    p.items.iter().find(|i| i.id == id)
}
pub fn sorted_item_ids(p: &PointSet, k: &KeysProfile) -> Vec<u32> {
    let conflicts = editor::analyze(p, k);
    let mut ids: Vec<_> = p.items.iter().map(|i| i.id).collect();
    ids.sort_by_key(|id| (!conflicts.disabled_points.contains(id), *id));
    ids
}
pub fn can_save_keys(p: &PointSet, k: &KeysProfile) -> bool {
    compile(p, k, &HandSettings::default(), TickRate(1_000_000)).is_ok()
}
pub fn mirror_rect(r: HandRect) -> HandRect {
    HandRect {
        x: [1. - r.x[1], 1. - r.x[0]],
        calibrated: false,
        ..r
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Corner {
    TopLeft,
    BottomRight,
    TopRight,
    BottomLeft,
    Left,
    Right,
    Top,
    Bottom,
}
pub fn drag_corner(mut r: HandRect, c: Corner, p: NormPoint) -> Result<HandRect, &'static str> {
    match c {
        Corner::TopRight => {
            r.x[1] = p.x;
            r.y[0] = p.y;
        }
        Corner::BottomLeft => {
            r.x[0] = p.x;
            r.y[1] = p.y;
        }
        Corner::Left => r.x[0] = p.x,
        Corner::Right => r.x[1] = p.x,
        Corner::Top => r.y[0] = p.y,
        Corner::Bottom => r.y[1] = p.y,
        Corner::TopLeft => {
            r.x[0] = p.x;
            r.y[0] = p.y
        }
        Corner::BottomRight => {
            r.x[1] = p.x;
            r.y[1] = p.y
        }
    }
    r.calibrated = true;
    if r.valid() {
        Ok(r)
    } else {
        Err("框超出螢幕或邊界交錯")
    }
}
pub fn rect_from_centers(
    first: NormPoint,
    last: NormPoint,
    slots: u8,
    height: f64,
) -> Result<HandRect, &'static str> {
    if slots < 2 || !height.is_finite() || height <= 0. || first.x >= last.x {
        return Err("中心順序或高度無效");
    }
    let width = (last.x - first.x) / f64::from(slots - 1);
    let r = HandRect {
        x: [first.x - width / 2., last.x + width / 2.],
        y: [
            (first.y + last.y - height) / 2.,
            (first.y + last.y + height) / 2.,
        ],
        calibrated: true,
    };
    if r.valid() {
        Ok(r)
    } else {
        Err("手牌框超出螢幕")
    }
}
