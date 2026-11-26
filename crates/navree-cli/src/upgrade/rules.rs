//! Declarative conversion tables mapping `chrono` and legacy ORM timestamp
//! names onto `navree::datetime` / `navree_datetime` and `HasTimestamps`.

/// Which crate path rewritten code names for Navree's date and time types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetCrate {
    /// Apps and plugins depending on the `navree` facade: `navree::datetime`.
    Facade,
    /// Navree's own library crates depending directly on `navree_datetime`.
    Direct,
}

impl TargetCrate {
    /// The Rust module path prefix (`navree::datetime` or `navree_datetime`).
    pub const fn root_path(self) -> &'static str {
        match self {
            TargetCrate::Facade => "navree::datetime",
            TargetCrate::Direct => "navree_datetime",
        }
    }
}

/// A direct 1-to-1 type or enum name mapping from `chrono` to `navree-datetime`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeRule {
    /// Name in `chrono` (`NaiveDate`, `TimeDelta`, `FixedOffset`, ...).
    pub chrono_name: &'static str,
    /// Name in `navree-datetime` (`Date`, `Span`, `Offset`, ...).
    pub target_name: &'static str,
    /// When `true`, an unqualified bare name only matches when explicitly imported
    /// from `chrono` (`Duration` must never match `std::time::Duration`).
    pub require_chrono_import_when_bare: bool,
}

/// Direct type and enum mappings.
pub const TYPE_RULES: &[TypeRule] = &[
    TypeRule {
        chrono_name: "NaiveDate",
        target_name: "Date",
        require_chrono_import_when_bare: false,
    },
    TypeRule {
        chrono_name: "NaiveTime",
        target_name: "Time",
        require_chrono_import_when_bare: false,
    },
    TypeRule {
        chrono_name: "NaiveDateTime",
        target_name: "DateTime",
        require_chrono_import_when_bare: false,
    },
    TypeRule {
        chrono_name: "TimeDelta",
        target_name: "Span",
        require_chrono_import_when_bare: false,
    },
    TypeRule {
        chrono_name: "Duration",
        target_name: "Span",
        require_chrono_import_when_bare: true,
    },
    TypeRule {
        chrono_name: "FixedOffset",
        target_name: "Offset",
        require_chrono_import_when_bare: false,
    },
    TypeRule {
        chrono_name: "SecondsFormat",
        target_name: "Precision",
        require_chrono_import_when_bare: false,
    },
    TypeRule {
        chrono_name: "Weekday",
        target_name: "Weekday",
        require_chrono_import_when_bare: false,
    },
    TypeRule {
        chrono_name: "Month",
        target_name: "Month",
        require_chrono_import_when_bare: false,
    },
];

/// Trait-only or context-replaced `chrono` imports that leave the `use` list
/// once their call sites have been rewritten.
pub const DROPPED_CHRONO_IMPORTS: &[&str] = &[
    "Utc",
    "DateTime",
    "Datelike",
    "Timelike",
    "SubsecRound",
    "DurationRound",
    "TimeZone",
];

/// Names exported at the root of `navree_datetime` that could collide with a
/// foreign binding of the same name in user code (`use uuid::timestamp::Timestamp`,
/// `use bson::DateTime`, ...).
pub const DATETIME_EXPORTS: &[&str] = &[
    "Date",
    "DateTime",
    "Disambiguation",
    "Error",
    "ErrorKind",
    "Formatted",
    "Month",
    "Offset",
    "Precision",
    "Result",
    "Span",
    "Time",
    "Timestamp",
    "Weekday",
    "Zone",
    "Zoned",
    "pattern",
];

/// Associated constant or enum variant mapping `ChronoType::Member` -> `TargetType::Member`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemberPathRule {
    /// Type or enum name in `chrono`.
    pub chrono_type: &'static str,
    /// Constant or variant name in `chrono`.
    pub chrono_member: &'static str,
    /// Replacement type or enum name in `navree-datetime`.
    pub target_type: &'static str,
    /// Replacement constant or variant name in `navree-datetime`.
    pub target_member: &'static str,
    /// Whether bare `chrono_type` requires an explicit `chrono` import in scope.
    pub require_chrono_import_when_bare: bool,
}

