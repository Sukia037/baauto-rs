use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(pub u8);
impl Key {
    pub const ESC: Self = Self(0x1b);
    pub const LEFT: Self = Self(1);
    pub fn is_mouse(self) -> bool {
        matches!(self.0, 1 | 2 | 4 | 5 | 6)
    }
}
impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            1 => "LButton",
            2 => "RButton",
            4 => "MButton",
            5 => "XButton1",
            6 => "XButton2",
            8 => "Backspace",
            9 => "Tab",
            13 => "Enter",
            16 => "Shift",
            17 => "Ctrl",
            18 => "Alt",
            27 => "Esc",
            32 => "Space",
            33 => "PgUp",
            34 => "PgDn",
            35 => "End",
            36 => "Home",
            37 => "Left",
            38 => "Up",
            39 => "Right",
            40 => "Down",
            45 => "Insert",
            46 => "Delete",
            91 => "Win",
            _ => "",
        };
        if !name.is_empty() {
            f.write_str(name)
        } else if self.0.is_ascii_uppercase() {
            write!(f, "{}", char::from(self.0).to_ascii_lowercase())
        } else if self.0.is_ascii_digit() {
            write!(f, "{}", char::from(self.0))
        } else if (112..=135).contains(&self.0) {
            write!(f, "F{}", self.0 - 111)
        } else {
            write!(f, "VK{:02X}", self.0)
        }
    }
}
impl FromStr for Key {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();
        let vk = match lower.as_str() {
            "lbutton" => 1,
            "rbutton" => 2,
            "mbutton" => 4,
            "xbutton1" => 5,
            "xbutton2" => 6,
            "backspace" => 8,
            "tab" => 9,
            "enter" => 13,
            "shift" => 16,
            "ctrl" => 17,
            "alt" => 18,
            "esc" | "escape" => 27,
            "space" => 32,
            "pgup" => 33,
            "pgdn" => 34,
            "end" => 35,
            "home" => 36,
            "left" => 37,
            "up" => 38,
            "right" => 39,
            "down" => 40,
            "insert" => 45,
            "delete" => 46,
            "win" => 91,
            _ => 0,
        };
        if vk != 0 {
            return Ok(Self(vk));
        }
        if s.len() == 1 && s.as_bytes()[0].is_ascii_alphanumeric() {
            return Ok(Self(s.as_bytes()[0].to_ascii_uppercase()));
        }
        if let Some(n) = lower
            .strip_prefix('f')
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=24).contains(n))
        {
            return Ok(Self(111 + n));
        }
        if let Some(n) = lower
            .strip_prefix("vk")
            .and_then(|n| u8::from_str_radix(n, 16).ok())
            .filter(|n| *n != 0)
        {
            return Ok(Self(n));
        }
        Err(format!("unknown key: {s}"))
    }
}
impl Serialize for Key {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModsMatch {
    AtLeast(u8),
    Exact(u8),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key: Key,
    pub mods: ModsMatch,
}
impl Chord {
    pub fn matches(self, mods: u8) -> bool {
        match self.mods {
            ModsMatch::AtLeast(m) => mods & m == m,
            ModsMatch::Exact(m) => mods == m,
        }
    }
    pub fn overlaps(self, other: Self) -> bool {
        self.key == other.key && (0..16).any(|m| self.matches(m) && other.matches(m))
    }
}
impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = match self.mods {
            ModsMatch::AtLeast(m) => m,
            ModsMatch::Exact(m) => {
                f.write_str("=")?;
                m
            }
        };
        for (bit, name) in [(1, "Ctrl"), (2, "Shift"), (4, "Alt"), (8, "Win")] {
            if m & bit != 0 {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", self.key)
    }
}
impl FromStr for Chord {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (exact, s) = s.strip_prefix('=').map_or((false, s), |s| (true, s));
        let mut parts = s.split('+').peekable();
        let mut mods = 0;
        while let Some(p) = parts.next() {
            if parts.peek().is_none() {
                return Ok(Self {
                    key: p.parse()?,
                    mods: if exact {
                        ModsMatch::Exact(mods)
                    } else {
                        ModsMatch::AtLeast(mods)
                    },
                });
            }
            let bit = match p.to_ascii_lowercase().as_str() {
                "ctrl" => 1,
                "shift" => 2,
                "alt" => 4,
                "win" => 8,
                _ => return Err(format!("unknown modifier: {p}")),
            };
            if mods & bit != 0 {
                return Err("duplicate modifier".into());
            }
            mods |= bit;
        }
        Err("empty chord".into())
    }
}
impl Serialize for Chord {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for Chord {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
