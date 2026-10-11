//! Only fault-injection builds execute these deliberately invalid operating conditions.
#![forbid(unsafe_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultAction {
    Panic,
    Abort,
    Hang(u32),
    Short,
    PipeDrop,
    Unhook,
    HookStall,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultRule {
    pub batch: u64,
    pub action: FaultAction,
}
impl std::str::FromStr for FaultRule {
    type Err = &'static str;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text == "hook-stall" {
            return Ok(Self {
                batch: 0,
                action: FaultAction::HookStall,
            });
        }
        let (kind, batch) = text.split_once('@').ok_or("fault rule requires @N")?;
        let batch = batch.parse::<u64>().map_err(|_| "invalid batch number")?;
        if batch == 0 {
            return Err("batch must be positive");
        }
        let action = match kind {
            "panic" => FaultAction::Panic,
            "abort" => FaultAction::Abort,
            "short" => FaultAction::Short,
            "pipe-drop" => FaultAction::PipeDrop,
            "unhook" => FaultAction::Unhook,
            _ if kind.starts_with("hang:") => {
                let millis = kind
                    .strip_prefix("hang:")
                    .unwrap()
                    .strip_suffix("ms")
                    .ok_or("hang requires milliseconds")?
                    .parse::<u32>()
                    .map_err(|_| "invalid hang duration")?;
                if millis == 0 {
                    return Err("hang duration must be positive");
                }
                FaultAction::Hang(millis)
            }
            _ => return Err("unknown fault injection"),
        };
        Ok(Self { batch, action })
    }
}
#[cfg(feature = "fault-injection")]
pub static RULE: std::sync::OnceLock<FaultRule> = std::sync::OnceLock::new();
#[cfg(feature = "fault-injection")]
pub static PIPE_DROP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "fault-injection")]
pub static HOOK_THREAD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