/// Associated constants and enum variants.
pub const MEMBER_PATH_RULES: &[MemberPathRule] = &[
    MemberPathRule {
        chrono_type: "DateTime",
        chrono_member: "UNIX_EPOCH",
        target_type: "Timestamp",
        target_member: "UNIX_EPOCH",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "DateTime",
        chrono_member: "MIN_UTC",
        target_type: "Timestamp",
        target_member: "MIN",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "DateTime",
        chrono_member: "MAX_UTC",
        target_type: "Timestamp",
        target_member: "MAX",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "NaiveDate",
        chrono_member: "MIN",
        target_type: "Date",
        target_member: "MIN",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "NaiveDate",
        chrono_member: "MAX",
        target_type: "Date",
        target_member: "MAX",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "NaiveTime",
        chrono_member: "MIN",
        target_type: "Time",
        target_member: "MIDNIGHT",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "TimeDelta",
        chrono_member: "MAX",
        target_type: "Span",
        target_member: "MAX",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "Duration",
        chrono_member: "MAX",
        target_type: "Span",
        target_member: "MAX",
        require_chrono_import_when_bare: true,
    },
    MemberPathRule {
        chrono_type: "TimeDelta",
        chrono_member: "MIN",
        target_type: "Span",
        target_member: "MIN",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "Duration",
        chrono_member: "MIN",
        target_type: "Span",
        target_member: "MIN",
        require_chrono_import_when_bare: true,
    },
    MemberPathRule {
        chrono_type: "SecondsFormat",
        chrono_member: "Secs",
        target_type: "Precision",
        target_member: "Secs",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "SecondsFormat",
        chrono_member: "Millis",
        target_type: "Precision",
        target_member: "Millis",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "SecondsFormat",
        chrono_member: "Micros",
        target_type: "Precision",
        target_member: "Micros",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "SecondsFormat",
        chrono_member: "Nanos",
        target_type: "Precision",
        target_member: "Nanos",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "SecondsFormat",
        chrono_member: "AutoSi",
        target_type: "Precision",
        target_member: "Auto",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "Weekday",
        chrono_member: "Mon",
        target_type: "Weekday",
        target_member: "Monday",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "Weekday",
        chrono_member: "Tue",
        target_type: "Weekday",
        target_member: "Tuesday",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "Weekday",
        chrono_member: "Wed",
        target_type: "Weekday",
        target_member: "Wednesday",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "Weekday",
        chrono_member: "Thu",
        target_type: "Weekday",
        target_member: "Thursday",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "Weekday",
        chrono_member: "Fri",
        target_type: "Weekday",
        target_member: "Friday",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "Weekday",
        chrono_member: "Sat",
        target_type: "Weekday",
        target_member: "Saturday",
        require_chrono_import_when_bare: false,
    },
    MemberPathRule {
        chrono_type: "Weekday",
        chrono_member: "Sun",
        target_type: "Weekday",
        target_member: "Sunday",
        require_chrono_import_when_bare: false,
    },
];

/// How an associated function call `ChronoType::fn_name(args...)` is rewritten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallKind {
    /// Keep arguments in order: `TargetType::target_fn(args...)`.
    Direct,
    /// Zero-argument function becomes an associated constant: `TargetType::TARGET_CONST`.
    ZeroArgConst,
    /// `chrono` returned `Option<T>`, `navree-datetime` returns `Result<T>`:
    /// append `.ok()` (or omit `.ok()` when immediately followed by `.unwrap()` / `.expect(..)`).
    ResultToOption,
    /// `DateTime::from_timestamp(secs, nanos)` -> `Timestamp::from_secs(secs).ok()` when
    /// `nanos` is literal `0`, else `Timestamp::from_secs_nanos(secs, nanos).ok()`.
    FromTimestamp,
    /// `NaiveTime::from_hms_{milli,micro,nano}_opt(h, m, s, sub)` ->
    /// `Time::new(h, m, s).and_then(|t| t.with_nanos(sub * scale)).ok()`.
    SubsecHmsOpt {
        /// Multiplier turning `sub` into nanoseconds (`1_000_000`, `1_000`, `1`).
        nanos_per_unit: u32,
    },
    /// `Type::parse_from_str(text, pattern)` -> `TargetType::parse(pattern, text)` (swaps args).
    SwapParseArgs,
    /// `DateTime::parse_from_rfc3339(s)` / `parse_from_rfc2822(s)` -> `Zoned::parse_rfc*`
    /// (or `Timestamp::parse_rfc*` when chained directly into `.with_timezone(&Utc)` / `.to_utc()`).
    ParseRfc,
}

