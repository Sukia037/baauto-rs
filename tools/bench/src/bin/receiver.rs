#![forbid(unsafe_code)]
#[cfg(not(windows))]
fn main() {
    eprintln!("baauto-receiver requires an interactive Windows desktop.");
}
#[cfg(windows)]
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use baauto_config::*;
    use std::path::PathBuf;
    let mut mode = Mode::Six;
    let mut side = CardUiSide::Right;
    let mut output = PathBuf::from("receiver.csv");
    let mut guide = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--hand") => {
                mode = match args.next().and_then(|v| v.into_string().ok()).as_deref() {
                    Some("six") => Mode::Six,
                    Some("ten") => Mode::Ten,
                    _ => return Err("--hand expects six or ten".into()),
                }
            }
            Some("--side") => {
                side = match args.next().and_then(|v| v.into_string().ok()).as_deref() {
                    Some("right") => CardUiSide::Right,
                    Some("left") => CardUiSide::Left,
                    _ => return Err("--side expects right or left".into()),
                }
            }
            Some("--output") => output = args.next().ok_or("missing CSV path")?.into(),
            Some("--guide") => {
                guide = Some(
                    args.next()
                        .and_then(|v| v.into_string().ok())
                        .ok_or("--guide expects seconds")?
                        .parse()?,
                )
            }
            _ => return Err("unknown receiver argument".into()),
        }
    }
    baauto_win_platform::run_receiver(mode, side, &output, guide)?;
    Ok(())
}
