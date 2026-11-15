//! Drivers a module registers under a name: [`DriverName`] and the set of
//! names the process knows, [`DriverContext`] (what a driver's factory
//! gets), [`DriverSlot`] (a selector a driver kind reads) and
//! [`RegisteredDriver`] (a driver a module registered), and the machinery
//! every driver kind of Navree shares.
//!
//! A driver name is a lower-case letter followed by up to 63 lower-case
//! letters, digits or `_` (`two_tier`, `sanduk`). Names live for the whole
//! process, each stored once, so a [`DriverName`] is a `Copy` handle that a
//! selector such as `CacheStoreKind` can hold without losing `Copy`.
//!
//! Only two things add a name to the set: a registration (and the built-in
//! names of a driver kind), with [`DriverName::register`], and a value read
//! from the app's configuration, with [`DriverName::new`]. A name read from
//! anywhere else (a request, a stored row, a job payload) goes through
//! [`DriverName::lookup`], which never adds one, so made-up names cannot
//! grow the set. A driver kind that reads a name from outside looks it up
//! among the names registered for that kind only, so the name of another
//! kind's driver is as unknown to it as a made-up one.
//!
//! The boot and `config:check` build their messages with the same
//! functions, so they say the same thing about a name that does not
//! resolve.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use navree_kernel::__private::app_in_chain;
use navree_kernel::{AppContext, BoxError, BoxFuture, Contributed, ModuleBuilder, Resolver};
use serde::de::{self, DeserializeOwned};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value as Json, json};

use crate::check::CheckReport;
use crate::de::{Sink, deserialize_tracked};
use crate::error::redact_values;
use crate::redact::{scrub, secret_values};
use crate::schema::SectionSchema;
use crate::usage::covers;
use crate::value::{Kind, Value, from_held_json};
use crate::{
    Config, ConfigError, ConfigRegistry, ConfigSources, ModuleBuilderConfigExt, SectionRegistration,
};

/// The most distinct driver names one process holds.
const LIMIT: usize = 1024;

/// The names of the process.
///
/// Names are never removed: registered and built-in names are in the set
/// before any configuration is read, so nothing read later can push them
/// out.
static NAMES: OnceLock<Mutex<Names>> = OnceLock::new();

/// The process's set of names, locked. A lock poisoned by a panic elsewhere
/// is taken over: the set is never left half-changed.
fn names() -> MutexGuard<'static, Names> {
    NAMES
        .get_or_init(|| Mutex::new(Names::with_limit(LIMIT)))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// A set of driver names, each stored once, holding at most `limit`.
struct Names {
    set: BTreeSet<&'static str>,
    /// The keys of the slots each name was registered for: a name is known
    /// for a kind only when a module registered a driver of that kind under
    /// it.
    registered: BTreeMap<&'static str, BTreeSet<&'static str>>,
    limit: usize,
}

impl Names {
    fn with_limit(limit: usize) -> Names {
        Names {
            set: BTreeSet::new(),
            registered: BTreeMap::new(),
            limit,
        }
    }

    /// Stores `name` itself, as [`register`](Names::register) does, as a
    /// name registered for the slot whose key is `slot`.
    fn register_in(
        &mut self,
        slot: &'static str,
        name: &'static str,
    ) -> Result<&'static str, DriverNameError> {
        let stored = self.register(name)?;
        self.registered.entry(stored).or_default().insert(slot);
        Ok(stored)
    }

    /// The stored name equal to `name` when it was registered for the slot
    /// whose key is `slot`; never stores one.
    fn lookup_in(&self, slot: &str, name: &str) -> Option<&'static str> {
        let (stored, slots) = self.registered.get_key_value(name)?;
        slots.contains(slot).then_some(*stored)
    }

    /// Stores `name` itself (no copy); the stored name when it is there.
    fn register(&mut self, name: &'static str) -> Result<&'static str, DriverNameError> {
        if !DriverName::is_valid(name) {
            return Err(DriverNameError::Invalid);
        }
        if let Some(stored) = self.set.get(name).copied() {
            return Ok(stored);
        }
        if self.set.len() >= self.limit {
            return Err(DriverNameError::TooMany);
        }
        self.set.insert(name);
        Ok(name)
    }

    /// The stored name equal to `name`, or a copy of it, stored once.
    fn intern(&mut self, name: &str) -> Result<&'static str, DriverNameError> {
        if !DriverName::is_valid(name) {
            return Err(DriverNameError::Invalid);
        }
        if let Some(stored) = self.set.get(name).copied() {
            return Ok(stored);
        }
        if self.set.len() >= self.limit {
            return Err(DriverNameError::TooMany);
        }
        let stored: &'static str = Box::leak(name.to_owned().into_boxed_str());
        self.set.insert(stored);
        Ok(stored)
    }

    /// The stored name equal to `name`; never stores one.
    fn lookup(&self, name: &str) -> Option<&'static str> {
        self.set.get(name).copied()
    }
}

/// The name of a driver: a lower-case letter followed by up to 63 lower-case
/// letters, digits or `_`. A `Copy` handle to a name the process stores
/// once; two names are equal when their text is.
///
/// A name written in code is [`from_static`](DriverName::from_static); a
/// name read from the app's configuration is [`new`](DriverName::new); any
/// other name (from a request, a stored row, a job payload) is
/// [`lookup`](DriverName::lookup), which only finds names the process
/// already knows.
///
/// ```
/// use navree_config::DriverName;
///
/// let name = DriverName::from_static("two_tier");
/// assert_eq!(name.as_str(), "two_tier");
/// assert_eq!(name.to_string(), "two_tier");
/// assert!(!DriverName::is_valid("Two-Tier"));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DriverName(&'static str);

impl DriverName {
    /// A name written in code. It does not touch the process's set of names.
    ///
    /// A name that is not a lower-case letter followed by up to 63
    /// lower-case letters, digits or `_` panics with `driver names are a
    /// lower-case letter followed by up to 63 lower-case letters, digits or
    /// _`; in a `const`, that fails the build.
    ///
    /// ```
    /// use navree_config::DriverName;
    ///
    /// const TWO_TIER: DriverName = DriverName::from_static("two_tier");
    /// assert_eq!(TWO_TIER.as_str(), "two_tier");
    /// ```
    ///
    /// ```compile_fail
    /// // a_bad_static_name_fails_in_a_const
    /// const BAD: navree_config::DriverName = navree_config::DriverName::from_static("Two-Tier");
    /// let _ = BAD;
    /// ```
    pub const fn from_static(name: &'static str) -> DriverName {
        if !DriverName::is_valid(name) {
            panic!(
                "driver names are a lower-case letter followed by up to 63 lower-case letters, digits or _"
            );
        }
        DriverName(name)
    }