/// Rule for an associated function call `ChronoType::chrono_fn(...)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallRule {
    /// Receiver type in `chrono`.
    pub chrono_type: &'static str,
    /// Associated function name in `chrono`.
    pub chrono_fn: &'static str,
    /// Expected argument count.
    pub arity: usize,
    /// Replacement type in `navree-datetime`.
    pub target_type: &'static str,
    /// Replacement function or constant name in `navree-datetime`.
    pub target_fn: &'static str,
    /// Conversion shape.
    pub kind: CallKind,
    /// Whether bare `chrono_type` requires an explicit `chrono` import in scope.
    pub require_chrono_import_when_bare: bool,
}

macro_rules! all_call_rules {
    (
        explicit: [ $($explicit:expr),* $(,)? ],
        span: [ $($chrono_fn:literal ($arity:literal) => $target_fn:literal : $kind:ident),+ $(,)? ]
    ) => {
        &[
            $($explicit,)*
            $(
                CallRule {
                    chrono_type: "TimeDelta",
                    chrono_fn: $chrono_fn,
                    arity: $arity,
                    target_type: "Span",
                    target_fn: $target_fn,
                    kind: CallKind::$kind,
                    require_chrono_import_when_bare: false,
                },
                CallRule {
                    chrono_type: "Duration",
                    chrono_fn: $chrono_fn,
                    arity: $arity,
                    target_type: "Span",
                    target_fn: $target_fn,
                    kind: CallKind::$kind,
                    require_chrono_import_when_bare: true,
                },
            )+
        ]
    };
}

/// Associated function and constructor rules.
pub const CALL_RULES: &[CallRule] = all_call_rules! {
    explicit: [
        CallRule {
            chrono_type: "Utc",
            chrono_fn: "now",
            arity: 0,
            target_type: "Timestamp",
            target_fn: "now",
            kind: CallKind::Direct,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveDate",
            chrono_fn: "from_ymd_opt",
            arity: 3,
            target_type: "Date",
            target_fn: "new",
            kind: CallKind::ResultToOption,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveDate",
            chrono_fn: "from_yo_opt",
            arity: 2,
            target_type: "Date",
            target_fn: "from_ordinal",
            kind: CallKind::ResultToOption,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveDate",
            chrono_fn: "from_weekday_of_month_opt",
            arity: 4,
            target_type: "Date",
            target_fn: "nth_weekday_of_month",
            kind: CallKind::Direct,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveTime",
            chrono_fn: "from_hms_opt",
            arity: 3,
            target_type: "Time",
            target_fn: "new",
            kind: CallKind::ResultToOption,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveTime",
            chrono_fn: "from_hms_milli_opt",
            arity: 4,
            target_type: "Time",
            target_fn: "new",
            kind: CallKind::SubsecHmsOpt {
                nanos_per_unit: 1_000_000,
            },
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveTime",
            chrono_fn: "from_hms_micro_opt",
            arity: 4,
            target_type: "Time",
            target_fn: "new",
            kind: CallKind::SubsecHmsOpt {
                nanos_per_unit: 1_000,
            },
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveTime",
            chrono_fn: "from_hms_nano_opt",
            arity: 4,
            target_type: "Time",
            target_fn: "new",
            kind: CallKind::SubsecHmsOpt { nanos_per_unit: 1 },
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveDateTime",
            chrono_fn: "new",
            arity: 2,
            target_type: "DateTime",
            target_fn: "new",
            kind: CallKind::Direct,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveDateTime",
            chrono_fn: "and_utc",
            arity: 1,
            target_type: "DateTime",
            target_fn: "in_utc",
            kind: CallKind::Direct,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "DateTime",
            chrono_fn: "from_timestamp",
            arity: 2,
            target_type: "Timestamp",
            target_fn: "from_secs_nanos",
            kind: CallKind::FromTimestamp,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "DateTime",
            chrono_fn: "from_timestamp_millis",
            arity: 1,
            target_type: "Timestamp",
            target_fn: "from_millis",
            kind: CallKind::ResultToOption,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "DateTime",
            chrono_fn: "from_timestamp_micros",
            arity: 1,
            target_type: "Timestamp",
            target_fn: "from_micros",
            kind: CallKind::ResultToOption,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "FixedOffset",
            chrono_fn: "east_opt",
            arity: 1,
            target_type: "Offset",
            target_fn: "east",
            kind: CallKind::ResultToOption,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "FixedOffset",
            chrono_fn: "west_opt",
            arity: 1,
            target_type: "Offset",
            target_fn: "west",
            kind: CallKind::ResultToOption,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveDate",
            chrono_fn: "parse_from_str",
            arity: 2,
            target_type: "Date",
            target_fn: "parse",
            kind: CallKind::SwapParseArgs,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveTime",
            chrono_fn: "parse_from_str",
            arity: 2,
            target_type: "Time",
            target_fn: "parse",
            kind: CallKind::SwapParseArgs,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "NaiveDateTime",
            chrono_fn: "parse_from_str",
            arity: 2,
            target_type: "DateTime",
            target_fn: "parse",
            kind: CallKind::SwapParseArgs,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "DateTime",
            chrono_fn: "parse_from_str",
            arity: 2,
            target_type: "Zoned",
            target_fn: "parse",
            kind: CallKind::SwapParseArgs,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "DateTime",
            chrono_fn: "parse_from_rfc3339",
            arity: 1,
            target_type: "Zoned",
            target_fn: "parse_rfc3339",
            kind: CallKind::ParseRfc,
            require_chrono_import_when_bare: false,
        },
        CallRule {
            chrono_type: "DateTime",
            chrono_fn: "parse_from_rfc2822",
            arity: 1,
            target_type: "Zoned",
            target_fn: "parse_rfc2822",
            kind: CallKind::ParseRfc,
            require_chrono_import_when_bare: false,
        },
    ],
    span: [
        "seconds"(1) => "seconds": Direct,
        "minutes"(1) => "minutes": Direct,
        "hours"(1) => "hours": Direct,
        "days"(1) => "days": Direct,
        "weeks"(1) => "weeks": Direct,
        "milliseconds"(1) => "millis": Direct,
        "microseconds"(1) => "micros": Direct,
        "nanoseconds"(1) => "nanos": Direct,
        "try_seconds"(1) => "try_seconds": Direct,
        "try_minutes"(1) => "try_minutes": Direct,
        "try_hours"(1) => "try_hours": Direct,
        "try_days"(1) => "try_days": Direct,
        "try_weeks"(1) => "try_weeks": Direct,
        "try_milliseconds"(1) => "try_millis": Direct,
        "try_microseconds"(1) => "try_micros": Direct,
        "try_nanoseconds"(1) => "try_nanos": Direct,
        "from_std"(1) => "try_from": Direct,
        "zero"(0) => "ZERO": ZeroArgConst,
    ]
};

