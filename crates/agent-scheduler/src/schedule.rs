//! Parsing a schedule spec, and computing the next fire time.
//!
//! Everything here is a **pure function of `(spec, now_ms)`** — no wall clock is
//! read. That is what makes the behaviour testable at all: a scheduler that
//! consults the system clock internally can only be tested by sleeping, which is
//! slow and flaky under `nix flake check`.
//!
//! The cron support is a deliberate subset (see `docs/components/scheduler.md`):
//! 5 fields, with `*`, `N`, `a-b`, `a,b`, and `*/step`. No `@macros`, no
//! day-of-week names, none of the `L`/`W`/`#` extensions. A spec using them is
//! **rejected**, not silently mis-scheduled.

use agent_core::{Error, Result, Schedule};

/// A minute is the finest cron granularity, and the tick loop advances by it.
const MINUTE_MS: u64 = 60_000;

/// Parse an operator-written spec into a typed [`Schedule`].
///
/// Accepted forms:
/// * `every 30s` / `every 15m` / `every 2h` / `every 1d`
/// * `cron: 0 * * * *` (or a bare 5-field expression)
/// * `once: <epoch-ms>` / `in 45m`
pub fn parse(spec: &str, now_ms: u64) -> Result<Schedule> {
    let s = spec.trim();
    if s.is_empty() {
        return Err(Error::Scheduler("schedule spec is empty".into()));
    }
    let lower = s.to_ascii_lowercase();

    if let Some(rest) = lower.strip_prefix("every ") {
        let secs = parse_duration_secs(rest.trim())?;
        if secs == 0 {
            return Err(Error::Scheduler(
                "interval must be greater than zero".into(),
            ));
        }
        return Ok(Schedule::Interval { secs });
    }
    if let Some(rest) = lower.strip_prefix("in ") {
        let secs = parse_duration_secs(rest.trim())?;
        return Ok(Schedule::Once {
            at_ms: now_ms.saturating_add(secs.saturating_mul(1_000)),
        });
    }
    if let Some(rest) = lower.strip_prefix("once:") {
        let at_ms: u64 = rest
            .trim()
            .parse()
            .map_err(|_| Error::Scheduler("`once:` needs an epoch-millisecond value".into()))?;
        return Ok(Schedule::Once { at_ms });
    }
    let cron_body = lower.strip_prefix("cron:").unwrap_or(&lower).trim();
    // Validate now so a bad expression fails at scheduling time, not silently at
    // fire time (or worse, never).
    let fields = CronFields::parse(cron_body)?;
    let _ = fields;
    Ok(Schedule::Cron {
        expr: cron_body.to_string(),
    })
}

/// `30s` / `15m` / `2h` / `1d` → seconds.
fn parse_duration_secs(s: &str) -> Result<u64> {
    let s = s.trim();
    let (num, mult) = match s.chars().last() {
        Some('s') => (&s[..s.len() - 1], 1u64),
        Some('m') => (&s[..s.len() - 1], 60),
        Some('h') => (&s[..s.len() - 1], 3_600),
        Some('d') => (&s[..s.len() - 1], 86_400),
        // A bare number is seconds.
        Some(c) if c.is_ascii_digit() => (s, 1),
        _ => return Err(Error::Scheduler(format!("unrecognised duration `{s}`"))),
    };
    let n: u64 = num
        .trim()
        .parse()
        .map_err(|_| Error::Scheduler(format!("unrecognised duration `{s}`")))?;
    n.checked_mul(mult)
        .ok_or_else(|| Error::Scheduler(format!("duration `{s}` overflows")))
}