    /// Puts a name written in code into the process's set, without copying
    /// it: what a registration method calls for the name it registers, and
    /// a driver kind for its built-in names, while modules register.
    pub fn register(name: &'static str) -> Result<DriverName, DriverNameError> {
        names().register(name).map(DriverName)
    }

    /// A name read from the app's configuration: checked, then stored once
    /// (the first time, as a copy). Only configuration values come through
    /// here.
    pub fn new(name: &str) -> Result<DriverName, DriverNameError> {
        names().intern(name).map(DriverName)
    }

    /// The name equal to `name` that the process already holds, if any. It
    /// never adds a name: every name read from a request, a stored row or a
    /// job payload goes through here.
    pub fn lookup(name: &str) -> Option<DriverName> {
        names().lookup(name).map(DriverName)
    }

    /// Whether `name` is a lower-case letter followed by up to 63 lower-case
    /// letters, digits or `_`.
    pub const fn is_valid(name: &str) -> bool {
        let bytes = name.as_bytes();
        if bytes.is_empty() || bytes.len() > 64 || !bytes[0].is_ascii_lowercase() {
            return false;
        }
        let mut i = 1;
        while i < bytes.len() {
            let byte = bytes[i];
            if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') {
                return false;
            }
            i += 1;
        }
        true
    }

    /// The name.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for DriverName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// The name as a quoted string: `"two_tier"`.
impl fmt::Debug for DriverName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0, f)
    }
}

/// The name, as a string.
impl Serialize for DriverName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

/// A string naming a driver the process already knows, through
/// [`DriverName::lookup`]: reading one never adds a name, and the error for
/// an unknown or malformed one never repeats the value.
impl<'de> Deserialize<'de> for DriverName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        DriverName::lookup(&name)
            .ok_or_else(|| de::Error::custom("not the name of a registered driver"))
    }
}

/// Why a driver name was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DriverNameError {
    /// The name is not a lower-case letter followed by up to 63 lower-case
    /// letters, digits or `_`.
    #[error(
        "a driver name is a lower-case letter followed by up to 63 lower-case letters, digits or _"
    )]
    Invalid,
    /// The process already holds 1024 distinct driver names.
    #[error("too many driver names in one process (at most 1024)")]
    TooMany,
}

/// What a driver's factory gets: the app, the driver's name, and where its
/// settings are.
///
/// The driver kind's module builds it when it builds the driver at boot. A
/// factory's error is shown in the boot error as written, so it must not
/// contain a secret from the driver's settings.
///
/// ```
/// use navree_config::DriverContext;
/// use navree_kernel::BoxError;
/// use serde::Deserialize;
///
/// /// The settings of a cache store registered as `two_tier`: `[cache.two_tier]`.
/// #[derive(Default, Deserialize)]
/// #[serde(default)]
/// struct TwoTierSettings {
///     capacity: usize,
/// }
///
/// // A driver's factory reads its settings from the context it is given.
/// fn capacity(ctx: &DriverContext) -> Result<usize, BoxError> {
///     let settings: TwoTierSettings = ctx.settings()?;
///     Ok(settings.capacity.max(1))
/// }
/// ```
#[derive(Clone)]
pub struct DriverContext {
    app: AppContext,
    name: DriverName,
    settings_key: String,
    instance: Option<String>,
    config: Option<Arc<Config>>,
    /// The table the driver kind's module holds for the driver (a disk's
    /// table, a provider table given in code); `None` reads the
    /// configuration at `settings_key`.
    held: Option<Json>,
}

impl DriverContext {
    /// The context of driver `name`, whose settings are at `settings_key`,
    /// built for `instance` (a disk, a provider table) when the kind has
    /// several. Reads the app's [`Config`] when it has one.
    #[doc(hidden)]
    pub async fn new(
        app: &AppContext,
        name: DriverName,
        settings_key: impl Into<String>,
        instance: Option<String>,
    ) -> DriverContext {
        let settings_key = settings_key.into();
        let config = app.try_get::<Config>().await;
        DriverContext {
            app: app.clone(),
            name,
            settings_key,
            instance,
            config,
            held: None,
        }
    }

    /// The name the driver was registered and selected under.
    pub fn name(&self) -> DriverName {
        self.name
    }

    /// The app, to resolve what the driver needs. The factory runs while
    /// its kind's service is being built (`Storage` for a disk driver,
    /// `Cache` for a cache store): asking for that service, or for anything
    /// built on it, fails the boot with the cycle, naming the driver. A
    /// driver that needs it keeps this handle and asks once the app runs.
    pub fn app(&self) -> &AppContext {
        &self.app
    }

    /// Where the driver's settings are: `cache.two_tier`,
    /// `storage.disks.uploads`, `ai.providers.vaani`.
    pub fn settings_key(&self) -> &str {
        &self.settings_key
    }

    /// The disk (storage) or the provider table (AI) the driver is built
    /// for; `None` for the other kinds.
    pub fn instance(&self) -> Option<&str> {
        self.instance.as_deref()
    }

    /// The settings, typed: the table the driver kind's module runs with
    /// when the module holds one (a disk's table without `driver`, a
    /// provider table given to `AiModule::for_root` in code), else the
    /// table at the settings key in the configuration. A missing table is
    /// `T::default()`. Values convert as the configuration's do, so
    /// `"2222"` from the environment fills a `u16`. An error names the key
    /// and never a value; so does the error of an app with no
    /// `ConfigModule`.
    pub fn settings<T: DeserializeOwned + Default>(&self) -> Result<T, BoxError> {
        if let Some(held) = &self.held {
            return read_settings(held).map_err(|message| self.unreadable(message));
        }
        let Some(config) = &self.config else {
            return Err(format!(
                "{}: the app has no ConfigModule, so driver `{}` cannot read its settings",
                self.settings_key, self.name
            )
            .into());
        };
        match self.configured(config) {
            Some(table) => read_table(table).map_err(|message| self.unreadable(message)),
            None => Ok(T::default()),
        }
    }

    /// The error of settings that do not read, at the settings key.
    fn unreadable(&self, message: String) -> BoxError {
        ConfigError::Section {
            key: self.settings_key.clone(),
            message,
        }
        .into()
    }

    /// The table of the driver's settings in `config`: for a driver built
    /// for an instance, that instance's own table in the table above it (so
    /// a table whose name holds a dot is found), else the table at the
    /// settings key.
    fn configured<'c>(&self, config: &'c Config) -> Option<&'c Value> {
        let below = self.instance.as_deref().and_then(|instance| {
            self.settings_key
                .strip_suffix(instance)
                .and_then(|rest| rest.strip_suffix('.'))
                .map(|parent| (parent, instance))
        });
        match below {
            Some((parent, instance)) => match &config.lookup(parent)?.kind {
                Kind::Table(tables) => tables.get(instance),
                _ => None,
            },
            None => config.lookup(&self.settings_key),
        }
    }
}