/// Direct 1-to-1 method rename rule (arguments and receiver stay in place).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodRenameRule {
    /// Method name in `chrono`.
    pub chrono_method: &'static str,
    /// Expected number of call arguments (excluding `self`).
    pub arity: usize,
    /// Replacement method name in `navree-datetime`.
    pub target_method: &'static str,
}

/// Method calls whose identifier changes 1-to-1 while receiver and arguments stay as written.
pub const METHOD_RENAME_RULES: &[MethodRenameRule] = &[
    MethodRenameRule {
        chrono_method: "timestamp",
        arity: 0,
        target_method: "as_secs",
    },
    MethodRenameRule {
        chrono_method: "timestamp_millis",
        arity: 0,
        target_method: "as_millis",
    },
    MethodRenameRule {
        chrono_method: "timestamp_micros",
        arity: 0,
        target_method: "as_micros",
    },
    MethodRenameRule {
        chrono_method: "timestamp_subsec_nanos",
        arity: 0,
        target_method: "subsec_nanos",
    },
    MethodRenameRule {
        chrono_method: "num_seconds",
        arity: 0,
        target_method: "as_seconds",
    },
    MethodRenameRule {
        chrono_method: "num_minutes",
        arity: 0,
        target_method: "as_minutes",
    },
    MethodRenameRule {
        chrono_method: "num_hours",
        arity: 0,
        target_method: "as_hours",
    },
    MethodRenameRule {
        chrono_method: "num_days",
        arity: 0,
        target_method: "as_days",
    },
    MethodRenameRule {
        chrono_method: "num_milliseconds",
        arity: 0,
        target_method: "as_millis",
    },
    MethodRenameRule {
        chrono_method: "num_microseconds",
        arity: 0,
        target_method: "as_micros",
    },
    MethodRenameRule {
        chrono_method: "checked_add_signed",
        arity: 1,
        target_method: "checked_add",
    },
    MethodRenameRule {
        chrono_method: "checked_sub_signed",
        arity: 1,
        target_method: "checked_sub",
    },
    MethodRenameRule {
        chrono_method: "signed_duration_since",
        arity: 1,
        target_method: "duration_since",
    },
    MethodRenameRule {
        chrono_method: "to_rfc3339_opts",
        arity: 2,
        target_method: "to_rfc3339_with",
    },
    MethodRenameRule {
        chrono_method: "and_utc",
        arity: 0,
        target_method: "in_utc",
    },
    MethodRenameRule {
        chrono_method: "naive_utc",
        arity: 0,
        target_method: "date_time_utc",
    },
    MethodRenameRule {
        chrono_method: "naive_local",
        arity: 0,
        target_method: "date_time",
    },
    MethodRenameRule {
        chrono_method: "to_utc",
        arity: 0,
        target_method: "timestamp",
    },
    MethodRenameRule {
        chrono_method: "and_time",
        arity: 1,
        target_method: "at",
    },
    MethodRenameRule {
        chrono_method: "succ_opt",
        arity: 0,
        target_method: "tomorrow",
    },
    MethodRenameRule {
        chrono_method: "pred_opt",
        arity: 0,
        target_method: "yesterday",
    },
    MethodRenameRule {
        chrono_method: "local_minus_utc",
        arity: 0,
        target_method: "seconds",
    },
    MethodRenameRule {
        chrono_method: "num_days_from_monday",
        arity: 0,
        target_method: "days_from_monday",
    },
    MethodRenameRule {
        chrono_method: "num_days_from_sunday",
        arity: 0,
        target_method: "days_from_sunday",
    },
];

