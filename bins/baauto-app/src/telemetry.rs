//! Background observer writes raw QPC records. It never sends input or changes Engine state.
#![cfg(windows)]
use crate::backend::UiEvent;
use baauto_protocol::shm::RingReader;
use baauto_win_platform::{Clock, Mapping, SupervisorCommand};
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering::SeqCst},
        mpsc,
    },
    thread,
    time::Duration,
};
pub struct Telemetry {
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}
impl Telemetry {
    pub fn start(
        tx: mpsc::Sender<SupervisorCommand>,
        events: mpsc::SyncSender<UiEvent>,
        folder: PathBuf,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        let join = thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                let clock = Clock::new().map_err(|e| e.to_string())?;
                let mut current: Option<Arc<Mapping>> = None;
                let mut reader = RingReader::default();
                let mut file: Option<std::io::BufWriter<std::fs::File>> = None;
                let mut path = PathBuf::new();
                let mut flushed = clock.now();
                while !signal.load(SeqCst) {
                    let (reply, rx) = mpsc::channel();
                    if tx.send(SupervisorCommand::Inspect { reply }).is_err() {
                        break;
                    }
                    if let Ok((mapping, _)) = rx.recv_timeout(Duration::from_millis(200)) {
                        if current
                            .as_ref()
                            .is_none_or(|old| !Arc::ptr_eq(old, &mapping))
                        {
                            if let (Some(map), Some(old)) = (&current, file.as_mut()) {
                                while let Some((seq, data)) = reader.read(&map.data().ring) {
                                    write!(old, "{seq}").map_err(|e| e.to_string())?;
                                    for value in data.0 {
                                        write!(old, ",{value}").map_err(|e| e.to_string())?;
                                    }
                                    writeln!(old).map_err(|e| e.to_string())?;
                                }
                                old.flush().map_err(|e| e.to_string())?;
                            }
                            if !path.as_os_str().is_empty() {
                                metadata(&path, reader.lost, clock.rate.0)?;
                            }
                            path = folder.join(format!(
                                "run-{}-{}.csv",
                                std::process::id(),
                                clock.now().0
                            ));
                            let mut writer = std::io::BufWriter::new(
                                std::fs::File::create(&path).map_err(|e| e.to_string())?,
                            );
                            writeln!(writer, "seq,kind,session,action,stage,hook_enter,enqueue,engine_receive,deadline,send_begin,send_end,inserted,expected,queue_depth,config_version,geometry_version,source_seq").map_err(|e| e.to_string())?;
                            file = Some(writer);
                            reader = RingReader::default();
                            current = Some(mapping.clone());
                        }
                        if let Some(writer) = file.as_mut() {
                            for _ in 0..65_536 {
                                let Some((seq, data)) = reader.read(&mapping.data().ring) else {
                                    break;
                                };
                                write!(writer, "{seq}").map_err(|e| e.to_string())?;
                                for value in data.0 {
                                    write!(writer, ",{value}").map_err(|e| e.to_string())?;
                                }
                                writeln!(writer).map_err(|e| e.to_string())?;
                            }
                            if clock.now().0.saturating_sub(flushed.0) >= clock.rate.0 {
                                writer.flush().map_err(|e| e.to_string())?;
                                metadata(&path, reader.lost, clock.rate.0)?;
                                flushed = clock.now();
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                // Read the final records after confirmed supervisor shutdown, then flush.
                if let (Some(mapping), Some(writer)) = (current, file.as_mut()) {
                    while let Some((seq, data)) = reader.read(&mapping.data().ring) {
                        write!(writer, "{seq}").map_err(|e| e.to_string())?;
                        for value in data.0 {
                            write!(writer, ",{value}").map_err(|e| e.to_string())?;
                        }
                        writeln!(writer).map_err(|e| e.to_string())?;
                    }
                    writer.flush().map_err(|e| e.to_string())?;
                    metadata(&path, reader.lost, clock.rate.0)?;
                }
                Ok(())
            })();
            if let Err(e) = result {
                let _ = events.send(UiEvent::Message(format!(
                    "紀錄寫入失敗：{e}；本次量測不可驗收"
                )));
            }
        });
        Self {
            stop,
            join: Some(join),
        }
    }
}
fn metadata(path: &std::path::Path, lost: u64, frequency: u64) -> Result<(), String> {
    let meta = serde_json::json!({"qpc_frequency": frequency, "lost_records": lost, "source":"GUI background observer; raw QPC"});
    std::fs::write(
        path.with_extension("json"),
        serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
impl Drop for Telemetry {
    fn drop(&mut self) {
        self.stop.store(true, SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