/// The next fire **strictly after** `after_ms`, or `None` for a spent one-shot.
///
/// Strictly-after is load-bearing, not a detail. The scheduler re-arms a job by
/// calling this with the instant it just fired: with "at or after" semantics a
/// cron expression matching that exact minute returns the same instant, so the
/// job is immediately due again and spins in a hot loop — and a one-shot would
/// re-arm at its own instant and fire forever. Both were caught by tests.
///
/// The cost is that a job scheduled at an instant it already matches waits for
/// the next occurrence (up to a minute for cron). That is the safe direction.
pub fn next_fire(schedule: &Schedule, after_ms: u64) -> Option<u64> {
    match schedule {
        Schedule::Once { at_ms } => (*at_ms > after_ms).then_some(*at_ms),
        Schedule::Interval { secs } => {
            let step = secs.max(&1).saturating_mul(1_000);
            Some(after_ms.saturating_add(step))
        }
        Schedule::Cron { expr } => {
            let fields = CronFields::parse(expr).ok()?;
            // Scan forward a minute at a time. Bounded to ~1 year: a valid cron
            // expression that matches nothing within a year (e.g. Feb 30) must
            // terminate rather than spin.
            // Start at the next minute boundary strictly after `after_ms`.
            let start = (after_ms / MINUTE_MS + 1) * MINUTE_MS;
            for i in 0..(366 * 24 * 60) {
                let t = start.saturating_add(i * MINUTE_MS);
                if fields.matches(t) {
                    return Some(t);
                }
            }
            None
        }
    }
}

/// A parsed 5-field cron expression: minute hour day-of-month month day-of-week.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CronFields {
    minute: Field,
    hour: Field,
    dom: Field,
    month: Field,
    dow: Field,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Field {
    Any,
    /// Explicit allowed values.
    Set(Vec<u32>),
}

impl Field {
    fn parse(spec: &str, min: u32, max: u32) -> Result<Self> {
        if spec == "*" {
            return Ok(Field::Any);
        }
        let mut out = Vec::new();
        for part in spec.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err(Error::Scheduler("empty cron field element".into()));
            }
            // `*/step` or `a-b/step`
            let (range, step) = match part.split_once('/') {
                Some((r, s)) => (
                    r,
                    s.parse::<u32>()
                        .map_err(|_| Error::Scheduler(format!("bad cron step `{s}`")))?,
                ),
                None => (part, 1),
            };
            if step == 0 {
                return Err(Error::Scheduler(
                    "cron step must be greater than zero".into(),
                ));
            }
            let (lo, hi) = if range == "*" {
                (min, max)
            } else if let Some((a, b)) = range.split_once('-') {
                (parse_num(a, min, max)?, parse_num(b, min, max)?)
            } else {
                let n = parse_num(range, min, max)?;
                (n, n)
            };
            if lo > hi {
                return Err(Error::Scheduler(format!("inverted cron range `{range}`")));
            }
            let mut v = lo;
            while v <= hi {
                out.push(v);
                v += step;
            }
        }
        out.sort_unstable();
        out.dedup();
        if out.is_empty() {
            return Err(Error::Scheduler("cron field matches nothing".into()));
        }
        Ok(Field::Set(out))
    }

    fn contains(&self, v: u32) -> bool {
        match self {
            Field::Any => true,
            Field::Set(s) => s.contains(&v),
        }
    }
}

fn parse_num(s: &str, min: u32, max: u32) -> Result<u32> {
    let n: u32 = s
        .trim()
        .parse()
        .map_err(|_| Error::Scheduler(format!("bad cron value `{s}`")))?;
    if n < min || n > max {
        return Err(Error::Scheduler(format!(
            "cron value `{n}` out of range {min}..={max}"
        )));
    }
    Ok(n)
}

impl CronFields {
    fn parse(expr: &str) -> Result<Self> {
        let f: Vec<&str> = expr.split_whitespace().collect();
        if f.len() != 5 {
            return Err(Error::Scheduler(format!(
                "cron expression needs 5 fields (minute hour day month weekday), got {}",
                f.len()
            )));
        }
        Ok(CronFields {
            minute: Field::parse(f[0], 0, 59)?,
            hour: Field::parse(f[1], 0, 23)?,
            dom: Field::parse(f[2], 1, 31)?,
            month: Field::parse(f[3], 1, 12)?,
            // 0 and 7 both mean Sunday, as in every cron.
            dow: Field::parse(f[4], 0, 7)?,
        })
    }

    fn matches(&self, epoch_ms: u64) -> bool {
        let t = civil_from_epoch_ms(epoch_ms);
        let dow_ok = self.dow.contains(t.weekday)
            || (t.weekday == 0 && self.dow.contains(7))
            || (t.weekday == 7 && self.dow.contains(0));
        self.minute.contains(t.minute)
            && self.hour.contains(t.hour)
            && self.month.contains(t.month)
            // Standard cron: when BOTH day-of-month and day-of-week are
            // restricted, either matching is enough.
            && match (&self.dom, &self.dow) {
                (Field::Any, _) | (_, Field::Any) => self.dom.contains(t.day) && dow_ok,
                _ => self.dom.contains(t.day) || dow_ok,
            }
    }
}