/// Old and new name of `navree-orm`'s timestamp column trait.
pub const ORM_OLD_TRAIT: &str = "Timestamp";

/// Replacement name for `navree_orm::Timestamp` so it never collides with `datetime::Timestamp`.
pub const ORM_NEW_TRAIT: &str = "HasTimestamps";

/// Note text emitted as `// navree upgrade: <reason>` where human judgment is required.
pub mod notes {
    /// Adding `Span::days` / `Span::weeks` to a `Zoned` value.
    pub const ZONED_SPAN_VS_CALENDAR: &str =
        "confirm exact 24h (Span::days) vs calendar wall-clock step (Zoned::add_days)";

    /// `Days::new`, `Months::new`, `checked_add_days`, `checked_sub_days`, `checked_add_months`.
    pub const CALENDAR_STEP: &str = "choose calendar step (Date::add_days / Date::add_months or Zoned::add_days / Zoned::add_months) vs exact Span";

    /// A non-literal pattern passed to `.format(pat)` or `parse(pat, text)`.
    pub const RUNTIME_PATTERN: &str = "runtime pattern — use Formatted::to_text() or pattern::check() if untrusted (Display panics on invalid specifiers)";

    /// `chrono::format::StrftimeItems` / `.format_with_items(..)`.
    pub const STRFTIME_ITEMS: &str =
        "runtime pattern — use pattern::check() and Formatted::to_text() instead of StrftimeItems";

    /// `LocalResult` or `from_local_datetime` / `offset_from_local_datetime`.
    pub const LOCAL_DISAMBIGUATION: &str =
        "choose Disambiguation (Compatible, Earlier, Later or Reject) for Zone::to_timestamp";

    /// Non-UTC `.with_timezone(zone)`.
    pub const WITH_TIMEZONE: &str = "use Timestamp::in_zone(zone) or Zoned::with_zone(zone) (Zone::fixed(offset) for an Offset)";

    /// Generic `Tz: TimeZone` bound.
    pub const TIMEZONE_BOUND: &str = "TimeZone trait removed; take Zone or Zoned directly";

    /// `DurationRound::duration_trunc` or non-microsecond `trunc_subsecs`.
    pub const SUBSEC_TRUNCATION: &str =
        "truncate with Timestamp::trunc_micros() or Timestamp::as_secs()";
}

/// Looks up a direct type rule by `chrono` name.
pub fn find_type_rule(name: &str) -> Option<&'static TypeRule> {
    TYPE_RULES.iter().find(|rule| rule.chrono_name == name)
}

/// Looks up a constant or enum variant rule by `(chrono_type, chrono_member)`.
pub fn find_member_rule(ty: &str, member: &str) -> Option<&'static MemberPathRule> {
    MEMBER_PATH_RULES
        .iter()
        .find(|rule| rule.chrono_type == ty && rule.chrono_member == member)
}

/// Looks up an associated function rule by `(chrono_type, chrono_fn, arity)`.
pub fn find_call_rule(ty: &str, func: &str, arity: usize) -> Option<&'static CallRule> {
    CALL_RULES
        .iter()
        .find(|rule| rule.chrono_type == ty && rule.chrono_fn == func && rule.arity == arity)
}

/// Looks up a 1-to-1 method rename rule by `(chrono_method, arity)`.
pub fn find_method_rename(method: &str, arity: usize) -> Option<&'static MethodRenameRule> {
    METHOD_RENAME_RULES
        .iter()
        .find(|rule| rule.chrono_method == method && rule.arity == arity)
}