/// Reads `table`, the settings a driver kind's module holds for a driver
/// (a disk's table), as `T`, exactly as `DriverContext::settings` reads
/// them: values convert as the configuration's do (`"2222"` from the
/// environment fills a `u16`, `"true"` a `bool`), and the error, its
/// message alone, names the setting and never a value of the table.
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
pub fn read_settings<T: DeserializeOwned>(table: &Json) -> Result<T, String> {
    let limit = ConfigSources::DEFAULT_MAX_DEPTH;
    let table = from_held_json(table, limit)
        .map_err(|_| format!("the settings nest deeper than the limit of {limit} levels"))?;
    read_table(&table)
}

/// Reads `table`, a driver's settings, as `T`, with the configuration's
/// reader: a string fills a number or a boolean. The error's message names
/// the setting below the table and no value of it: numbers and booleans
/// are masked as under a secret key, quoted strings are masked, and every
/// value of the table is scrubbed from what is left.
fn read_table<T: DeserializeOwned>(table: &Value) -> Result<T, String> {
    deserialize_tracked(table, "", &Sink::default())
        .map_err(|error| scrub(redact_values(&error.render(true)), &every_value(table)))
}

/// Every value of `table` as text (each string, number and boolean, and
/// the password of a URL in a string): none may appear in an error about a
/// driver's settings.
fn every_value(table: &Value) -> Vec<String> {
    let mut found = secret_values(table);
    let mut pending = vec![table];
    while let Some(value) = pending.pop() {
        match &value.kind {
            Kind::String(text) if !text.is_empty() => found.push(text.clone()),
            Kind::Bool(b) => found.push(b.to_string()),
            Kind::Int(i) => found.push(i.to_string()),
            Kind::UInt(u) => found.push(u.to_string()),
            Kind::Float(f) => found.push(f.to_string()),
            Kind::Array(items) => pending.extend(items),
            Kind::Table(map) => pending.extend(map.values()),
            Kind::String(_) | Kind::Null => {}
        }
    }
    found
}

/// The driver's name, its settings key and its instance; never a setting.
impl fmt::Debug for DriverContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DriverContext")
            .field("name", &self.name)
            .field("settings_key", &self.settings_key)
            .field("instance", &self.instance)
            .finish_non_exhaustive()
    }
}

/// One selector a driver kind's module reads: its key, its built-in names,
/// the method that registers more, and the kind in words. The module
/// contributes it, so `config:check` and `config:schema` know the selector
/// without booting the app.
///
/// ```
/// use navree_config::DriverSlot;
///
/// const SLOT: DriverSlot =
///     DriverSlot::new("cache.store", &["memory", "redis"], "m.cache_store", "cache store");
/// assert_eq!(SLOT.key(), "cache.store");
/// assert_eq!(SLOT.built_in(), ["memory", "redis"]);
/// assert_eq!(SLOT.register(), "m.cache_store");
/// assert_eq!(SLOT.what(), "cache store");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DriverSlot {
    key: &'static str,
    built_in: &'static [&'static str],
    register: &'static str,
    what: &'static str,
    table_name: bool,
}

impl DriverSlot {
    /// The selector at `key` (`cache.store`), with one `*` for a table of
    /// tables (`storage.disks.*.driver`, `ai.providers.*.driver`);
    /// `built_in` are its built-in names, `register` the method that
    /// registers more (`m.cache_store`) and `what` the driver's kind in
    /// words (`cache store`).
    pub const fn new(
        key: &'static str,
        built_in: &'static [&'static str],
        register: &'static str,
        what: &'static str,
    ) -> DriverSlot {
        DriverSlot {
            key,
            built_in,
            register,
            what,
            table_name: false,
        }
    }

    /// For a `*` key: a table with no value at the key names its driver by
    /// its own name (`[ai.providers.openai]` means `driver = "openai"`).
    pub const fn defaults_to_table_name(self) -> DriverSlot {
        DriverSlot {
            table_name: true,
            ..self
        }
    }

    /// The selector's key.
    pub fn key(&self) -> &'static str {
        self.key
    }

    /// The built-in names, in order.
    pub fn built_in(&self) -> &'static [&'static str] {
        self.built_in
    }

    /// The method that registers more names.
    pub fn register(&self) -> &'static str {
        self.register
    }

    /// The driver's kind in words.
    pub fn what(&self) -> &'static str {
        self.what
    }
}

/// A driver a module registered: the key of its slot and its name. Every
/// registration method contributes one.
///
/// ```
/// use navree_config::{DriverName, RegisteredDriver};
///
/// let driver = RegisteredDriver::new("cache.store", DriverName::from_static("two_tier"));
/// assert_eq!(driver.slot(), "cache.store");
/// assert_eq!(driver.name().as_str(), "two_tier");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RegisteredDriver {
    slot: &'static str,
    name: DriverName,
}

impl RegisteredDriver {
    /// The registration of `name` for the slot whose key is `slot`.
    pub const fn new(slot: &'static str, name: DriverName) -> RegisteredDriver {
        RegisteredDriver { slot, name }
    }

    /// The key of the slot.
    pub fn slot(&self) -> &'static str {
        self.slot
    }

    /// The registered name.
    pub fn name(&self) -> DriverName {
        self.name
    }
}

/// A registration as a module wrote it: its slot, the name it gave, and why
/// the name was refused, if it was. Kept for a refused name too, so the
/// boot can say which module gave it.
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct DriverRecord {
    slot: &'static str,
    name: &'static str,
    refused: Option<DriverNameError>,
}

impl DriverRecord {
    /// The registration of `name` for the slot whose key is `slot`.
    pub fn new(
        slot: &'static str,
        name: &'static str,
        refused: Option<DriverNameError>,
    ) -> DriverRecord {
        DriverRecord {
            slot,
            name,
            refused,
        }
    }
}

/// What builds a driver whose contract is `X`.
type BuildDriver<X> =
    Arc<dyn Fn(DriverContext) -> BoxFuture<'static, Result<Arc<X>, BoxError>> + Send + Sync>;

/// The factory a registration holds, for drivers whose contract is `X`.
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
pub struct DriverFactory<X: ?Sized> {
    slot: &'static str,
    name: DriverName,
    build: BuildDriver<X>,
}

impl<X: ?Sized> Clone for DriverFactory<X> {
    fn clone(&self) -> Self {
        DriverFactory {
            slot: self.slot,
            name: self.name,
            build: self.build.clone(),
        }
    }
}

