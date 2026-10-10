use crate::*;
#[derive(Clone, Copy, Debug, Default)]
pub struct FakeClock(pub Ticks);
impl FakeClock {
    pub fn advance(&mut self, ticks: u64) {
        self.0 = self.0.after(ticks);
    }
}
#[derive(Clone, Debug)]
pub struct Record {
    pub batch_id: u64,
    pub action: Option<ActionId>,
    pub stage: u32,
    pub deadline: Ticks,
    pub begin: Ticks,
    pub end: Ticks,
}
#[derive(Default)]
pub struct FakeDriver {
    pub records: Vec<Record>,
    pub late_batch: Option<(u64, u64)>,
    pub short_batch: Option<(u64, u16)>,
}
impl FakeDriver {
    pub fn submit(&mut self, b: &Batch, now: Ticks) -> SubmitOutcome {
        let end = now.after(
            self.late_batch
                .filter(|(id, _)| *id == b.id)
                .map_or(0, |(_, late)| late),
        );
        self.records.push(Record {
            batch_id: b.id,
            action: b.action,
            stage: b.stage,
            deadline: b.deadline,
            begin: now,
            end,
        });
        if let Some((_, inserted)) = self.short_batch.filter(|(id, _)| *id == b.id) {
            SubmitOutcome::Short { inserted, end }
        } else {
            SubmitOutcome::Ok { begin: now, end }
        }
    }
    pub fn csv(&self) -> String {
        use std::fmt::Write;
        let mut s = "batch_id,action,stage,deadline,begin,end\n".to_owned();
        for r in &self.records {
            let action = r
                .action
                .map_or(0, |a| (u64::from(a.generation) << 16) | u64::from(a.slot));
            writeln!(
                s,
                "{},{},{},{},{},{}",
                r.batch_id, action, r.stage, r.deadline.0, r.begin.0, r.end.0
            )
            .unwrap();
        }
        s
    }
}