struct Civil {
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    /// 0 = Sunday.
    weekday: u32,
}

/// Epoch ms → UTC civil fields. Hand-rolled (days-from-civil, Howard Hinnant's
/// algorithm) so the crate stays dependency-free, and UTC-anchored so a job's
/// fire time does not shift with the server's local zone.
fn civil_from_epoch_ms(ms: u64) -> Civil {
    let secs = ms / 1_000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;

    // 1970-01-01 was a Thursday (4).
    let weekday = (((days % 7) + 4 + 7) % 7) as u32;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let _ = era;

    Civil {
        month,
        day,
        hour: (rem / 3_600) as u32,
        minute: ((rem % 3_600) / 60) as u32,
        weekday,
    }
}

/// Bench hook: parse + next-fire over a cron expression (the CPU path).
#[doc(hidden)]
pub fn bench_next_fire(expr: &str, from_ms: u64) -> Option<u64> {
    let sched = parse(expr, from_ms).ok()?;
    next_fire(&sched, from_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// 2024-01-01T00:00:00Z, a Monday.
    const MON: u64 = 1_704_067_200_000;

    #[rstest]
    #[case::positive_interval_seconds("every 30s", Schedule::Interval { secs: 30 })]
    #[case::positive_interval_minutes("every 15m", Schedule::Interval { secs: 900 })]
    #[case::positive_interval_hours("every 2h", Schedule::Interval { secs: 7200 })]
    #[case::positive_interval_days("every 1d", Schedule::Interval { secs: 86400 })]
    #[case::positive_bare_number_is_seconds("every 45", Schedule::Interval { secs: 45 })]
    #[case::positive_cron_prefixed("cron: 0 * * * *", Schedule::Cron { expr: "0 * * * *".into() })]
    #[case::positive_cron_bare("*/5 * * * *", Schedule::Cron { expr: "*/5 * * * *".into() })]
    fn parse_cases(#[case] spec: &str, #[case] want: Schedule) {
        assert_eq!(parse(spec, MON).unwrap(), want);
    }

    #[test]
    fn positive_relative_once_is_anchored_to_the_injected_clock() {
        let s = parse("in 45m", MON).unwrap();
        assert_eq!(
            s,
            Schedule::Once {
                at_ms: MON + 45 * 60 * 1000
            }
        );
    }

    /// A bad spec must be rejected at scheduling time — not silently
    /// mis-scheduled, and never a panic.
    #[rstest]
    #[case::negative_empty("")]
    #[case::negative_whitespace("   ")]
    #[case::negative_bad_duration("every banana")]
    #[case::negative_zero_interval("every 0s")]
    #[case::negative_cron_too_few_fields("* * *")]
    #[case::negative_cron_too_many_fields("* * * * * *")]
    #[case::negative_cron_out_of_range("99 * * * *")]
    #[case::negative_cron_inverted_range("30-10 * * * *")]
    #[case::negative_cron_zero_step("*/0 * * * *")]
    #[case::negative_cron_garbage("nonsense here at all!")]
    #[case::adversarial_unsupported_macro("@hourly")]
    #[case::adversarial_dow_name("0 0 * * MON")]
    #[case::adversarial_huge_number("every 99999999999999999999d")]
    fn negative_bad_specs_are_rejected(#[case] spec: &str) {
        assert!(parse(spec, MON).is_err(), "`{spec}` must be rejected");
    }

    /// Next-fire is a pure function of (schedule, now) — no clock is read.
    #[test]
    fn positive_interval_next_fire_is_relative_to_the_argument() {
        let s = Schedule::Interval { secs: 60 };
        assert_eq!(next_fire(&s, MON), Some(MON + 60_000));
        assert_eq!(next_fire(&s, MON + 5_000), Some(MON + 65_000));
    }

    /// A one-shot fires once and then never again — the `None` is what stops the
    /// job being re-armed forever.
    #[test]
    fn boundary_once_is_spent_after_its_instant() {
        let s = Schedule::Once { at_ms: MON };
        assert_eq!(next_fire(&s, MON - 1), Some(MON));
        // Strictly after: re-arming AT the fire instant yields None, which is
        // what stops a one-shot firing forever.
        assert_eq!(next_fire(&s, MON), None, "spent once it has fired");
        assert_eq!(next_fire(&s, MON + 1), None, "spent");
    }

    #[rstest]
    // Every hour on the hour: from 00:00 the next is 01:00.
    #[case::positive_hourly("0 * * * *", MON, MON + 3_600_000)]
    // Every 5 minutes.
    #[case::positive_step("*/5 * * * *", MON + 60_000, MON + 5 * 60_000)]
    // A specific time of day: 06:30.
    #[case::positive_daily_at("30 6 * * *", MON, MON + (6 * 60 + 30) * 60_000)]
    // Strictly after: an expression matching `from` exactly yields the NEXT one,
    // which is what keeps a re-armed job from spinning.
    #[case::boundary_match_at_from_yields_next("0 * * * *", MON, MON + 3_600_000)]
    // A list.
    #[case::positive_list("0,30 * * * *", MON + 60_000, MON + 30 * 60_000)]
    fn cron_next_fire_cases(#[case] expr: &str, #[case] from: u64, #[case] want: u64) {
        let s = parse(expr, from).unwrap();
        assert_eq!(next_fire(&s, from), Some(want), "expr: {expr}");
    }

    /// Day-of-week must be computed correctly, not approximated. 2024-01-01 was
    /// a Monday, so a Monday-only job fires that day and a Sunday-only job does
    /// not fire until the 7th.
    #[test]
    fn positive_day_of_week_is_correct() {
        // Search from just before midnight so the boundary itself is the answer.
        let just_before = MON - 1;
        let mon = parse("0 0 * * 1", MON).unwrap();
        assert_eq!(
            next_fire(&mon, just_before),
            Some(MON),
            "2024-01-01 was a Monday"
        );
        let sun = parse("0 0 * * 0", MON).unwrap();
        assert_eq!(
            next_fire(&sun, just_before),
            Some(MON + 6 * 86_400_000),
            "the next Sunday is the 7th"
        );
    }

    /// Both 0 and 7 mean Sunday, as in every other cron.
    #[test]
    fn corner_zero_and_seven_both_mean_sunday() {
        let a = next_fire(&parse("0 0 * * 0", MON).unwrap(), MON - 1);
        let b = next_fire(&parse("0 0 * * 7", MON).unwrap(), MON - 1);
        assert_eq!(a, b);
    }

    /// A cron that can never match must terminate rather than spin forever.
    #[test]
    fn adversarial_impossible_cron_terminates() {
        // Feb 30 never exists.
        let s = parse("0 0 30 2 *", MON).unwrap();
        assert_eq!(next_fire(&s, MON), None, "must give up, not hang");
    }

    /// A huge interval must not overflow into the past.
    #[test]
    fn adversarial_huge_interval_saturates() {
        let s = Schedule::Interval { secs: u64::MAX };
        let got = next_fire(&s, MON).unwrap();
        assert!(got >= MON, "next fire went backwards");
    }

    #[rstest]
    #[case::boundary_epoch(0)]
    #[case::boundary_far_future(4_102_444_800_000)]
    #[case::corner_leap_day(1_709_164_800_000)] // 2024-02-29
    fn adversarial_civil_conversion_never_panics(#[case] ms: u64) {
        let c = civil_from_epoch_ms(ms);
        assert!((1..=12).contains(&c.month), "month {}", c.month);
        assert!((1..=31).contains(&c.day), "day {}", c.day);
        assert!(c.hour < 24 && c.minute < 60);
        assert!(c.weekday < 7);
    }

    /// 2024-02-29 was a Thursday — a leap-day check on the civil conversion.
    #[test]
    fn corner_leap_day_is_a_thursday() {
        let c = civil_from_epoch_ms(1_709_164_800_000);
        assert_eq!((c.month, c.day), (2, 29));
        assert_eq!(c.weekday, 4, "2024-02-29 was a Thursday");
    }
}