/// Puts `name`, written in code, in the process's set as a name a module
/// registered for the slot whose key is `slot`: what every registration
/// method calls. [`lookup_in`] then finds it for that slot, and for no
/// other.
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
pub fn register_in(slot: &'static str, name: &'static str) -> Result<DriverName, DriverNameError> {
    names().register_in(slot, name).map(DriverName)
}

/// The name equal to `name` that a module registered for the slot whose
/// key is `slot`, if one did. A name read from a request, a stored row or a
/// job payload is known for a kind only so: the name of another kind's
/// driver, another kind's built-in name and a name only the configuration
/// gave are not. It never adds a name.
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
pub fn lookup_in(slot: &str, name: &str) -> Option<DriverName> {
    names().lookup_in(slot, name).map(DriverName)
}

/// Registers `name` for `slot`, built by `factory`. An accepted name
/// contributes its [`DriverRecord`], its [`DriverFactory`], a
/// [`RegisteredDriver`] and, with `settings = Some(section)`, a claim on
/// `{section}.{name}`, where the driver's settings are. A refused name
/// contributes its [`DriverRecord`] only, and the boot fails naming it.
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
pub fn register_driver<'m, X, F, Fut>(
    m: &'m mut ModuleBuilder,
    slot: &DriverSlot,
    settings: Option<&'static str>,
    name: &'static str,
    factory: F,
) -> &'m mut ModuleBuilder
where
    X: ?Sized + Send + Sync + 'static,
    F: Fn(DriverContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Arc<X>, BoxError>> + Send + 'static,
{
    match register_in(slot.key(), name) {
        Ok(driver) => {
            m.contribute(DriverRecord::new(slot.key(), name, None))
                .contribute(DriverFactory::<X> {
                    slot: slot.key(),
                    name: driver,
                    build: Arc::new(move |ctx| Box::pin(factory(ctx))),
                })
                .contribute(RegisteredDriver::new(slot.key(), driver));
            if let Some(section) = settings {
                m.config_claim(&format!("{section}.{name}"));
            }
            m
        }
        Err(error) => m.contribute(DriverRecord::new(slot.key(), name, Some(error))),
    }
}

/// Contributes `slot`, after putting its built-in names in the process's
/// set (a full set refuses them; they are known without being in it).
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
pub fn contribute_slot(m: &mut ModuleBuilder, slot: DriverSlot) -> &mut ModuleBuilder {
    for &name in slot.built_in() {
        let _ = DriverName::register(name);
    }
    m.contribute(slot)
}

/// What is wrong with the registrations of `slot` among `records`: in
/// the order they were made, a name that is refused, then one that is
/// built in, then one equal to a key of the section (`section_keys`);
/// then, in name order, each name registered twice or more, naming the
/// first two modules.
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
pub fn registration_problems(
    slot: &DriverSlot,
    section_keys: &[String],
    records: &[Contributed<DriverRecord>],
) -> Vec<String> {
    let mut problems = Vec::new();
    let mut accepted: BTreeMap<&'static str, Vec<&'static str>> = BTreeMap::new();
    for record in records
        .iter()
        .filter(|record| record.value.slot == slot.key())
    {
        let (name, module) = (record.value.name, record.module.name);
        match record.value.refused {
            Some(DriverNameError::Invalid) => problems.push(not_a_name(slot, name, module)),
            Some(DriverNameError::TooMany) => problems.push(DriverNameError::TooMany.to_string()),
            None if slot.built_in().contains(&name) => {
                problems.push(built_in_taken(slot, name, module));
            }
            None if section_keys.iter().any(|key| key.as_str() == name) => {
                problems.push(setting_taken(slot, name, module));
            }
            None => accepted.entry(name).or_default().push(module),
        }
    }
    for (name, modules) in &accepted {
        if let [first, second, ..] = modules.as_slice() {
            problems.push(twice(slot, name, first, second));
        }
    }
    problems
}

/// The names of the sections registered just below `section`, as the keys
/// they are in it, each once: `push` for the section `notify.push` below
/// `notify`. Such a section is a setting of `section` kept in a table of its
/// own, so no driver may take its name.
fn nested_section_keys<'a>(section: &str, sections: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for key in sections {
        let Some(rest) = key
            .strip_prefix(section)
            .and_then(|rest| rest.strip_prefix('.'))
        else {
            continue;
        };
        let first = rest.split('.').next().unwrap_or(rest);
        if !first.is_empty() && !keys.iter().any(|known| known == first) {
            keys.push(first.to_owned());
        }
    }
    keys
}

/// The first problem of the registrations of `slot` in `app`, as the boot
/// error. `section` is the schema of the section the slot is in; its
/// top-level keys, and the keys of the sections registered just below it
/// (`push` for `notify.push`), are names a registration may not take (a `*`
/// slot has none).
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
pub fn check_registrations(
    app: &AppContext,
    slot: &DriverSlot,
    section: Option<SectionSchema>,
) -> Result<(), BoxError> {
    let section_keys: Vec<String> = match section {
        Some(schema) if !slot.key().contains('*') => {
            let mut keys: Vec<String> = schema.fields.into_iter().map(|field| field.name).collect();
            let registered = app.contributions::<SectionRegistration>();
            keys.extend(nested_section_keys(
                section_of(slot.key()),
                registered
                    .iter()
                    .map(|registration| registration.value.key()),
            ));
            keys
        }
        _ => Vec::new(),
    };
    let records = app.contributions::<DriverRecord>();
    match registration_problems(slot, &section_keys, &records)
        .into_iter()
        .next()
    {
        Some(problem) => Err(problem.into()),
        None => Ok(()),
    }
}

/// Builds the driver registered as `name` for `slot`, with its settings at
/// `settings_key` and for `instance`, from the factory of the provider `r`
/// builds. `settings` is the table the kind's module holds for the driver
/// (a disk's table, a provider table given in code), which the factory
/// reads in place of the configuration's. No such registration is the
/// error that names `key` (the selector's key,
/// `storage.disks.uploads.driver` for a `*` slot) and the names that are
/// known; a failing factory is an error naming the driver, the method and
/// the module. The factory's app continues `r`'s chain, so a factory that
/// asks it for a service being built on the way to the driver (the kind's
/// own) fails with that cycle instead of waiting for ever.
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
pub async fn build_driver<X: ?Sized + Send + Sync + 'static>(
    r: &Resolver,
    slot: &DriverSlot,
    key: &str,
    name: DriverName,
    settings_key: String,
    instance: Option<String>,
    settings: Option<Json>,
) -> Result<Arc<X>, BoxError> {
    let app = app_in_chain(r);
    let found = app
        .contributions::<DriverFactory<X>>()
        .into_iter()
        .find(|factory| factory.value.slot == slot.key() && factory.value.name == name);
    let Some(found) = found else {
        let drivers = registered(&app);
        return Err(unregistered(slot, key, name.as_str(), &drivers).into());
    };
    let mut ctx = DriverContext::new(&app, name, settings_key, instance).await;
    ctx.held = settings;
    (found.value.build)(ctx)
        .await
        .map_err(|error| failed(slot, name.as_str(), found.module.name, &error).into())
}

