//! `cargo xtask tz-tables DIR [--check]`: compiles an unpacked IANA time zone
//! database release into `crates/navree-datetime/src/zone/tables.rs`.
//!
//! Reads the release identifier (`version` or `NEWS`), the ten standard data
//! files (`africa`, `antarctica`, `asia`, `australasia`, `europe`,
//! `northamerica`, `southamerica`, `etcetera`, `backward`, `factory`) and
//! `zone.tab`, and writes:
//! - every zone's historical offset transitions and its standing rule after the
//!   last change;
//! - every zone and link name of the release;
//! - the legacy alias table that normalizes old names to PostgreSQL-safe
//!   canonical names, and the per-country zone list (primary zone first).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use xtask::time::{civil_from_days, days_from_civil};

/// Output file, relative to the workspace root.
pub const TABLES_PATH: &str = "crates/navree-datetime/src/zone/tables.rs";

/// Standard IANA source files included in a tz database build.
const DATA_FILES: &[&str] = &[
    "africa",
    "antarctica",
    "asia",
    "australasia",
    "europe",
    "northamerica",
    "southamerica",
    "etcetera",
    "backward",
    "factory",
];

/// Names that always normalize to `UTC` in `Zone::parse`.
const UTC_NAMES: &[&str] = &[
    "Etc/GMT",
    "Etc/GMT+0",
    "Etc/GMT-0",
    "Etc/GMT0",
    "Etc/Greenwich",
    "Etc/UCT",
    "Etc/UTC",
    "Etc/Universal",
    "Etc/Zulu",
    "GMT",
    "GMT+0",
    "GMT-0",
    "GMT0",
    "Greenwich",
    "UCT",
    "Universal",
    "Zulu",
];

/// Legacy names whose canonical location target differs from their raw
/// `backward` link target.
const EXTRA_ALIASES: &[(&str, &str)] = &[("America/Coral_Harbour", "America/Atikokan")];

/// Primary zone for countries that span more than one zone and where the
/// primary zone is not the first entry of `zone.tab`.
const PRIMARY_COUNTRY_ZONE: &[(&str, &str)] = &[
    ("AQ", "Antarctica/Casey"),
    ("AR", "America/Argentina/Buenos_Aires"),
    ("AU", "Australia/Sydney"),
    ("BR", "America/Sao_Paulo"),
    ("CA", "America/Toronto"),
    ("CL", "America/Santiago"),
    ("CY", "Asia/Nicosia"),
    ("ES", "Europe/Madrid"),
    ("FM", "Pacific/Pohnpei"),
    ("GL", "America/Nuuk"),
    ("KI", "Pacific/Tarawa"),
    ("MH", "Pacific/Majuro"),
    ("MN", "Asia/Ulaanbaatar"),
    ("MX", "America/Mexico_City"),
    ("PF", "Pacific/Tahiti"),
    ("PG", "Pacific/Port_Moresby"),
    ("PS", "Asia/Hebron"),
    ("PT", "Europe/Lisbon"),
    ("RU", "Europe/Moscow"),
    ("UA", "Europe/Kyiv"),
    ("US", "America/New_York"),
    ("UZ", "Asia/Tashkent"),
];

/// Runs `cargo xtask tz-tables DIR [--check]`.
pub fn cmd(root: &Path, args: &[&str]) -> Result<ExitCode, String> {
    let mut dir: Option<&str> = None;
    let mut check = false;
    for arg in args {
        match *arg {
            "--check" => check = true,
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}")),
            path if dir.is_none() => dir = Some(path),
            extra => return Err(format!("unexpected argument {extra}")),
        }
    }
    let dir =
        dir.ok_or("tz-tables needs an unpacked tzdata directory: cargo xtask tz-tables DIR")?;
    let stale = run(root, Path::new(dir), check)?;
    if !check {
        println!("wrote {TABLES_PATH} from {dir}");
        return Ok(ExitCode::SUCCESS);
    }
    if stale.is_empty() {
        println!("{TABLES_PATH} matches {dir}");
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("- {TABLES_PATH} is stale; write it again with `cargo xtask tz-tables {dir}`");
    Ok(ExitCode::FAILURE)
}

