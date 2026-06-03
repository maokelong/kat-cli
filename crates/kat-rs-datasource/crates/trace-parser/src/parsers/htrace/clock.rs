use super::*;

impl HtraceParser {
    pub(super) fn add_header_clock_snapshot(&mut self, header: &[u8]) -> ParseResult<()> {
        if self.has_clock_snapshot() {
            return Ok(());
        }

        let snapshot = [
            (TS_CLOCK_BOOTTIME, 60usize),
            (TS_CLOCK_REALTIME, 68usize),
            (TS_CLOCK_REALTIME_COARSE, 76usize),
            (TS_CLOCK_MONOTONIC, 84usize),
            (TS_CLOCK_MONOTONIC_COARSE, 92usize),
            (TS_CLOCK_MONOTONIC_RAW, 100usize),
        ]
        .into_iter()
        .filter_map(|(clock_id, offset)| match read_u64_le(header, offset) {
            Ok(0) => None,
            Ok(ts) => Some(Ok((clock_id, ts))),
            Err(err) => Some(Err(err)),
        })
        .collect::<ParseResult<Vec<_>>>()?;
        self.add_clock_snapshot(&snapshot);
        Ok(())
    }

    pub(super) fn has_clock_snapshot(&self) -> bool {
        !self.clock_offsets.is_empty()
    }

    pub(super) fn add_clock_snapshot(&mut self, snapshot: &[(i32, u64)]) {
        if snapshot.len() < 2 {
            return;
        }
        for left in 0..snapshot.len() - 1 {
            for right in left + 1..snapshot.len() {
                let (src_clock, src_ts) = snapshot[left];
                let (dst_clock, dst_ts) = snapshot[right];
                self.add_convert_clock_map(src_clock, dst_clock, src_ts, dst_ts);
                self.add_convert_clock_map(dst_clock, src_clock, dst_ts, src_ts);
            }
        }
    }

    pub(super) fn add_convert_clock_map(
        &mut self,
        src_clock: i32,
        dst_clock: i32,
        src_ts: u64,
        dst_ts: u64,
    ) {
        self.clock_offsets
            .entry((src_clock, dst_clock))
            .or_default()
            .insert(src_ts, dst_ts as i128 - src_ts as i128);
    }

    pub(super) fn to_primary_trace_time(&self, src_clock: i32, src_ts: u64) -> u64 {
        if src_clock == TS_CLOCK_BOOTTIME {
            return src_ts;
        }
        self.convert_clock(src_clock, src_ts, TS_CLOCK_BOOTTIME)
    }

    pub(super) fn convert_clock(&self, src_clock: i32, src_ts: u64, dst_clock: i32) -> u64 {
        if src_clock == dst_clock {
            return src_ts;
        }
        let Some(offsets) = self.clock_offsets.get(&(src_clock, dst_clock)) else {
            return src_ts;
        };
        let Some((_, offset)) = offsets.range(..=src_ts).next_back() else {
            return src_ts;
        };
        let converted = src_ts as i128 + *offset;
        converted.clamp(0, u64::MAX as i128) as u64
    }

    pub(super) fn plugin_realtime_ts(&self, plugin: &ProfilerPluginData) -> Option<i64> {
        plugin_outer_ts(plugin)
            .map(|ts| self.to_primary_trace_time(TS_CLOCK_REALTIME, ts as u64) as i64)
    }
}