/// The error of a selector whose value is neither a built-in name nor a
/// driver name: it lists the built-in names and never repeats the value.
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
pub fn selector_error(field: &str, slot: &DriverSlot) -> String {
    format!(
        "the {field} must be {} or the name of {} {} registered with {} (a lower-case letter, then lower-case letters, digits or _)",
        built_ins(slot),
        article(slot.what()),
        slot.what(),
        slot.register()
    )
}

/// What a selector's hand-written `Deserialize` reads in place of a
/// `String`: a string, as it is written (the kind judges it), or a table
/// whose one key is a built-in name of `slot` (`{ memory = {} }`), as that
/// name, whatever its value. A table never names a registered driver.
/// Anything else (a table of another key, of no key or of two, a list, a
/// number, a boolean) is the error of [`selector_error`], which never
/// repeats the value. A format that is not human-readable is read for a
/// string only.
///
/// Not public API: what the driver kinds of Navree call.
#[doc(hidden)]
pub fn selector_name<'de, D: Deserializer<'de>>(
    deserializer: D,
    field: &str,
    slot: &DriverSlot,
) -> Result<String, D::Error> {
    let tables = deserializer.is_human_readable();
    let visitor = SelectorName {
        field,
        slot,
        tables,
    };
    if tables {
        deserializer.deserialize_any(visitor)
    } else {
        deserializer.deserialize_str(visitor)
    }
}

/// What [`selector_name`] reads: a string, or, with `tables`, a table whose
/// one key is a built-in name of `slot`. Every other value is the
/// selector's error.
struct SelectorName<'a> {
    field: &'a str,
    slot: &'a DriverSlot,
    tables: bool,
}

impl SelectorName<'_> {
    /// The selector's error, which never repeats the value.
    fn refuse<E: de::Error>(&self) -> E {
        E::custom(selector_error(self.field, self.slot))
    }
}

