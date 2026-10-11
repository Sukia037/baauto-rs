use eframe::egui;
/// Startup only; system collection is read before any GUI frame or input hook.
pub fn install(ctx: &egui::Context) {
    let mut definitions = egui::FontDefinitions::default();
    definitions.font_data.insert(
        "BAauto TC fallback".into(),
        egui::FontData::from_static(include_bytes!("../assets/NotoSansTC-Regular-subset.ttf"))
            .into(),
    );
    #[cfg(windows)]
    {
        let root = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:/Windows".into());
        if let Ok(bytes) = std::fs::read(root.join("Fonts/msjh.ttc")) {
            // MSJH collection contains JhengHei and JhengHei UI; select the UI family by its SFNT name instead of assuming a collection index.
            if let Some(index) = jhenghei_ui_index(&bytes) {
                let mut data = egui::FontData::from_owned(bytes);
                data.index = index;
                definitions
                    .font_data
                    .insert("Microsoft JhengHei UI".into(), data.into());
                definitions
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "Microsoft JhengHei UI".into());
                definitions
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .push("Microsoft JhengHei UI".into());
            }
        }
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push("BAauto TC fallback".into());
    }
    ctx.set_fonts(definitions);
}
/// Checked SFNT name-table reader, including TTC absolute table offsets. No unsafe/font parser in hot paths.
#[cfg(windows)]
fn jhenghei_ui_index(bytes: &[u8]) -> Option<u32> {
    let u16_at = |i: usize| -> Option<u16> {
        Some(u16::from_be_bytes(
            bytes.get(i..i.checked_add(2)?)?.try_into().ok()?,
        ))
    };
    let u32_at = |i: usize| -> Option<u32> {
        Some(u32::from_be_bytes(
            bytes.get(i..i.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    let count = if bytes.get(..4)? == b"ttcf" {
        u32_at(8)?
    } else {
        1
    };
    if count > 64 {
        return None;
    }
    for index in 0..count {
        let base = if bytes.get(..4)? == b"ttcf" {
            u32_at(12 + index as usize * 4)? as usize
        } else {
            0
        };
        let tables = u16_at(base + 4)?;
        if tables > 256 {
            return None;
        }
        for t in 0..tables as usize {
            let table = base + 12 + t * 16;
            if bytes.get(table..table + 4) != Some(b"name") {
                continue;
            }
            let name = u32_at(table + 8)? as usize;
            let records = u16_at(name + 2)? as usize;
            let storage = name + u16_at(name + 4)? as usize;
            if records > 4096 {
                return None;
            }
            for r in 0..records {
                let entry = name + 6 + r * 12;
                if u16_at(entry)? != 3 || !matches!(u16_at(entry + 6)?, 1 | 16) {
                    continue;
                }
                let length = u16_at(entry + 8)? as usize;
                let start = storage + u16_at(entry + 10)? as usize;
                let raw = bytes.get(start..start.checked_add(length)?)?;
                let units: Vec<_> = raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| u16::from_be_bytes([p[0], p[1]]))
                    .collect();
                if String::from_utf16(&units)
                    .ok()?
                    .eq_ignore_ascii_case("Microsoft JhengHei UI")
                {
                    return Some(index);
                }
            }
        }
    }
    None
}