/// Generates `crates/navree-datetime/src/zone/tables.rs` from `tzdata_dir`, or with
/// `check` writes nothing and returns the stale path if it differs.
pub fn run(root: &Path, tzdata_dir: &Path, check: bool) -> Result<Vec<PathBuf>, String> {
    let generated = generate(tzdata_dir)?;
    let target = root.join(TABLES_PATH);
    let on_disk = std::fs::read_to_string(&target)
        .map(|s| s.replace("\r\n", "\n"))
        .ok();
    if on_disk.as_deref() == Some(generated.as_str()) {
        return Ok(Vec::new());
    }
    if check {
        return Ok(vec![target]);
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(&target, generated).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(Vec::new())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TimeRef {
    Wall,
    Standard,
    Utc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum DaySpec {
    Dom(u8),
    LastWeekday(u8),
    OnOrAfter { weekday: u8, dom: u8 },
    OnOrBefore { weekday: u8, dom: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuleTo {
    Year(i32),
    Max,
}

#[derive(Clone, Debug)]
struct RuleRow {
    from_year: i32,
    to_year: RuleTo,
    month: u8,
    on: DaySpec,
    at_secs: i32,
    at_ref: TimeRef,
    save: i32,
    letter: String,
}

#[derive(Clone, Debug)]
enum ZoneRules {
    None,
    Fixed(i32),
    Named(String),
}

#[derive(Clone, Copy, Debug)]
struct UntilSpec {
    year: i32,
    month: u8,
    day: DaySpec,
    time_secs: i32,
    time_ref: TimeRef,
}

#[derive(Clone, Debug)]
struct ZoneEra {
    stdoff: i32,
    rules: ZoneRules,
    format: String,
    until: Option<UntilSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct RawPeriod {
    std_offset: i32,
    dst_offset: i32,
    abbrev: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct RuleEdge {
    month: u8,
    day: DaySpec,
    at_secs: i32,
    at_ref: TimeRef,
    period_idx: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct StandingSpec {
    std_offset: i32,
    first: RuleEdge,
    second: RuleEdge,
}

struct CompiledZone {
    initial_period: u16,
    transitions: Vec<(i64, u16)>,
    standing: Option<u16>,
}

/// Generates the full Rust source of `tables.rs` from `tzdata_dir`.
pub fn generate(tzdata_dir: &Path) -> Result<String, String> {
    let version = read_version(tzdata_dir)?;
    let mut rules_by_name: HashMap<String, Vec<RuleRow>> = HashMap::new();
    let mut raw_zones: BTreeMap<String, Vec<ZoneEra>> = BTreeMap::new();
    let mut raw_links: BTreeMap<String, String> = BTreeMap::new();
    let mut backward_links: HashMap<String, String> = HashMap::new();

    for file_name in DATA_FILES {
        let path = tzdata_dir.join(file_name);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .replace("\r\n", "\n");
        if *file_name == "backward" {
            parse_backward_aliases(&text, &mut backward_links)?;
        }
        parse_data_file(
            *file_name,
            &text,
            &mut rules_by_name,
            &mut raw_zones,
            &mut raw_links,
        )?;
    }

    // Build canonical alias table (legacy name -> PostgreSQL-safe canonical name).
    let aliases = build_aliases(&backward_links);
    let alias_lookup: HashMap<String, String> = aliases
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // Build COUNTRY_ZONES from zone.tab.
    let zone_tab_path = tzdata_dir.join("zone.tab");
    let zone_tab = std::fs::read_to_string(&zone_tab_path)
        .map_err(|e| format!("{}: {e}", zone_tab_path.display()))?
        .replace("\r\n", "\n");
    let countries = build_countries(&zone_tab, &alias_lookup)?;

    // Compile every canonical zone.
    let mut abbrev_pool: BTreeSet<String> = BTreeSet::new();
    let mut temp_zones: BTreeMap<
        String,
        (
            RawPeriod,
            Vec<(i64, RawPeriod)>,
            Option<(i32, (RuleRow, RawPeriod), (RuleRow, RawPeriod))>,
        ),
    > = BTreeMap::new();

    for (zone_name, eras) in &raw_zones {
        let compiled = compile_zone(zone_name, eras, &rules_by_name)?;
        abbrev_pool.insert(compiled.0.abbrev.clone());
        for (_, p) in &compiled.1 {
            abbrev_pool.insert(p.abbrev.clone());
        }
        if let Some((_, (_, ref p1), (_, ref p2))) = compiled.2 {
            abbrev_pool.insert(p1.abbrev.clone());
            abbrev_pool.insert(p2.abbrev.clone());
        }
        temp_zones.insert(zone_name.clone(), compiled);
    }

    let abbrevs: Vec<String> = abbrev_pool.into_iter().collect();
    let abbrev_idx = |s: &str| -> u16 {
        u16::try_from(abbrevs.binary_search_by(|a| a.as_str().cmp(s)).unwrap())
            .expect("abbrev count fits in u16")
    };

    let mut period_pool: BTreeSet<(i32, i32, u16)> = BTreeSet::new();
    for (init, trs, standing) in temp_zones.values() {
        period_pool.insert((init.std_offset, init.dst_offset, abbrev_idx(&init.abbrev)));
        for (_, p) in trs {
            period_pool.insert((p.std_offset, p.dst_offset, abbrev_idx(&p.abbrev)));
        }
        if let Some((_, (_, p1), (_, p2))) = standing {
            period_pool.insert((p1.std_offset, p1.dst_offset, abbrev_idx(&p1.abbrev)));
            period_pool.insert((p2.std_offset, p2.dst_offset, abbrev_idx(&p2.abbrev)));
        }
    }
    let periods: Vec<(i32, i32, u16)> = period_pool.into_iter().collect();
    let period_idx = |p: &RawPeriod| -> u16 {
        let key = (p.std_offset, p.dst_offset, abbrev_idx(&p.abbrev));
        u16::try_from(periods.binary_search(&key).unwrap()).expect("period count fits in u16")
    };

    let mut standing_pool: BTreeSet<StandingSpec> = BTreeSet::new();
    for (_, _, standing) in temp_zones.values() {
        if let Some((stdoff, (r1, p1), (r2, p2))) = standing {
            standing_pool.insert(StandingSpec {
                std_offset: *stdoff,
                first: RuleEdge {
                    month: r1.month,
                    day: r1.on,
                    at_secs: r1.at_secs,
                    at_ref: r1.at_ref,
                    period_idx: period_idx(p1),
                },
                second: RuleEdge {
                    month: r2.month,
                    day: r2.on,
                    at_secs: r2.at_secs,
                    at_ref: r2.at_ref,
                    period_idx: period_idx(p2),
                },
            });
        }
    }
    let standing_rules: Vec<StandingSpec> = standing_pool.into_iter().collect();
    let standing_idx = |spec: &StandingSpec| -> u16 {
        u16::try_from(standing_rules.binary_search(spec).unwrap())
            .expect("standing count fits in u16")
    };

    let mut zone_names: Vec<String> = Vec::with_capacity(temp_zones.len());
    let mut compiled_zones: Vec<CompiledZone> = Vec::with_capacity(temp_zones.len());
    for (name, (init, trs, standing)) in &temp_zones {
        zone_names.push(name.clone());
        let transitions = trs.iter().map(|(at, p)| (*at, period_idx(p))).collect();
        let standing = standing.as_ref().map(|(stdoff, (r1, p1), (r2, p2))| {
            standing_idx(&StandingSpec {
                std_offset: *stdoff,
                first: RuleEdge {
                    month: r1.month,
                    day: r1.on,
                    at_secs: r1.at_secs,
                    at_ref: r1.at_ref,
                    period_idx: period_idx(p1),
                },
                second: RuleEdge {
                    month: r2.month,
                    day: r2.on,
                    at_secs: r2.at_secs,
                    at_ref: r2.at_ref,
                    period_idx: period_idx(p2),
                },
            })
        });
        compiled_zones.push(CompiledZone {
            initial_period: period_idx(init),
            transitions,
            standing,
        });
    }

    let zone_data_idx = |name: &str| -> Result<u16, String> {
        let mut cur = name;
        for _ in 0..16 {
            if let Ok(idx) = zone_names.binary_search_by(|z| z.as_str().cmp(cur)) {
                return u16::try_from(idx).map_err(|_| "too many zones".to_owned());
            }
            if let Some(next) = raw_links.get(cur) {
                cur = next.as_str();
            } else {
                return Err(format!("link target `{cur}` (from `{name}`) is not a Zone"));
            }
        }
        Err(format!("link cycle involving `{name}`"))
    };

    // Every named entry (zones and links), sorted by exact name.
    let mut all_entries: BTreeMap<String, u16> = BTreeMap::new();
    for name in &zone_names {
        all_entries.insert(name.clone(), zone_data_idx(name)?);
    }
    for link_name in raw_links.keys() {
        if all_entries.contains_key(link_name) {
            return Err(format!("duplicate zone/link name `{link_name}`"));
        }
        all_entries.insert(link_name.clone(), zone_data_idx(link_name)?);
    }
    let entries: Vec<(String, u16)> = all_entries.into_iter().collect();

    // Case-insensitive index over `entries`.
    let mut by_lower: Vec<u16> = (0..entries.len())
        .map(|i| u16::try_from(i).expect("entry count fits in u16"))
        .collect();
    by_lower.sort_by(|&a, &b| {
        entries[usize::from(a)]
            .0
            .to_ascii_lowercase()
            .cmp(&entries[usize::from(b)].0.to_ascii_lowercase())
            .then_with(|| entries[usize::from(a)].0.cmp(&entries[usize::from(b)].0))
    });
    for pair in by_lower.windows(2) {
        let left = &entries[usize::from(pair[0])].0;
        let right = &entries[usize::from(pair[1])].0;
        if left.eq_ignore_ascii_case(right) {
            return Err(format!("case-colliding zone names `{left}` and `{right}`"));
        }
    }

    let utc_index = entries
        .binary_search_by(|(name, _)| name.as_str().cmp("UTC"))
        .map(|i| u16::try_from(i).expect("fits in u16"))
        .map_err(|_| "tzdata has no `UTC` zone or link".to_owned())?;

    Ok(render_tables(
        &version,
        utc_index,
        &abbrevs,
        &periods,
        &standing_rules,
        &compiled_zones,
        &entries,
        &by_lower,
        &aliases,
        &countries,
    ))
}

fn read_version(tzdata_dir: &Path) -> Result<String, String> {
    let version_path = tzdata_dir.join("version");
    if let Ok(v) = std::fs::read_to_string(&version_path) {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }
    let news_path = tzdata_dir.join("NEWS");
    if let Ok(news) = std::fs::read_to_string(&news_path) {
        for line in news.lines() {
            if let Some(rest) = line.strip_prefix("Release ")
                && let Some(ver) = rest.split_whitespace().next()
            {
                return Ok(ver.to_owned());
            }
        }
    }
    Err(format!(
        "no `version` or `NEWS` release line found in {}",
        tzdata_dir.display()
    ))
}

fn parse_backward_aliases(text: &str, out: &mut HashMap<String, String>) -> Result<(), String> {
    let mut section = "";
    let mut saw_pre_2013 = false;
    for line in text.lines() {
        for heading in [
            "Pre-1993",
            "Two-part names",
            "Pre-2013 practice",
            "Non-zone.tab",
            "Alternate names",
        ] {
            if line.starts_with(&format!("# {heading}")) {
                section = heading;
                if heading == "Pre-2013 practice" {
                    saw_pre_2013 = true;
                }
            }
        }
        if section == "Pre-2013 practice" {
            continue;
        }
        let (code, comment) = split_comment(line);
        let tokens = tokenize(code);
        if tokens.len() >= 3 && tokens[0] == "Link" {
            let target = comment
                .and_then(|c| c.trim().strip_prefix("= "))
                .and_then(|c| c.split_whitespace().next())
                .unwrap_or(tokens[1].as_str());
            out.insert(tokens[2].clone(), target.to_owned());
        }
    }
    if !saw_pre_2013 || out.is_empty() {
        return Err("backward: missing `# Pre-2013 practice` section or legacy links".to_owned());
    }
    Ok(())
}

fn build_aliases(backward_links: &HashMap<String, String>) -> Vec<(String, String)> {
    let canonical = |name: &str| -> String { resolve_canonical(name, backward_links) };
    let mut names: BTreeSet<String> = backward_links.keys().cloned().collect();
    for utc_name in UTC_NAMES {
        names.insert((*utc_name).to_owned());
    }
    for (alias, _) in EXTRA_ALIASES {
        names.insert((*alias).to_owned());
    }
    let mut aliases: Vec<(String, String)> = names
        .into_iter()
        .filter(|name| name != "UTC")
        .map(|name| {
            let target = canonical(&name);
            (name, target)
        })
        .filter(|(name, target)| name != target)
        .collect();
    aliases.sort_by(|(a, _), (b, _)| {
        a.to_ascii_lowercase()
            .cmp(&b.to_ascii_lowercase())
            .then_with(|| a.cmp(b))
    });
    aliases
}

fn resolve_canonical(name: &str, backward_links: &HashMap<String, String>) -> String {
    if UTC_NAMES.contains(&name) {
        return "UTC".to_owned();
    }
    for (alias, target) in EXTRA_ALIASES {
        if *alias == name {
            return (*target).to_owned();
        }
    }
    let mut seen = BTreeSet::new();
    let mut current = name;
    while let Some(next) = backward_links.get(current) {
        if !seen.insert(current) {
            break;
        }
        current = next.as_str();
        for (alias, target) in EXTRA_ALIASES {
            if *alias == current {
                return (*target).to_owned();
            }
        }
    }
    if UTC_NAMES.contains(&current) {
        "UTC".to_owned()
    } else {
        current.to_owned()
    }
}

fn build_countries(
    zone_tab: &str,
    alias_lookup: &HashMap<String, String>,
) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut by_code: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in zone_tab.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let code = cols
            .next()
            .ok_or_else(|| format!("malformed zone.tab line: {line}"))?
            .trim();
        let _coords = cols
            .next()
            .ok_or_else(|| format!("malformed zone.tab line: {line}"))?;
        let tz_raw = cols
            .next()
            .ok_or_else(|| format!("malformed zone.tab line: {line}"))?
            .trim();
        let tz = alias_lookup
            .get(tz_raw)
            .cloned()
            .unwrap_or_else(|| tz_raw.to_owned());
        let list = by_code.entry(code.to_owned()).or_default();
        if !list.contains(&tz) {
            list.push(tz);
        }
    }

    let primary_map: HashMap<&str, &str> = PRIMARY_COUNTRY_ZONE.iter().copied().collect();
    let mut out = Vec::with_capacity(by_code.len());
    for (code, mut list) in by_code {
        let primary = primary_map
            .get(code.as_str())
            .copied()
            .filter(|p| list.iter().any(|z| z == p))
            .map(str::to_owned)
            .unwrap_or_else(|| list[0].clone());
        list.retain(|z| z != &primary);
        list.sort();
        list.insert(0, primary);
        out.push((code, list));
    }
    Ok(out)
}

fn split_comment(line: &str) -> (&str, Option<&str>) {
    let mut in_quotes = false;
    for (idx, b) in line.bytes().enumerate() {
        match b {
            b'"' => in_quotes = !in_quotes,
            b'#' if !in_quotes => return (&line[..idx], Some(&line[idx + 1..])),
            _ => {}
        }
    }
    (line, None)
}

fn tokenize(code: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut had_quotes = false;
    for ch in code.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            had_quotes = true;
        } else if ch.is_ascii_whitespace() && !in_quotes {
            if !cur.is_empty() || had_quotes {
                tokens.push(std::mem::take(&mut cur));
                had_quotes = false;
            }
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() || had_quotes {
        tokens.push(cur);
    }
    tokens
}

fn parse_data_file(
    file_name: &str,
    text: &str,
    rules_by_name: &mut HashMap<String, Vec<RuleRow>>,
    raw_zones: &mut BTreeMap<String, Vec<ZoneEra>>,
    raw_links: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let mut active_zone: Option<String> = None;
    for (line_no, raw_line) in text.lines().enumerate() {
        let where_at = || format!("{file_name}:{}: ", line_no + 1);
        let (code, _) = split_comment(raw_line);
        let tokens = tokenize(code);
        if tokens.is_empty() {
            continue;
        }
        if let Some(ref zone_name) = active_zone.clone() {
            let era = parse_zone_era(&tokens).map_err(|e| format!("{}{e}", where_at()))?;
            let continues = era.until.is_some();
            raw_zones.get_mut(zone_name).expect("active zone").push(era);
            if !continues {
                active_zone = None;
            }
            continue;
        }
        match tokens[0].as_str() {
            "Rule" => {
                if tokens.len() != 10 {
                    return Err(format!(
                        "{}expected 10 fields in Rule, got {}",
                        where_at(),
                        tokens.len()
                    ));
                }
                let from_year: i32 = match tokens[2].as_str() {
                    "min" | "minimum" => 1800,
                    s => s
                        .parse()
                        .map_err(|_| format!("{}bad FROM year `{s}`", where_at()))?,
                };
                let to_year = match tokens[3].as_str() {
                    "only" => RuleTo::Year(from_year),
                    "max" | "maximum" => RuleTo::Max,
                    s => RuleTo::Year(
                        s.parse()
                            .map_err(|_| format!("{}bad TO year `{s}`", where_at()))?,
                    ),
                };
                let month = parse_month(&tokens[5]).map_err(|e| format!("{}{e}", where_at()))?;
                let on = parse_day_spec(&tokens[6]).map_err(|e| format!("{}{e}", where_at()))?;
                let (at_secs, at_ref) =
                    parse_at(&tokens[7]).map_err(|e| format!("{}{e}", where_at()))?;
                let save = if tokens[8] == "-" {
                    0
                } else {
                    parse_hms(&tokens[8]).map_err(|e| format!("{}{e}", where_at()))?
                };
                let letter = if tokens[9] == "-" {
                    String::new()
                } else {
                    tokens[9].clone()
                };
                rules_by_name
                    .entry(tokens[1].clone())
                    .or_default()
                    .push(RuleRow {
                        from_year,
                        to_year,
                        month,
                        on,
                        at_secs,
                        at_ref,
                        save,
                        letter,
                    });
            }
            "Zone" => {
                if tokens.len() < 5 {
                    return Err(format!("{}Zone line too short", where_at()));
                }
                let name = tokens[1].clone();
                if raw_zones.contains_key(&name) {
                    return Err(format!("{}duplicate Zone `{name}`", where_at()));
                }
                let era = parse_zone_era(&tokens[2..]).map_err(|e| format!("{}{e}", where_at()))?;
                let continues = era.until.is_some();
                raw_zones.insert(name.clone(), vec![era]);
                if continues {
                    active_zone = Some(name);
                }
            }
            "Link" => {
                if tokens.len() != 3 {
                    return Err(format!(
                        "{}expected 3 fields in Link, got {}",
                        where_at(),
                        tokens.len()
                    ));
                }
                raw_links.insert(tokens[2].clone(), tokens[1].clone());
            }
            other => {
                return Err(format!("{}unexpected directive `{other}`", where_at()));
            }
        }
    }
    if let Some(unclosed) = active_zone {
        return Err(format!("{file_name}: unclosed Zone `{unclosed}`"));
    }
    Ok(())
}

fn parse_zone_era(fields: &[String]) -> Result<ZoneEra, String> {
    if fields.len() < 3 {
        return Err("Zone continuation needs STDOFF, RULES and FORMAT".to_owned());
    }
    let stdoff = if fields[0] == "-" {
        0
    } else {
        parse_hms(&fields[0])?
    };
    let rules = if fields[1] == "-" {
        ZoneRules::None
    } else if fields[1].starts_with(|c: char| c.is_ascii_digit() || c == '+' || c == '-')
        && fields[1].bytes().any(|b| b.is_ascii_digit())
        && parse_hms(&fields[1]).is_ok()
    {
        ZoneRules::Fixed(parse_hms(&fields[1])?)
    } else {
        ZoneRules::Named(fields[1].clone())
    };
    let format = fields[2].clone();
    let until = if fields.len() > 3 {
        Some(parse_until(&fields[3..])?)
    } else {
        None
    };
    Ok(ZoneEra {
        stdoff,
        rules,
        format,
        until,
    })
}

fn parse_until(rest: &[String]) -> Result<UntilSpec, String> {
    if rest.len() > 4 {
        return Err(format!("too many UNTIL tokens: {rest:?}"));
    }
    let year: i32 = rest[0]
        .parse()
        .map_err(|_| format!("bad UNTIL year `{}`", rest[0]))?;
    let month = if rest.len() >= 2 {
        parse_month(&rest[1])?
    } else {
        1
    };
    let day = if rest.len() >= 3 {
        parse_day_spec(&rest[2])?
    } else {
        DaySpec::Dom(1)
    };
    let (time_secs, time_ref) = if rest.len() >= 4 {
        parse_at(&rest[3])?
    } else {
        (0, TimeRef::Wall)
    };
    Ok(UntilSpec {
        year,
        month,
        day,
        time_secs,
        time_ref,
    })
}

fn parse_month(s: &str) -> Result<u8, String> {
    let lower = s.to_ascii_lowercase();
    let months = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    for (idx, full) in months.iter().enumerate() {
        if lower.len() >= 3 && full.starts_with(&lower) {
            return Ok((idx + 1) as u8);
        }
    }
    Err(format!("unknown month `{s}`"))
}

fn parse_weekday(s: &str) -> Result<u8, String> {
    let lower = s.to_ascii_lowercase();
    let days = [
        "sunday",
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
    ];
    for (idx, full) in days.iter().enumerate() {
        if lower.len() >= 3 && full.starts_with(&lower) {
            return Ok(idx as u8);
        }
    }
    Err(format!("unknown weekday `{s}`"))
}

fn parse_day_spec(s: &str) -> Result<DaySpec, String> {
    if let Ok(dom) = s.parse::<u8>()
        && (1..=31).contains(&dom)
    {
        return Ok(DaySpec::Dom(dom));
    }
    if let Some(rest) = s.strip_prefix("last") {
        return Ok(DaySpec::LastWeekday(parse_weekday(rest)?));
    }
    if let Some((wd, dom)) = s.split_once(">=") {
        let dom: u8 = dom
            .parse()
            .map_err(|_| format!("bad day-of-month in `{s}`"))?;
        return Ok(DaySpec::OnOrAfter {
            weekday: parse_weekday(wd)?,
            dom,
        });
    }
    if let Some((wd, dom)) = s.split_once("<=") {
        let dom: u8 = dom
            .parse()
            .map_err(|_| format!("bad day-of-month in `{s}`"))?;
        return Ok(DaySpec::OnOrBefore {
            weekday: parse_weekday(wd)?,
            dom,
        });
    }
    Err(format!("unknown day specification `{s}`"))
}

fn parse_at(s: &str) -> Result<(i32, TimeRef), String> {
    if s == "-" {
        return Ok((0, TimeRef::Wall));
    }
    let (body, time_ref) = match s.as_bytes().last().copied() {
        Some(b'w') => (&s[..s.len() - 1], TimeRef::Wall),
        Some(b's') => (&s[..s.len() - 1], TimeRef::Standard),
        Some(b'u' | b'g' | b'z') => (&s[..s.len() - 1], TimeRef::Utc),
        _ => (s, TimeRef::Wall),
    };
    Ok((parse_hms(body)?, time_ref))
}

fn parse_hms(s: &str) -> Result<i32, String> {
    let (sign, rest) = if let Some(r) = s.strip_prefix('-') {
        (-1_i32, r)
    } else if let Some(r) = s.strip_prefix('+') {
        (1_i32, r)
    } else {
        (1_i32, s)
    };
    let mut parts = rest.split(':');
    let hours: u32 = parts
        .next()
        .ok_or_else(|| format!("empty time `{s}`"))?
        .parse()
        .map_err(|_| format!("bad hours in `{s}`"))?;
    let minutes: u32 = match parts.next() {
        Some(m) => m.parse().map_err(|_| format!("bad minutes in `{s}`"))?,
        None => 0,
    };
    let seconds: u32 = match parts.next() {
        Some(sec) => sec.parse().map_err(|_| format!("bad seconds in `{s}`"))?,
        None => 0,
    };
    if parts.next().is_some() || hours > 168 || minutes > 59 || seconds > 59 {
        return Err(format!("malformed time `{s}`"));
    }
    let total = i32::try_from(hours * 3600 + minutes * 60 + seconds)
        .map_err(|_| format!("time overflow in `{s}`"))?;
    Ok(sign * total)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: u8) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => unreachable!(),
    }
}

fn weekday_of_days(days: i64) -> u8 {
    (days + 4).rem_euclid(7) as u8
}

fn resolve_day_days(year: i64, month: u8, spec: DaySpec) -> i64 {
    match spec {
        DaySpec::Dom(d) => days_from_civil(year, u32::from(month), u32::from(d)),
        DaySpec::LastWeekday(target) => {
            let last = days_from_civil(year, u32::from(month), days_in_month(year, month));
            let diff = i64::from((weekday_of_days(last) + 7 - target) % 7);
            last - diff
        }
        DaySpec::OnOrAfter { weekday, dom } => {
            let base = days_from_civil(year, u32::from(month), u32::from(dom));
            let diff = i64::from((weekday + 7 - weekday_of_days(base)) % 7);
            base + diff
        }
        DaySpec::OnOrBefore { weekday, dom } => {
            let base = days_from_civil(year, u32::from(month), u32::from(dom));
            let diff = i64::from((weekday_of_days(base) + 7 - weekday) % 7);
            base - diff
        }
    }
}

fn event_utc(
    year: i64,
    month: u8,
    day: DaySpec,
    time_secs: i32,
    time_ref: TimeRef,
    stdoff: i32,
    current_save: i32,
) -> i64 {
    let local = resolve_day_days(year, month, day) * 86_400 + i64::from(time_secs);
    match time_ref {
        TimeRef::Utc => local,
        TimeRef::Standard => local - i64::from(stdoff),
        TimeRef::Wall => local - i64::from(stdoff + current_save),
    }
}

fn format_abbrev(format: &str, stdoff: i32, save: i32, letter: &str) -> String {
    if let Some((when_std, when_dst)) = format.split_once('/') {
        return if save == 0 {
            when_std.to_owned()
        } else {
            when_dst.to_owned()
        };
    }
    if format.contains("%s") {
        return format.replace("%s", letter);
    }
    if format.contains("%z") {
        let total = stdoff + save;
        let sign = if total < 0 { '-' } else { '+' };
        let abs = total.unsigned_abs();
        let hh = abs / 3600;
        let mm = (abs % 3600) / 60;
        let ss = abs % 60;
        let z = if ss != 0 {
            format!("{sign}{hh:02}{mm:02}{ss:02}")
        } else if mm != 0 {
            format!("{sign}{hh:02}{mm:02}")
        } else {
            format!("{sign}{hh:02}")
        };
        return format.replace("%z", &z);
    }
    format.to_owned()
}

type ZoneCompilation = (
    RawPeriod,
    Vec<(i64, RawPeriod)>,
    Option<(i32, (RuleRow, RawPeriod), (RuleRow, RawPeriod))>,
);

fn compile_zone(
    zone_name: &str,
    eras: &[ZoneEra],
    rules_by_name: &HashMap<String, Vec<RuleRow>>,
) -> Result<ZoneCompilation, String> {
    let mut initial: Option<RawPeriod> = None;
    let mut transitions: Vec<(i64, RawPeriod)> = Vec::new();
    let mut standing: Option<(i32, (RuleRow, RawPeriod), (RuleRow, RawPeriod))> = None;

    let push = |at: Option<i64>,
                period: RawPeriod,
                initial: &mut Option<RawPeriod>,
                transitions: &mut Vec<(i64, RawPeriod)>| {
        let Some(ts) = at else {
            *initial = Some(period);
            return;
        };
        while transitions.last().is_some_and(|(last_ts, _)| *last_ts > ts) {
            transitions.pop();
        }
        if transitions
            .last()
            .is_some_and(|(last_ts, _)| *last_ts == ts)
        {
            transitions.pop();
        }
        let active = transitions
            .last()
            .map(|(_, p)| p)
            .or(initial.as_ref())
            .expect("initial period set");
        if *active != period {
            transitions.push((ts, period));
        }
    };

    let mut era_start: Option<i64> = None;
    let mut prev_stdoff = 0_i32;
    let mut prev_save = 0_i32;
    for era in eras {
        match &era.rules {
            ZoneRules::None | ZoneRules::Fixed(_) => {
                let save = match era.rules {
                    ZoneRules::None => 0,
                    ZoneRules::Fixed(s) => s,
                    ZoneRules::Named(_) => unreachable!(),
                };
                let period = RawPeriod {
                    std_offset: era.stdoff,
                    dst_offset: save,
                    abbrev: format_abbrev(&era.format, era.stdoff, save, ""),
                };
                push(era_start, period, &mut initial, &mut transitions);
                era_start = era.until.map(|u| {
                    event_utc(
                        i64::from(u.year),
                        u.month,
                        u.day,
                        u.time_secs,
                        u.time_ref,
                        era.stdoff,
                        save,
                    )
                });
                prev_stdoff = era.stdoff;
                prev_save = save;
            }
            ZoneRules::Named(rule_name) => {
                let rule_set = rules_by_name
                    .get(rule_name)
                    .ok_or_else(|| format!("{zone_name}: unknown Rule `{rule_name}`"))?;
                let mut current_save = 0_i32;
                let mut current_letter = rule_set
                    .iter()
                    .filter(|r| r.save == 0)
                    .min_by_key(|r| (r.from_year, r.month))
                    .map_or_else(String::new, |r| r.letter.clone());

                let min_year = rule_set.iter().map(|r| r.from_year).min().unwrap_or(1900);
                let max_rule_bound = rule_set
                    .iter()
                    .map(|r| match r.to_year {
                        RuleTo::Year(y) => y,
                        RuleTo::Max => r.from_year,
                    })
                    .max()
                    .unwrap_or(min_year);
                let start_year = era_start
                    .map(|s| civil_from_days(s.div_euclid(86_400)).0 as i32)
                    .unwrap_or(min_year);
                let end_year = match era.until {
                    Some(u) => u.year + 1,
                    None => max_rule_bound.max(start_year) + 2,
                };

                // Collect all (year, rule_index) candidates across min_year..=end_year.
                let mut pool: Vec<(i32, usize)> = Vec::new();
                for y in min_year..=end_year {
                    for (idx, r) in rule_set.iter().enumerate() {
                        let applies = y >= r.from_year
                            && match r.to_year {
                                RuleTo::Year(to) => y <= to,
                                RuleTo::Max => true,
                            };
                        if applies {
                            pool.push((y, idx));
                        }
                    }
                }

                let mut entered_era = false;
                let mut until_cutoff: Option<i64> = None;

                while !pool.is_empty() {
                    // Pick the candidate whose UTC instant under `current_save` comes earliest.
                    let mut best_pos = 0;
                    let mut best_utc = i64::MAX;
                    for (pos, &(y, idx)) in pool.iter().enumerate() {
                        let r = &rule_set[idx];
                        let t = event_utc(
                            i64::from(y),
                            r.month,
                            r.on,
                            r.at_secs,
                            r.at_ref,
                            era.stdoff,
                            current_save,
                        );
                        if t < best_utc {
                            best_utc = t;
                            best_pos = pos;
                        }
                    }

                    let (pool_year, idx) = pool[best_pos];
                    let r = &rule_set[idx];

                    if let Some(start_ts) = era_start {
                        let prev_utc = event_utc(
                            i64::from(pool_year),
                            r.month,
                            r.on,
                            r.at_secs,
                            r.at_ref,
                            prev_stdoff,
                            prev_save,
                        );
                        if best_utc <= start_ts || (!entered_era && prev_utc <= start_ts) {
                            pool.swap_remove(best_pos);
                            current_save = r.save;
                            current_letter.clone_from(&r.letter);
                            continue;
                        }
                    }

                    if let Some(u) = era.until {
                        let u_utc = event_utc(
                            i64::from(u.year),
                            u.month,
                            u.day,
                            u.time_secs,
                            u.time_ref,
                            era.stdoff,
                            current_save,
                        );
                        if best_utc >= u_utc {
                            until_cutoff = Some(u_utc);
                            break;
                        }
                    }

                    pool.swap_remove(best_pos);

                    if let Some(start_ts) = era_start {
                        if !entered_era {
                            entered_era = true;
                            let start_period = RawPeriod {
                                std_offset: era.stdoff,
                                dst_offset: current_save,
                                abbrev: format_abbrev(
                                    &era.format,
                                    era.stdoff,
                                    current_save,
                                    &current_letter,
                                ),
                            };
                            push(Some(start_ts), start_period, &mut initial, &mut transitions);
                        }
                    } else if !entered_era {
                        entered_era = true;
                        let start_period = RawPeriod {
                            std_offset: era.stdoff,
                            dst_offset: current_save,
                            abbrev: format_abbrev(
                                &era.format,
                                era.stdoff,
                                current_save,
                                &current_letter,
                            ),
                        };
                        push(None, start_period, &mut initial, &mut transitions);
                    }

                    current_save = r.save;
                    current_letter.clone_from(&r.letter);
                    let next_period = RawPeriod {
                        std_offset: era.stdoff,
                        dst_offset: current_save,
                        abbrev: format_abbrev(
                            &era.format,
                            era.stdoff,
                            current_save,
                            &current_letter,
                        ),
                    };
                    push(Some(best_utc), next_period, &mut initial, &mut transitions);
                }

                if !entered_era {
                    let start_period = RawPeriod {
                        std_offset: era.stdoff,
                        dst_offset: current_save,
                        abbrev: format_abbrev(
                            &era.format,
                            era.stdoff,
                            current_save,
                            &current_letter,
                        ),
                    };
                    push(era_start, start_period, &mut initial, &mut transitions);
                }

                if let Some(u) = era.until {
                    era_start = Some(until_cutoff.unwrap_or_else(|| {
                        event_utc(
                            i64::from(u.year),
                            u.month,
                            u.day,
                            u.time_secs,
                            u.time_ref,
                            era.stdoff,
                            current_save,
                        )
                    }));
                    prev_stdoff = era.stdoff;
                    prev_save = current_save;
                } else {
                    era_start = None;
                    let max_rules: Vec<&RuleRow> = rule_set
                        .iter()
                        .filter(|r| r.to_year == RuleTo::Max)
                        .collect();
                    match max_rules.as_slice() {
                        [] => {}
                        [a, b] => {
                            // Order the two annual rules by when they fire in `end_year + 1`.
                            let test_y = i64::from(end_year + 1);
                            let ta = event_utc(
                                test_y,
                                a.month,
                                a.on,
                                a.at_secs,
                                a.at_ref,
                                era.stdoff,
                                current_save,
                            );
                            let tb = event_utc(
                                test_y,
                                b.month,
                                b.on,
                                b.at_secs,
                                b.at_ref,
                                era.stdoff,
                                current_save,
                            );
                            let (r1, r2) = if ta <= tb { (*a, *b) } else { (*b, *a) };
                            let p1 = RawPeriod {
                                std_offset: era.stdoff,
                                dst_offset: r1.save,
                                abbrev: format_abbrev(&era.format, era.stdoff, r1.save, &r1.letter),
                            };
                            let p2 = RawPeriod {
                                std_offset: era.stdoff,
                                dst_offset: r2.save,
                                abbrev: format_abbrev(&era.format, era.stdoff, r2.save, &r2.letter),
                            };
                            if p1 != p2 {
                                standing = Some((era.stdoff, (r1.clone(), p1), (r2.clone(), p2)));
                            }
                        }
                        other => {
                            return Err(format!(
                                "{zone_name}: expected 0 or 2 max rules in `{rule_name}`, found {}",
                                other.len()
                            ));
                        }
                    }
                }
            }
        }
    }

    let initial = initial.ok_or_else(|| format!("{zone_name}: empty zone"))?;
    if let Some((_, first_p)) = transitions.first() {
        assert_ne!(
            initial, *first_p,
            "{zone_name}: redundant initial transition"
        );
    }
    assert!(
        transitions
            .windows(2)
            .all(|w| w[0].0 < w[1].0 && w[0].1 != w[1].1),
        "{zone_name}: transitions must be strictly increasing with distinct adjacent periods"
    );

    Ok((initial, transitions, standing))
}

#[allow(clippy::too_many_arguments)]
fn render_tables(
    version: &str,
    utc_index: u16,
    abbrevs: &[String],
    periods: &[(i32, i32, u16)],
    standing_rules: &[StandingSpec],
    zones: &[CompiledZone],
    entries: &[(String, u16)],
    by_lower: &[u16],
    aliases: &[(String, String)],
    countries: &[(String, Vec<String>)],
) -> String {
    let mut out = String::with_capacity(220_000);
    writeln!(
        out,
        "//! Generated by `cargo xtask tz-tables`; do not edit by hand."
    )
    .unwrap();
    writeln!(out, "//!").unwrap();
    writeln!(
        out,
        "//! Time zone transitions, standing rules, links, legacy aliases and country"
    )
    .unwrap();
    writeln!(
        out,
        "//! zones compiled from IANA tzdata {version} (public domain)."
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "/// The IANA time zone database release compiled into these tables."
    )
    .unwrap();
    writeln!(out, "pub(crate) const TZDATA_VERSION: &str = {version:?};").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "/// Index of `UTC` in [`ENTRIES`].").unwrap();
    writeln!(out, "pub(crate) const UTC_INDEX: u16 = {utc_index};").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "/// How a standing rule's transition clock time is interpreted."
    )
    .unwrap();
    writeln!(out, "#[allow(dead_code)]").unwrap();
    writeln!(out, "#[derive(Clone, Copy)]").unwrap();
    writeln!(out, "pub(crate) enum TimeRef {{").unwrap();
    writeln!(out, "    Wall,").unwrap();
    writeln!(out, "    Standard,").unwrap();
    writeln!(out, "    Utc,").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "/// Which calendar day of a month a standing rule fires on."
    )
    .unwrap();
    writeln!(out, "#[allow(dead_code)]").unwrap();
    writeln!(out, "#[derive(Clone, Copy)]").unwrap();
    writeln!(out, "pub(crate) enum DaySpec {{").unwrap();
    writeln!(out, "    Dom(u8),").unwrap();
    writeln!(out, "    LastWeekday(u8),").unwrap();
    writeln!(out, "    OnOrAfter {{ weekday: u8, dom: u8 }},").unwrap();
    writeln!(out, "    OnOrBefore {{ weekday: u8, dom: u8 }},").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "/// One annual transition edge of a standing rule.").unwrap();
    writeln!(out, "#[derive(Clone, Copy)]").unwrap();
    writeln!(out, "pub(crate) struct RuleEdge {{").unwrap();
    writeln!(out, "    pub(crate) month: u8,").unwrap();
    writeln!(out, "    pub(crate) day: DaySpec,").unwrap();
    writeln!(out, "    pub(crate) at_secs: i32,").unwrap();
    writeln!(out, "    pub(crate) at_ref: TimeRef,").unwrap();
    writeln!(out, "    pub(crate) period: u16,").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "/// Two annual transitions repeating after a zone's last historical change."
    )
    .unwrap();
    writeln!(out, "#[derive(Clone, Copy)]").unwrap();
    writeln!(out, "pub(crate) struct StandingRule {{").unwrap();
    writeln!(out, "    pub(crate) std_offset: i32,").unwrap();
    writeln!(out, "    pub(crate) first: RuleEdge,").unwrap();
    writeln!(out, "    pub(crate) second: RuleEdge,").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "/// Offset and abbreviation of a zone over a span of instants."
    )
    .unwrap();
    writeln!(out, "#[derive(Clone, Copy)]").unwrap();
    writeln!(out, "pub(crate) struct PeriodRecord {{").unwrap();
    writeln!(out, "    pub(crate) std_offset: i32,").unwrap();
    writeln!(out, "    pub(crate) dst_offset: i32,").unwrap();
    writeln!(out, "    pub(crate) abbrev: u16,").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "/// Compiled transition history and optional standing rule of a canonical zone."
    )
    .unwrap();
    writeln!(out, "#[derive(Clone, Copy)]").unwrap();
    writeln!(out, "pub(crate) struct ZoneRecord {{").unwrap();
    writeln!(out, "    pub(crate) initial: u16,").unwrap();
    writeln!(out, "    pub(crate) start: u32,").unwrap();
    writeln!(out, "    pub(crate) end: u32,").unwrap();
    writeln!(out, "    pub(crate) standing: Option<u16>,").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // ABBREVS
    writeln!(
        out,
        "/// Distinct zone abbreviations referenced by [`PERIODS`]."
    )
    .unwrap();
    writeln!(out, "#[rustfmt::skip]").unwrap();
    writeln!(out, "pub(crate) const ABBREVS: &[&str] = &[").unwrap();
    for chunk in abbrevs.chunks(8) {
        write!(out, "   ").unwrap();
        for s in chunk {
            write!(out, " {s:?},").unwrap();
        }
        writeln!(out).unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();

    // PERIODS
    writeln!(
        out,
        "/// Distinct `(std_offset, dst_offset, abbrev)` records."
    )
    .unwrap();
    writeln!(out, "#[rustfmt::skip]").unwrap();
    writeln!(out, "pub(crate) const PERIODS: &[PeriodRecord] = &[").unwrap();
    for (std_off, dst_off, abbrev) in periods {
        writeln!(
            out,
            "    PeriodRecord {{ std_offset: {std_off}, dst_offset: {dst_off}, abbrev: {abbrev} }},"
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();

    // STANDING_RULES
    writeln!(out, "/// Standing annual rules referenced by [`ZONES`].").unwrap();
    writeln!(out, "#[rustfmt::skip]").unwrap();
    writeln!(out, "pub(crate) const STANDING_RULES: &[StandingRule] = &[").unwrap();
    for sr in standing_rules {
        writeln!(
            out,
            "    StandingRule {{ std_offset: {}, first: {}, second: {} }},",
            sr.std_offset,
            fmt_edge(&sr.first),
            fmt_edge(&sr.second),
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();

    // Flat transitions slice + ZONES slice
    let mut flat_transitions: Vec<(i64, u16)> = Vec::new();
    let mut slices: Vec<(usize, usize)> = Vec::with_capacity(zones.len());
    for z in zones {
        let start = flat_transitions.len();
        flat_transitions.extend_from_slice(&z.transitions);
        slices.push((start, flat_transitions.len()));
    }

    writeln!(
        out,
        "/// All zone transitions `(utc_seconds, period_index)`, indexed by [`ZONES`]."
    )
    .unwrap();
    writeln!(out, "#[rustfmt::skip]").unwrap();
    writeln!(out, "pub(crate) const TRANSITIONS: &[(i64, u16)] = &[").unwrap();
    for chunk in flat_transitions.chunks(6) {
        write!(out, "   ").unwrap();
        for (at, p) in chunk {
            write!(out, " ({at}, {p}),").unwrap();
        }
        writeln!(out).unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "/// Compiled data of each canonical zone.").unwrap();
    writeln!(out, "#[rustfmt::skip]").unwrap();
    writeln!(out, "pub(crate) const ZONES: &[ZoneRecord] = &[").unwrap();
    for (z, &(start, end)) in zones.iter().zip(slices.iter()) {
        let standing = match z.standing {
            Some(idx) => format!("Some({idx})"),
            None => "None".to_owned(),
        };
        writeln!(
            out,
            "    ZoneRecord {{ initial: {}, start: {start}, end: {end}, standing: {standing} }},",
            z.initial_period
        )
        .unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();

    // ENTRIES
    writeln!(
        out,
        "/// Every zone and link name of the tz database release, sorted by exact name."
    )
    .unwrap();
    writeln!(out, "#[rustfmt::skip]").unwrap();
    writeln!(out, "pub(crate) const ENTRIES: &[(&str, u16)] = &[").unwrap();
    for (name, z_idx) in entries {
        writeln!(out, "    ({name:?}, {z_idx}),").unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();

    // BY_LOWER
    writeln!(
        out,
        "/// Indices into [`ENTRIES`], sorted by ASCII lower-cased name."
    )
    .unwrap();
    writeln!(out, "#[rustfmt::skip]").unwrap();
    writeln!(out, "pub(crate) const BY_LOWER: &[u16] = &[").unwrap();
    for chunk in by_lower.chunks(16) {
        write!(out, "   ").unwrap();
        for idx in chunk {
            write!(out, " {idx},").unwrap();
        }
        writeln!(out).unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();

    // ALIASES
    writeln!(
        out,
        "/// Legacy and alternate names, with the current name each stands for,"
    )
    .unwrap();
    writeln!(out, "/// sorted by the lower-cased legacy name.").unwrap();
    writeln!(out, "#[rustfmt::skip]").unwrap();
    writeln!(out, "pub(crate) const ALIASES: &[(&str, &str)] = &[").unwrap();
    for (name, target) in aliases {
        writeln!(out, "    ({name:?}, {target:?}),").unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(out).unwrap();

    // COUNTRY_ZONES
    writeln!(
        out,
        "/// ISO 3166-1 alpha-2 codes with their zones, primary zone first, sorted"
    )
    .unwrap();
    writeln!(out, "/// by code.").unwrap();
    writeln!(out, "#[rustfmt::skip]").unwrap();
    writeln!(
        out,
        "pub(crate) const COUNTRY_ZONES: &[(&str, &[&str])] = &["
    )
    .unwrap();
    for (code, list) in countries {
        let items: Vec<String> = list.iter().map(|z| format!("{z:?}")).collect();
        writeln!(out, "    ({code:?}, &[{}]),", items.join(", ")).unwrap();
    }
    writeln!(out, "];").unwrap();

    out
}

fn fmt_edge(edge: &RuleEdge) -> String {
    let day = match edge.day {
        DaySpec::Dom(d) => format!("DaySpec::Dom({d})"),
        DaySpec::LastWeekday(w) => format!("DaySpec::LastWeekday({w})"),
        DaySpec::OnOrAfter { weekday, dom } => {
            format!("DaySpec::OnOrAfter {{ weekday: {weekday}, dom: {dom} }}")
        }
        DaySpec::OnOrBefore { weekday, dom } => {
            format!("DaySpec::OnOrBefore {{ weekday: {weekday}, dom: {dom} }}")
        }
    };
    let time_ref = match edge.at_ref {
        TimeRef::Wall => "TimeRef::Wall",
        TimeRef::Standard => "TimeRef::Standard",
        TimeRef::Utc => "TimeRef::Utc",
    };
    format!(
        "RuleEdge {{ month: {}, day: {day}, at_secs: {}, at_ref: {time_ref}, period: {} }}",
        edge.month, edge.at_secs, edge.period_idx
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_sample_release(dir: &Path) {
        std::fs::write(dir.join("version"), "2025b\n").unwrap();
        std::fs::write(
            dir.join("africa"),
            "Zone\tAfrica/Abidjan\t-0:16:08 -\tLMT\t1912\n\t\t\t0:00\t-\tGMT\n",
        )
        .unwrap();
        std::fs::write(dir.join("antarctica"), "").unwrap();
        std::fs::write(
            dir.join("asia"),
            "Zone\tAsia/Kolkata\t5:53:28\t-\tHMT\t1880\n\t\t\t5:21:10\t-\tMMT\t1906\n\t\t\t5:30\t-\tIST\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("australasia"),
            "Rule\tLH\t2008\tmax\t-\tApr\tSun>=1\t2:00\t0\t-\n\
             Rule\tLH\t2008\tmax\t-\tOct\tSun>=1\t2:00\t0:30\t-\n\
             Zone\tAustralia/Lord_Howe\t10:30\tLH\t%z\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("europe"),
            "Rule\tEU\t1981\tmax\t-\tMar\tlastSun\t1:00u\t1:00\tS\n\
             Rule\tEU\t1996\tmax\t-\tOct\tlastSun\t1:00u\t0\t-\n\
             Zone\tEurope/Berlin\t1:00\tEU\tCE%sT\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("northamerica"),
            "Rule\tUS\t2007\tmax\t-\tMar\tSun>=8\t2:00\t1:00\tD\n\
             Rule\tUS\t2007\tmax\t-\tNov\tSun>=1\t2:00\t0\tS\n\
             Zone\tAmerica/New_York\t-5:00\tUS\tE%sT\n",
        )
        .unwrap();
        std::fs::write(dir.join("southamerica"), "").unwrap();
        std::fs::write(
            dir.join("etcetera"),
            "Zone\tEtc/UTC\t0\t-\tUTC\nZone\tEtc/GMT\t0\t-\tGMT\nLink\tEtc/GMT\tGMT\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("backward"),
            "# Pre-1993 naming conventions\n\
             Link\tEtc/UTC\tUTC\n\
             Link\tAmerica/New_York\tUS/Eastern\n\
             # Pre-2013 practice\n\
             Link\tEurope/Berlin\tEurope/Oslo\n\
             # Alternate names\n\
             Link\tAsia/Kolkata\tAsia/Calcutta\n",
        )
        .unwrap();
        std::fs::write(dir.join("factory"), "Zone\tFactory\t0\t-\t-00\n").unwrap();
        std::fs::write(
            dir.join("zone.tab"),
            "# country\tcoordinates\tTZ\n\
             DE\t+5230+01322\tEurope/Berlin\n\
             IN\t+2232+08822\tAsia/Kolkata\n\
             NO\t+5955+01045\tEurope/Oslo\n\
             US\t+404251-0740023\tAmerica/New_York\n",
        )
        .unwrap();
    }

    #[test]
    fn the_generator_writes_the_same_tables_from_the_same_release() {
        let release = tempfile::tempdir().unwrap();
        write_sample_release(release.path());

        let first = generate(release.path()).unwrap();
        let second = generate(release.path()).unwrap();
        assert_eq!(first, second);
        assert!(first.contains(r#"TZDATA_VERSION: &str = "2025b""#));
        assert!(first.contains(r#"("Asia/Calcutta", "Asia/Kolkata")"#));
        assert!(first.contains(r#"("US/Eastern", "America/New_York")"#));
        assert!(!first.contains(r#"("Europe/Oslo", "Europe/Berlin")"#));
        assert!(first.contains(r#"("IN", &["Asia/Kolkata"])"#));
        assert!(first.contains(r#"("NO", &["Europe/Oslo"])"#));

        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            run(root.path(), release.path(), true).unwrap(),
            [root.path().join(TABLES_PATH)]
        );
        assert!(run(root.path(), release.path(), false).unwrap().is_empty());
        assert!(run(root.path(), release.path(), true).unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(root.path().join(TABLES_PATH)).unwrap(),
            first
        );

        assert!(parse_hms("--1:00").is_err());
        assert!(parse_hms("-0:25:21").unwrap() == -1521);
        let mut empty_backward = HashMap::new();
        assert!(parse_backward_aliases("Link\tEtc/UTC\tUTC\n", &mut empty_backward).is_err());

        let eras = vec![
            parse_zone_era(&[
                "-12:00".into(),
                "-".into(),
                "-12".into(),
                "2000".into(),
                "Jan".into(),
                "2".into(),
            ])
            .unwrap(),
            parse_zone_era(&[
                "12:00".into(),
                "-".into(),
                "+12".into(),
                "2000".into(),
                "Jan".into(),
                "2".into(),
                "12:00".into(),
            ])
            .unwrap(),
            parse_zone_era(&["0:00".into(), "-".into(), "UTC".into()]).unwrap(),
        ];
        let (_, transitions, _) = compile_zone("Pacific/Test", &eras, &HashMap::new()).unwrap();
        assert!(transitions.windows(2).all(|w| w[0].0 < w[1].0));
    }
}