impl<'de> de::Visitor<'de> for SelectorName<'_> {
    type Value = String;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = self.slot.what();
        write!(f, "the name of {} {what}", article(what))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<String, E> {
        Ok(value.to_owned())
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<String, E> {
        Ok(value)
    }

    /// Bytes that are UTF-8 are a string, as `String` reads them.
    fn visit_bytes<E: de::Error>(self, value: &[u8]) -> Result<String, E> {
        std::str::from_utf8(value)
            .map(str::to_owned)
            .map_err(|_| self.refuse())
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<String, A::Error> {
        if !self.tables {
            return Err(self.refuse());
        }
        let Some(key) = map.next_key::<String>()? else {
            return Err(self.refuse());
        };
        if !self.slot.built_in().contains(&key.as_str()) {
            return Err(self.refuse());
        }
        map.next_value::<de::IgnoredAny>()?;
        if map.next_key::<de::IgnoredAny>()?.is_some() {
            return Err(self.refuse());
        }
        Ok(key)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<String, E> {
        Err(self.refuse())
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<String, E> {
        Err(self.refuse())
    }

    fn visit_i128<E: de::Error>(self, _: i128) -> Result<String, E> {
        Err(self.refuse())
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<String, E> {
        Err(self.refuse())
    }

    fn visit_u128<E: de::Error>(self, _: u128) -> Result<String, E> {
        Err(self.refuse())
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<String, E> {
        Err(self.refuse())
    }

    fn visit_none<E: de::Error>(self) -> Result<String, E> {
        Err(self.refuse())
    }

    fn visit_some<D: Deserializer<'de>>(self, _: D) -> Result<String, D::Error> {
        Err(self.refuse())
    }

    fn visit_unit<E: de::Error>(self) -> Result<String, E> {
        Err(self.refuse())
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, _: D) -> Result<String, D::Error> {
        Err(self.refuse())
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, _: A) -> Result<String, A::Error> {
        Err(self.refuse())
    }

    fn visit_enum<A: de::EnumAccess<'de>>(self, _: A) -> Result<String, A::Error> {
        Err(self.refuse())
    }
}

/// Every driver the modules of `app` registered.
fn registered(app: &AppContext) -> Vec<RegisteredDriver> {
    app.contributions::<RegisteredDriver>()
        .into_iter()
        .map(|contributed| (*contributed.value).clone())
        .collect()
}

/// `an` before a kind that starts with a vowel or reads as one (`AI`,
/// `SMS`), else `a`.
fn article(what: &str) -> &'static str {
    let vowel = what
        .chars()
        .next()
        .is_some_and(|c| matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u'));
    if vowel || what.starts_with("AI") || what.starts_with("SMS") {
        "an"
    } else {
        "a"
    }
}

/// The section a slot's key is in: its first segment (`cache` for
/// `cache.store`, `notify` for `notify.sms.driver`).
fn section_of(key: &str) -> &str {
    key.split('.').next().unwrap_or(key)
}

/// The built-in names of `slot`, each in backticks: `` `memory`, `redis` ``.
fn built_ins(slot: &DriverSlot) -> String {
    slot.built_in()
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The names `slot` knows: its built-in names in order, then the names
/// registered for it in name order.
fn known_names(slot: &DriverSlot, drivers: &[RegisteredDriver]) -> Vec<&'static str> {
    let registered: BTreeSet<&'static str> = drivers
        .iter()
        .filter(|driver| driver.slot() == slot.key())
        .map(|driver| driver.name().as_str())
        .filter(|name| !slot.built_in().contains(name))
        .collect();
    slot.built_in().iter().copied().chain(registered).collect()
}

/// [`known_names`], joined by `, `.
fn known(slot: &DriverSlot, drivers: &[RegisteredDriver]) -> String {
    known_names(slot, drivers).join(", ")
}

/// `{key}` names `{name}`, which no module registered for `slot`.
pub(crate) fn unregistered(
    slot: &DriverSlot,
    key: &str,
    name: &str,
    drivers: &[RegisteredDriver],
) -> String {
    format!(
        "{key} is `{name}`, but no module registers {} {} under that name with {} (known: {})",
        article(slot.what()),
        slot.what(),
        slot.register(),
        known(slot, drivers)
    )
}

/// Two modules registered `name` for `slot`.
fn twice(slot: &DriverSlot, name: &str, first: &str, second: &str) -> String {
    format!(
        "{} `{name}` is registered by both `{first}` and `{second}`; register it once",
        slot.what()
    )
}

/// `module` registered a built-in name.
fn built_in_taken(slot: &DriverSlot, name: &str, module: &str) -> String {
    format!(
        "{}(\"{name}\") in `{module}`: `{name}` is a built-in {}; register yours under another name",
        slot.register(),
        slot.what()
    )
}

/// `module` registered a name that is not a driver name.
fn not_a_name(slot: &DriverSlot, name: &str, module: &str) -> String {
    format!(
        "{}(\"{name}\") in `{module}`: {} {} name is a lower-case letter followed by up to 63 lower-case letters, digits or _",
        slot.register(),
        article(slot.what()),
        slot.what()
    )
}

/// `module` registered a name that is a key of the slot's section.
fn setting_taken(slot: &DriverSlot, name: &str, module: &str) -> String {
    let section = section_of(slot.key());
    format!(
        "{}(\"{name}\") in `{module}`: `{section}.{name}` is a setting of the {section} section; register the {} under another name",
        slot.register(),
        slot.what()
    )
}

/// The factory of `name`, registered by `module`, failed with `error`.
fn failed(slot: &DriverSlot, name: &str, module: &str, error: &BoxError) -> String {
    format!(
        "{} `{name}` (registered with {} in `{module}`) failed to start: {error}",
        slot.what(),
        slot.register()
    )
}

/// Whether a module registered `name` for `slot`.
fn is_registered(slot: &DriverSlot, name: &str, drivers: &[RegisteredDriver]) -> bool {
    drivers
        .iter()
        .any(|driver| driver.slot() == slot.key() && driver.name().as_str() == name)
}

/// The tables of the `*` slot `slot` in `config`, by name, each with the
/// driver it names: the value at the key below the table, or, for a slot
/// that defaults to the table's name, the table's own name. An empty table
/// counts.
pub(crate) fn instance_tables(config: &Config, slot: &DriverSlot) -> Vec<(String, Option<String>)> {
    let Some((prefix, suffix)) = slot.key().split_once(".*.") else {
        return Vec::new();
    };
    let Some(Value {
        kind: Kind::Table(tables),
        ..
    }) = config.lookup(prefix)
    else {
        return Vec::new();
    };
    tables
        .iter()
        .filter(|(_, table)| table.is_table())
        .map(|(name, table)| {
            let driver = table
                .lookup(suffix)
                .and_then(Value::scalar_string)
                .or_else(|| slot.table_name.then(|| name.clone()));
            (name.clone(), driver)
        })
        .collect()
}

/// The keys of the tables of the `*` slots among `slots` whose driver a
/// module registered (`storage.disks.uploads`): the driver reads such a
/// table whole, so none of its keys is unknown.
pub(crate) fn registered_tables(
    config: &Config,
    slots: &[DriverSlot],
    drivers: &[RegisteredDriver],
) -> Vec<String> {
    let mut tables = Vec::new();
    for slot in slots {
        let Some((prefix, _)) = slot.key().split_once(".*.") else {
            continue;
        };
        for (table, driver) in instance_tables(config, slot) {
            if driver.is_some_and(|driver| is_registered(slot, &driver, drivers)) {
                tables.push(format!("{prefix}.{table}"));
            }
        }
    }
    tables
}

/// The driver names `config` gives `slot`, each with its key: the value at
/// the slot's key, or, for a `*` slot, each table's driver
/// (`storage.disks.uploads.driver`).
fn configured(config: &Config, slot: &DriverSlot) -> Vec<(String, String)> {
    match slot.key().split_once(".*.") {
        None => config
            .lookup(slot.key())
            .and_then(Value::scalar_string)
            .map(|name| (slot.key().to_owned(), name))
            .into_iter()
            .collect(),
        Some((prefix, suffix)) => instance_tables(config, slot)
            .into_iter()
            .filter_map(|(table, driver)| {
                driver.map(|driver| (format!("{prefix}.{table}.{suffix}"), driver))
            })
            .collect(),
    }
}

/// The keys of the sections that failed to read or validate.
fn failing_sections(errors: &[ConfigError]) -> Vec<String> {
    let mut keys = Vec::new();
    for error in errors {
        match error {
            ConfigError::Invalid { problems } => keys.extend(failing_sections(problems)),
            ConfigError::Section { key, .. }
            | ConfigError::Validation { key, .. }
            | ConfigError::MissingSection { key } => keys.push(key.clone()),
            _ => {}
        }
    }
    keys
}

/// What [`CheckReport::with_drivers`] adds: an error for each configured
/// driver name of `slots`, read trimmed and in lower case (as search reads
/// `search.engine`), that is neither built in nor registered (not for a
/// slot whose section already has an error), and no unknown key at or
/// below a table a registered driver reads.
pub(crate) fn check_names(
    report: &mut CheckReport,
    sources: &ConfigSources,
    slots: &[DriverSlot],
    drivers: &[RegisteredDriver],
) {
    if slots.is_empty() {
        return;
    }
    let Ok(config) = Config::load(sources) else {
        return;
    };
    let tables = registered_tables(&config, slots, drivers);
    report
        .unknown
        .retain(|unknown| !tables.iter().any(|table| covers(table, &unknown.key)));
    let failing = failing_sections(&report.errors);
    for slot in slots {
        if failing.iter().any(|section| covers(section, slot.key())) {
            continue;
        }
        for (key, name) in configured(&config, slot) {
            let name = name.trim().to_lowercase();
            if DriverName::is_valid(&name)
                && !slot.built_in().contains(&name.as_str())
                && !is_registered(slot, &name, drivers)
            {
                let message = unregistered(slot, &key, &name, drivers);
                report.errors.push(ConfigError::Driver { key, message });
            }
        }
    }
}

/// The top-level keys of the section `slot` is in, as the registry
/// describes it, and the keys of the sections registered just below it;
/// none for a `*` slot.
pub(crate) fn section_keys(registry: &ConfigRegistry, slot: &DriverSlot) -> Vec<String> {
    if slot.key().contains('*') {
        return Vec::new();
    }
    let section = section_of(slot.key());
    let mut keys: Vec<String> = registry
        .section(section)
        .and_then(|info| info.schema.as_ref())
        .map(|schema| {
            schema
                .fields
                .iter()
                .map(|field| field.name.clone())
                .collect()
        })
        .unwrap_or_default();
    keys.extend(nested_section_keys(
        section,
        registry.sections().iter().map(|info| info.key),
    ));
    keys
}

/// For each slot: its key, its built-in names then the names registered
/// for it in name order, and its method.
pub(crate) fn slot_names(
    slots: &[DriverSlot],
    drivers: &[RegisteredDriver],
) -> Vec<(&'static str, Vec<String>, &'static str)> {
    slots
        .iter()
        .map(|slot| {
            let names = known_names(slot, drivers)
                .into_iter()
                .map(str::to_owned)
                .collect();
            (slot.key(), names, slot.register())
        })
        .collect()
}

/// Closes, in the JSON Schema `root`, the open choice of each slot whose
/// key has no `*` to the names that boot (see [`slot_names`]), keeping the
/// field's description. Only a field that describes itself as driver names
/// is closed: a setting that also takes a URL or a table, such as
/// `mail.transport`, keeps the schema it has.
pub(crate) fn close_choices(root: &mut Json, slots: &[(&'static str, Vec<String>, &'static str)]) {
    for (key, names, register) in slots {
        if key.contains('*') {
            continue;
        }
        let pointer: String = key
            .split('.')
            .map(|segment| {
                format!(
                    "/properties/{}",
                    segment.replace('~', "~0").replace('/', "~1")
                )
            })
            .collect();
        let Some(node) = root.pointer_mut(&pointer) else {
            continue;
        };
        if node.get("x-navree-register").is_none() {
            continue;
        }
        let description = node.get("description").cloned();
        let mut closed = json!({
            "type": "string",
            "enum": names,
            "x-navree-register": register,
        });
        if let Some(description) = description {
            closed["description"] = description;
        }
        *node = closed;
    }
}

#[cfg(test)]
mod tests {
    use navree_kernel::{ModuleId, ModuleInfo};
    use serde::de::Visitor;

    use super::*;

    #[test]
    fn interned_names_are_stored_once_and_compare_by_text() {
        let read = DriverName::new("amritsar_cache").unwrap();
        let again = DriverName::new("amritsar_cache").unwrap();
        assert!(std::ptr::eq(read.as_str(), again.as_str()), "one copy");
        assert_eq!(
            read,
            DriverName::from_static("amritsar_cache"),
            "equality is by text"
        );
        assert_eq!(read.to_string(), "amritsar_cache");
        assert_eq!(format!("{read:?}"), "\"amritsar_cache\"");
        assert_eq!(DriverName::new("Amritsar"), Err(DriverNameError::Invalid));
        assert_eq!(
            DriverName::new("amritsar-cache"),
            Err(DriverNameError::Invalid)
        );
        assert_eq!(
            DriverName::new(&"a".repeat(65)),
            Err(DriverNameError::Invalid)
        );
        assert!(DriverName::is_valid(&"a".repeat(64)));
    }

    #[test]
    fn lookup_never_inserts_and_register_stores_without_copying() {
        assert_eq!(DriverName::lookup("amritsar_queue"), None);
        assert_eq!(
            DriverName::lookup("amritsar_queue"),
            None,
            "a miss adds nothing"
        );
        static NAME: &str = "amritsar_queue";
        let registered = DriverName::register(NAME).unwrap();
        assert!(
            std::ptr::eq(registered.as_str(), NAME),
            "the given text, not a copy"
        );
        assert_eq!(DriverName::lookup("amritsar_queue"), Some(registered));
        assert_eq!(
            DriverName::register("Amritsar_Queue"),
            Err(DriverNameError::Invalid)
        );
    }

    #[test]
    fn the_1025th_distinct_name_is_refused() {
        // A set of its own, so the process's names are not used up.
        let mut names = Names::with_limit(LIMIT);
        for i in 0..LIMIT {
            names.intern(&format!("ludhiana_{i}")).unwrap();
        }
        assert_eq!(
            names.intern("ludhiana_extra"),
            Err(DriverNameError::TooMany)
        );
        assert_eq!(
            names.intern("ludhiana_7").map(str::len),
            Ok(10),
            "a known name is still found"
        );
        assert_eq!(LIMIT, 1024);
        assert_eq!(
            DriverNameError::TooMany.to_string(),
            "too many driver names in one process (at most 1024)"
        );
    }

    #[test]
    fn a_name_is_known_for_the_kind_it_was_registered_for_only() {
        // A set of its own, then the process's.
        let mut names = Names::with_limit(LIMIT);
        names.register_in("vault.kind", "jalandhar_chest").unwrap();
        names.intern("jalandhar_box").unwrap();
        assert_eq!(
            names.lookup_in("vault.kind", "jalandhar_chest"),
            Some("jalandhar_chest")
        );
        assert_eq!(
            names.lookup_in("vault.lock", "jalandhar_chest"),
            None,
            "another kind's name"
        );
        assert_eq!(
            names.lookup_in("vault.kind", "jalandhar_box"),
            None,
            "a configured name is not a registration"
        );
        assert_eq!(names.lookup_in("vault.kind", "jalandhar_made_up"), None);
        assert_eq!(names.lookup("jalandhar_made_up"), None, "a miss adds nothing");

        let chest = register_in("vault.kind", "amritsar_chest").unwrap();
        assert_eq!(lookup_in("vault.kind", "amritsar_chest"), Some(chest));
        assert_eq!(lookup_in("vault.lock", "amritsar_chest"), None);
        assert_eq!(DriverName::lookup("amritsar_chest"), Some(chest));
        assert_eq!(lookup_in("vault.kind", "amritsar_made_up"), None);
        assert_eq!(DriverName::lookup("amritsar_made_up"), None);
        assert_eq!(
            register_in("vault.kind", "Amritsar Chest"),
            Err(DriverNameError::Invalid)
        );
    }

    #[test]
    fn driver_names_deserialize_without_repeating_a_bad_value() {
        DriverName::register("amritsar_mail").unwrap();
        let name: DriverName = serde_json::from_value(serde_json::json!("amritsar_mail")).unwrap();
        assert_eq!(name.as_str(), "amritsar_mail");
        assert_eq!(
            serde_json::to_value(name).unwrap(),
            serde_json::json!("amritsar_mail")
        );
        for bad in ["amritsar_unknown_9f3", "Not Valid!"] {
            let error = serde_json::from_value::<DriverName>(serde_json::json!(bad))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("not the name of a registered driver"),
                "{error}"
            );
            assert!(!error.contains(bad), "{error}");
        }
        assert_eq!(
            DriverName::lookup("amritsar_unknown_9f3"),
            None,
            "deserializing never inserts"
        );
    }

    const SLOT: DriverSlot =
        DriverSlot::new("vault.kind", &["memory"], "m.vault_kind", "vault kind");

    #[test]
    fn every_kind_reads_with_its_article() {
        for what in [
            "cache store",
            "queue store",
            "session store",
            "backplane",
            "mail transport",
            "disk driver",
            "WhatsApp driver",
            "search engine",
            "notification channel",
        ] {
            assert_eq!(article(what), "a", "{what}");
        }
        for what in ["AI provider driver", "SMS driver"] {
            assert_eq!(article(what), "an", "{what}");
        }
    }

    /// `name`, registered for `SLOT` by `module`.
    fn record(
        module: &'static str,
        name: &'static str,
        refused: Option<DriverNameError>,
    ) -> Contributed<DriverRecord> {
        Contributed {
            module: ModuleInfo {
                id: ModuleId::named::<DriverRecord>(module),
                name: module,
                global: false,
            },
            value: Arc::new(DriverRecord::new(SLOT.key(), name, refused)),
        }
    }

    #[test]
    fn registration_problems_name_the_method_the_module_and_the_name() {
        let records = [
            record("ChestModule", "chest", None),
            record("OtherChestModule", "chest", None),
            record("MemoryModule", "memory", None),
            record("BadModule", "Chest", Some(DriverNameError::Invalid)),
            record("PrefixModule", "prefix", None),
        ];
        let section_keys = ["prefix".to_owned(), "kind".to_owned()];
        assert_eq!(
            registration_problems(&SLOT, &section_keys, &records),
            [
                "m.vault_kind(\"memory\") in `MemoryModule`: `memory` is a built-in vault kind; register yours under another name",
                "m.vault_kind(\"Chest\") in `BadModule`: a vault kind name is a lower-case letter followed by up to 63 lower-case letters, digits or _",
                "m.vault_kind(\"prefix\") in `PrefixModule`: `vault.prefix` is a setting of the vault section; register the vault kind under another name",
                "vault kind `chest` is registered by both `ChestModule` and `OtherChestModule`; register it once",
            ]
        );
        // The records of another slot are not this slot's.
        let other = DriverSlot::new("vault.lock", &[], "m.vault_lock", "vault lock");
        assert!(registration_problems(&other, &section_keys, &records).is_empty());
    }

    #[test]
    fn the_selector_text_never_repeats_the_value() {
        assert_eq!(
            selector_error("kind", &SLOT),
            "the kind must be `memory` or the name of a vault kind registered with m.vault_kind (a lower-case letter, then lower-case letters, digits or _)"
        );
        let cache = DriverSlot::new(
            "cache.store",
            &["memory", "redis"],
            "m.cache_store",
            "cache store",
        );
        assert_eq!(
            selector_error("store", &cache),
            "the store must be `memory`, `redis` or the name of a cache store registered with m.cache_store (a lower-case letter, then lower-case letters, digits or _)"
        );
    }

    #[test]
    fn only_a_selector_that_describes_itself_as_driver_names_is_closed() {
        let transport = serde_json::json!({ "description": "a name, a URL or a table" });
        let mut document = serde_json::json!({
            "properties": {
                "vault": {
                    "properties": {
                        "kind": {
                            "type": "string",
                            "anyOf": [{ "enum": ["memory"] }],
                            "x-navree-register": "m.vault_kind",
                        },
                        "transport": transport.clone(),
                    },
                },
            },
        });
        close_choices(
            &mut document,
            &[
                (
                    "vault.kind",
                    vec!["memory".to_owned(), "chest".to_owned()],
                    "m.vault_kind",
                ),
                (
                    "vault.transport",
                    vec!["memory".to_owned()],
                    "m.vault_transport",
                ),
            ],
        );
        let vault = &document["properties"]["vault"]["properties"];
        assert_eq!(
            vault["kind"]["enum"],
            serde_json::json!(["memory", "chest"])
        );
        assert_eq!(vault["kind"].get("anyOf"), None, "the choice is closed");
        assert_eq!(vault["transport"], transport, "a free setting stays free");
    }

    /// A format that is not human-readable (as bincode or postcard are),
    /// holding one string: it reads only through `deserialize_str`.
    struct Compact(&'static str);

    impl<'de> Deserializer<'de> for Compact {
        type Error = de::value::Error;

        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
            Err(de::Error::custom("this format reads a string only"))
        }

        fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
            visitor.visit_borrowed_str(self.0)
        }

        fn is_human_readable(&self) -> bool {
            false
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char string
            bytes byte_buf option unit unit_struct newtype_struct seq tuple
            tuple_struct map struct enum identifier ignored_any
        }
    }

    #[test]
    fn selector_name_reads_a_string_or_a_one_key_table_of_a_built_in_name() {
        let read = |value: Json| selector_name(value, "kind", &SLOT).map_err(|e| e.to_string());
        assert_eq!(
            read(serde_json::json!("chest")),
            Ok("chest".to_owned()),
            "a string is returned as written; the kind judges it"
        );
        for table in [
            serde_json::json!({"memory": {}}),
            serde_json::json!({"memory": null}),
            serde_json::json!({"memory": {"size": 3}}),
        ] {
            assert_eq!(read(table.clone()), Ok("memory".to_owned()), "{table}");
        }
        let refused = selector_error("kind", &SLOT);
        for value in [
            // A table never names a registered driver.
            serde_json::json!({"chest": {}}),
            serde_json::json!({"memory": {}, "chest": {}}),
            serde_json::json!({}),
            serde_json::json!(["memory"]),
            serde_json::json!(5),
            serde_json::json!(true),
        ] {
            let error = read(value.clone()).unwrap_err();
            assert_eq!(error, refused, "{value}");
            assert!(!error.contains("chest"), "{error}");
        }
        // A format that is not human-readable is read for a string only.
        assert_eq!(
            selector_name(Compact("memory"), "kind", &SLOT).unwrap(),
            "memory"
        );
    }

    /// A section whose selector is read as the driver kinds read theirs.
    #[derive(Deserialize)]
    struct Vault {
        #[serde(deserialize_with = "vault_kind")]
        kind: String,
    }

    fn vault_kind<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
        selector_name(deserializer, "kind", &SLOT)
    }

    #[test]
    fn the_configuration_reads_a_selector_written_as_a_one_key_table_as_before() {
        let read = |kind: Json| {
            let sources = ConfigSources::new().set("vault", serde_json::json!({"kind": kind}));
            Config::load(&sources)
                .unwrap()
                .section::<Vault>("vault")
                .map(|vault| vault.kind)
                .map_err(|error| error.to_string())
        };
        assert_eq!(read(serde_json::json!("chest")), Ok("chest".to_owned()));
        assert_eq!(
            read(serde_json::json!({"memory": {"size": 3}})),
            Ok("memory".to_owned())
        );
        let error = read(serde_json::json!({"chest": {"size": 3}})).unwrap_err();
        assert!(error.contains(&selector_error("kind", &SLOT)), "{error}");
        assert!(!error.contains("chest"), "{error}");
    }
}
