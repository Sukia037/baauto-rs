use baauto_config::NormPoint;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}
impl Rect {
    pub fn valid(self) -> bool {
        self.width > 0
            && self.height > 0
            && self.left.checked_add(self.width - 1).is_some()
            && self.top.checked_add(self.height - 1).is_some()
    }
    pub fn inside(self, x: i32, y: i32) -> bool {
        self.valid()
            && i64::from(x) >= i64::from(self.left)
            && i64::from(x) < i64::from(self.left) + i64::from(self.width)
            && i64::from(y) >= i64::from(self.top)
            && i64::from(y) < i64::from(self.top) + i64::from(self.height)
    }
    pub fn norm(self, x: i32, y: i32) -> NormPoint {
        NormPoint {
            x: (f64::from(x) - f64::from(self.left)) / f64::from(self.width),
            y: (f64::from(y) - f64::from(self.top)) / f64::from(self.height),
        }
    }
    pub fn pixel(self, p: NormPoint) -> Option<(i32, i32)> {
        if !self.valid() || !p.valid() {
            return None;
        }
        Some((
            self.left + ((p.x * f64::from(self.width)).round() as i32).min(self.width - 1),
            self.top + ((p.y * f64::from(self.height)).round() as i32).min(self.height - 1),
        ))
    }
    pub fn fullscreen_16x9(self, window: Self) -> bool {
        self == window && i64::from(self.width) * 9 == i64::from(self.height) * 16
    }
}
/// Chooses the first representable absolute coordinate in a virtual-desktop pixel; Windows uses floor(abs * dimension / 65536).
pub fn absolute_for_pixel(desktop: Rect, x: i32, y: i32) -> Option<(i32, i32)> {
    if !desktop.inside(x, y) || desktop.width > 65536 || desktop.height > 65536 {
        return None;
    }
    let convert = |p: i32, origin: i32, size: i32| -> i32 {
        (((i64::from(p) - i64::from(origin)) as u64) * 65536).div_ceil(size as u64) as i32
    };
    Some((
        convert(x, desktop.left, desktop.width),
        convert(y, desktop.top, desktop.height),
    ))
}
pub fn pixel_for_absolute(desktop: Rect, x: i32, y: i32) -> (i32, i32) {
    (
        desktop.left + (i64::from(x) * i64::from(desktop.width) / 65536) as i32,
        desktop.top + (i64::from(y) * i64::from(desktop.height) / 65536) as i32,
    )
}
