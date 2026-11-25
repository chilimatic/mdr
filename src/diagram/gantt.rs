//! Mermaid `gantt`: one bar per task on a day scale that fits the window width.

use std::collections::HashMap;

use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

use super::chart::{pad_right, trunc};
use super::{Lines, Palette};

const MAX_TASKS: usize = 500;
const MAX_SPAN_DAYS: i64 = 20 * 366;
const MIN_BAR: usize = 8;

pub struct Task {
    name: String,
    start: i64,
    end: i64,
    done: bool,
    active: bool,
    crit: bool,
    milestone: bool,
}

#[derive(Default)]
pub struct Gantt {
    title: Option<String>,
    /// Section name and its tasks. Tasks before the first `section` have no name.
    sections: Vec<(Option<String>, Vec<Task>)>,
}

// ---------------------------------------------------------------------------
// Calendar math: days since 1970-01-01.
// ---------------------------------------------------------------------------

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `YYYY-MM-DD`, checked against the calendar.
fn parse_date(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let field = |r: std::ops::Range<usize>| -> Option<i64> { if s.get(r.clone())?.bytes().all(|c| c.is_ascii_digit()) { s[r].parse().ok() } else { None } };
    let (y, m, d) = (field(0..4)?, field(5..7)?, field(8..10)?);
    let days = days_from_civil(y, m, d);
    (civil_from_days(days) == (y, m, d)).then_some(days)
}

fn fmt_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `3d`, `2w`, `12h`: whole days, hours and smaller units rounded up.
fn parse_duration(s: &str) -> Option<i64> {
    let unit = s.chars().last()?;
    let num = &s[..s.len() - unit.len_utf8()];
    if num.is_empty() || num.len() > 6 || !num.bytes().all(|c| c.is_ascii_digit() || c == b'.') {
        return None;
    }
    let n: f64 = num.parse().ok()?;
    let per_day = match unit {
        'd' => 1.0,
        'w' => 1.0 / 7.0,
        'M' => 1.0 / 30.0,
        'y' => 1.0 / 365.0,
        'h' => 24.0,
        'm' => 1440.0,
        's' => 86_400.0,
        _ => return None,
    };
    Some((n / per_day).ceil() as i64)
}

// ---------------------------------------------------------------------------

const IGNORED: [&str; 11] =
    ["axisFormat", "excludes", "includes", "todayMarker", "tickInterval", "weekday", "weekend", "click", "accTitle", "accDescr", "inclusiveEndDates"];

pub fn parse(body: &[&str]) -> Result<Gantt, String> {
    let mut g = Gantt::default();
    let mut ids: HashMap<String, i64> = HashMap::new();
    let mut prev_end: Option<i64> = None;
    let mut count = 0;
    for &line in body {
        let first = line.split(|c: char| c.is_whitespace() || c == ':').next().unwrap_or("");
        let rest = line[first.len()..].trim();
        match first {
            "title" => g.title = Some(rest.to_string()),
            "dateFormat" if rest != "YYYY-MM-DD" => return Err(format!("gantt dateFormat \"{rest}\" is not supported, use YYYY-MM-DD")),
            "dateFormat" => {}
            "section" => g.sections.push((Some(rest.to_string()), Vec::new())),
            _ if IGNORED.contains(&first) => {}
            _ => {
                let Some((name, spec)) = line.split_once(':') else { continue };
                let task = parse_task(name.trim(), spec, &mut ids, prev_end)?;
                count += 1;
                if count > MAX_TASKS {
                    return Err(format!("gantt chart has more than {MAX_TASKS} tasks"));
                }
                prev_end = Some(task.end);
                if g.sections.is_empty() {
                    g.sections.push((None, Vec::new()));
                }
                g.sections.last_mut().expect("section exists").1.push(task);
            }
        }
    }
    let tasks = || g.sections.iter().flat_map(|s| &s.1);
    let (Some(lo), Some(hi)) = (tasks().map(|t| t.start).min(), tasks().map(|t| t.end.max(t.start)).max()) else {
        return Err("gantt chart has no tasks".into());
    };
    if hi - lo > MAX_SPAN_DAYS {
        return Err("gantt chart spans more than 20 years".into());
    }
    Ok(g)
}

