use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveTime, TimeZone, Utc, Weekday};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BusyInterval {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy)]
pub struct WorkingHours {
    pub start_min: u32,
    pub end_min: u32,
}

impl Default for WorkingHours {
    fn default() -> Self {
        Self { start_min: 9 * 60, end_min: 18 * 60 }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Slot {
    pub start: String,
    pub end: String,
}

/// Sort by start and coalesce overlapping/adjacent intervals.
pub fn merge_intervals(mut ivs: Vec<BusyInterval>) -> Vec<BusyInterval> {
    ivs.sort_by_key(|i| i.start);
    let mut out: Vec<BusyInterval> = Vec::new();
    for iv in ivs {
        match out.last_mut() {
            Some(last) if iv.start <= last.end => {
                if iv.end > last.end {
                    last.end = iv.end;
                }
            }
            _ => out.push(iv),
        }
    }
    out
}

fn is_free(t0: DateTime<Utc>, t1: DateTime<Utc>, busy: &[BusyInterval]) -> bool {
    !busy.iter().any(|b| b.start < t1 && t0 < b.end)
}

/// Earliest-first free slots of `duration_min` on a `granularity_min` grid,
/// within [start_min, end_min) local working hours, skipping weekends.
pub fn suggest_slots(
    busy: Vec<BusyInterval>,
    range_start: DateTime<Utc>,
    range_end: DateTime<Utc>,
    tz: FixedOffset,
    wh: WorkingHours,
    duration_min: i64,
    granularity_min: i64,
    max: usize,
) -> Vec<Slot> {
    let merged = merge_intervals(busy);
    let dur = Duration::minutes(duration_min);
    let step = Duration::minutes(granularity_min.max(1));
    let mut out = Vec::new();

    let mut day = range_start.with_timezone(&tz).date_naive();
    let last_day = range_end.with_timezone(&tz).date_naive();
    while day <= last_day {
        if !matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            let start_t = NaiveTime::from_num_seconds_from_midnight_opt(wh.start_min * 60, 0);
            let end_t = NaiveTime::from_num_seconds_from_midnight_opt(wh.end_min * 60, 0);
            if let (Some(st), Some(et)) = (start_t, end_t) {
                let ws = tz.from_local_datetime(&day.and_time(st)).single();
                let we = tz.from_local_datetime(&day.and_time(et)).single();
                if let (Some(ws), Some(we)) = (ws, we) {
                    let ws_utc = ws.with_timezone(&Utc);
                    let ws_u = ws_utc.max(range_start);
                    let we_u = we.with_timezone(&Utc).min(range_end);
                    let mut t = ws_u;
                    if t > ws_utc {
                        let elapsed = (t - ws_utc).num_minutes();
                        let rem = elapsed % granularity_min.max(1);
                        if rem != 0 {
                            t += Duration::minutes(granularity_min.max(1) - rem);
                        }
                    }
                    while t + dur <= we_u {
                        if is_free(t, t + dur, &merged) {
                            out.push(Slot {
                                start: t.with_timezone(&tz).to_rfc3339(),
                                end: (t + dur).with_timezone(&tz).to_rfc3339(),
                            });
                            if out.len() >= max {
                                return out;
                            }
                        }
                        t += step;
                    }
                }
            }
        }
        match day.succ_opt() {
            Some(next) => day = next,
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, TimeZone, Utc};

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn merges_overlapping_and_adjacent() {
        let merged = merge_intervals(vec![
            BusyInterval { start: utc(2026, 7, 1, 9, 0), end: utc(2026, 7, 1, 10, 0) },
            BusyInterval { start: utc(2026, 7, 1, 9, 30), end: utc(2026, 7, 1, 11, 0) },
            BusyInterval { start: utc(2026, 7, 1, 11, 0), end: utc(2026, 7, 1, 12, 0) },
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].end, utc(2026, 7, 1, 12, 0));
    }

    #[test]
    fn suggests_earliest_free_slots_in_working_hours() {
        // Kyiv summer = +03:00. Wednesday 2026-07-01, no busy at all.
        let tz = FixedOffset::east_opt(3 * 3600).unwrap();
        let slots = suggest_slots(
            vec![],
            utc(2026, 7, 1, 0, 0),
            utc(2026, 7, 1, 23, 0),
            tz,
            WorkingHours::default(),
            30,
            30,
            3,
        );
        assert_eq!(slots.len(), 3);
        // 09:00 local == 06:00 UTC
        assert!(slots[0].start.starts_with("2026-07-01T09:00:00"));
        assert!(slots[1].start.starts_with("2026-07-01T09:30:00"));
    }

    #[test]
    fn skips_busy_blocks() {
        let tz = FixedOffset::east_opt(3 * 3600).unwrap();
        // Busy 09:00-10:00 local == 06:00-07:00 UTC
        let busy = vec![BusyInterval { start: utc(2026, 7, 1, 6, 0), end: utc(2026, 7, 1, 7, 0) }];
        let slots = suggest_slots(
            busy, utc(2026, 7, 1, 0, 0), utc(2026, 7, 1, 23, 0),
            tz, WorkingHours::default(), 60, 30, 1,
        );
        // First 60-min free slot must start at/after 10:00 local
        assert!(slots[0].start.starts_with("2026-07-01T10:00:00"), "got {}", slots[0].start);
    }

    #[test]
    fn skips_weekends() {
        let tz = FixedOffset::east_opt(3 * 3600).unwrap();
        // 2026-07-04 is Saturday, 2026-07-05 Sunday, 2026-07-06 Monday.
        let slots = suggest_slots(
            vec![], utc(2026, 7, 4, 0, 0), utc(2026, 7, 6, 23, 0),
            tz, WorkingHours::default(), 30, 30, 1,
        );
        assert!(slots[0].start.starts_with("2026-07-06"), "got {}", slots[0].start);
    }

    #[test]
    fn no_slot_when_fully_busy() {
        let tz = FixedOffset::east_opt(3 * 3600).unwrap();
        // Busy the entire working day 06:00-15:00 UTC (09:00-18:00 local).
        let busy = vec![BusyInterval { start: utc(2026, 7, 1, 6, 0), end: utc(2026, 7, 1, 15, 0) }];
        let slots = suggest_slots(
            busy, utc(2026, 7, 1, 0, 0), utc(2026, 7, 1, 23, 0),
            tz, WorkingHours::default(), 30, 30, 5,
        );
        assert!(slots.is_empty());
    }

    #[test]
    fn snaps_first_slot_to_granularity_grid_when_range_starts_off_grid() {
        let tz = FixedOffset::east_opt(3 * 3600).unwrap();
        // 06:17 UTC == 09:17 local; working window opens 09:00 local (06:00 UTC).
        let slots = suggest_slots(
            vec![],
            utc(2026, 7, 1, 6, 17),
            utc(2026, 7, 1, 23, 0),
            tz,
            WorkingHours::default(),
            30,
            30,
            1,
        );
        assert!(slots[0].start.starts_with("2026-07-01T09:30:00"), "got {}", slots[0].start);
    }

    #[test]
    fn spans_midweek_across_weekend_into_next_week() {
        let tz = FixedOffset::east_opt(3 * 3600).unwrap();
        // Fri 2026-07-03 -> Sat/Sun skipped -> Mon 2026-07-06. Busy all of Friday's working hours.
        let busy = vec![BusyInterval { start: utc(2026, 7, 3, 6, 0), end: utc(2026, 7, 3, 15, 0) }];
        let slots = suggest_slots(
            busy,
            utc(2026, 7, 3, 0, 0),
            utc(2026, 7, 6, 23, 0),
            tz,
            WorkingHours::default(),
            30,
            30,
            1,
        );
        // Friday fully busy, Sat/Sun skipped -> first free slot is Monday 09:00 local.
        assert!(slots[0].start.starts_with("2026-07-06T09:00:00"), "got {}", slots[0].start);
    }
}