fn parse_task(name: &str, spec: &str, ids: &mut HashMap<String, i64>, prev_end: Option<i64>) -> Result<Task, String> {
    let fields: Vec<&str> = spec.split(',').map(str::trim).filter(|f| !f.is_empty()).collect();
    let tags = fields.iter().take_while(|f| matches!(**f, "done" | "active" | "crit" | "milestone")).count();
    let has = |t: &str| fields[..tags].contains(&t);
    let rest = &fields[tags..];
    let (id, start, end) = match *rest {
        [] => (None, None, None),
        [e] if e.starts_with("after ") => (None, Some(e), None),
        [e] => (None, None, Some(e)),
        [s, e] => (None, Some(s), Some(e)),
        [i, s, e] => (Some(i), Some(s), Some(e)),
        _ => return Err(format!("cannot parse gantt task \"{name}\"")),
    };
    let bad = |what: &str, v: &str| format!("gantt task \"{name}\": {what} \"{v}\"");
    let start = match start {
        Some(s) => match s.strip_prefix("after ") {
            Some(after) => {
                let mut latest = None;
                for a in after.split_whitespace() {
                    let e = *ids.get(a).ok_or_else(|| bad("unknown task id", a))?;
                    latest = latest.max(Some(e));
                }
                latest.ok_or_else(|| bad("bad start", s))?
            }
            None => parse_date(s).ok_or_else(|| bad("bad start date", s))?,
        },
        None => prev_end.ok_or_else(|| format!("gantt task \"{name}\" needs a start date"))?,
    };
    let end = match end {
        None => start + 1,
        Some(e) => match parse_date(e) {
            Some(d) => d,
            None => start + parse_duration(e).ok_or_else(|| bad("bad end or duration", e))?,
        },
    };
    let end = end.max(start);
    if let Some(i) = id {
        ids.insert(i.to_string(), end);
    }
    Ok(Task { name: name.to_string(), start, end, done: has("done"), active: has("active"), crit: has("crit"), milestone: has("milestone") })
}

// ---------------------------------------------------------------------------

pub fn render(g: &Gantt, width: usize, pal: &Palette) -> Result<Lines, String> {
    let tasks = || g.sections.iter().flat_map(|s| &s.1);
    let lo = tasks().map(|t| t.start).min().unwrap_or(0);
    let hi = tasks().map(|t| t.end.max(t.start)).max().unwrap_or(lo);
    let span = (hi - lo).max(1) as usize;
    let widest = tasks().map(|t| t.name.width()).max().unwrap_or(0);
    // Row: two spaces, label, space, axis bar, then the bar area.
    let label_w = widest.min(width.saturating_sub(4) * 2 / 5).max(4);
    let bar_w = width.saturating_sub(label_w + 4);
    if bar_w < MIN_BAR {
        return Err(format!("gantt chart needs at least {} columns", label_w + 4 + MIN_BAR));
    }
    let col = |day: i64| ((day - lo) as usize * bar_w / span).min(bar_w - 1);

    let mut out: Lines = Vec::new();
    if let Some(t) = g.title.as_ref().filter(|t| !t.is_empty()) {
        out.push(vec![(trunc(t, width), pal.title)]);
    }
    for (name, list) in &g.sections {
        if let Some(n) = name {
            out.push(vec![(trunc(n, width), pal.border)]);
        }
        for t in list {
            let x0 = col(t.start);
            let (x1, glyph) = if t.milestone {
                (x0 + 1, "◆")
            } else {
                let end = ((t.end - lo) as usize * bar_w).div_ceil(span).min(bar_w);
                (end.max(x0 + 1), if t.done { "░" } else if t.active { "▓" } else { "█" })
            };
            let style: Style = if t.crit {
                pal.series(3)
            } else if t.done {
                pal.dim
            } else if t.active {
                pal.text
            } else {
                pal.series(0)
            };
            out.push(vec![
                (format!("  {} ", pad_right(&trunc(&t.name, label_w), label_w)), pal.text),
                ("│".to_string(), pal.edge),
                (" ".repeat(x0), Style::default()),
                (glyph.repeat(x1 - x0), style),
            ]);
        }
    }

    // Axis: a rule with ticks, then the first, middle and last date.
    let mut ticks: Vec<char> = vec!['─'; bar_w];
    let mut dates: Vec<char> = vec![' '; bar_w];
    let mut put = |text: String, at: usize, tick: usize| {
        for (k, c) in text.chars().enumerate() {
            if let Some(slot) = dates.get_mut(at + k) {
                *slot = c;
            }
        }
        ticks[tick] = '┴';
    };
    put(fmt_date(lo), 0, 0);
    if bar_w >= 21 {
        put(fmt_date(hi), bar_w - 10, bar_w - 1);
    }
    if bar_w >= 34 {
        put(fmt_date(lo + (hi - lo) / 2), (bar_w - 10) / 2, bar_w / 2);
    }
    let pad = " ".repeat(label_w + 3);
    out.push(vec![(format!("{pad}└{}", ticks.iter().collect::<String>()), pal.edge)]);
    out.push(vec![(format!("{pad} {}", dates.iter().collect::<String>().trim_end()), pal.label)]);
    Ok(out)
}
