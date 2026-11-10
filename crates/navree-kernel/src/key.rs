//! Identities used by the container: modules, providers and scopes.

use std::any::TypeId;
use std::borrow::Cow;
use std::fmt;
use std::hash::{Hash, Hasher};

/// Identity of a module: its Rust type plus a readable name.
///
/// Two module values with the same `ModuleId` are the same module: the kernel
/// registers each id once (see [`Module::is_configured`](crate::Module::is_configured)).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ModuleId {
    type_id: TypeId,
    name: &'static str,
}

impl ModuleId {
    /// The id of module type `M`, named after the short type name of `M`.
    pub fn of<M: ?Sized + 'static>() -> Self {
        Self {
            type_id: TypeId::of::<M>(),
            name: short_type_name(std::any::type_name::<M>()),
        }
    }

    /// A module id for type `M` with an explicit display name.
    pub(crate) fn with_name<M: ?Sized + 'static>(name: &'static str) -> Self {
        Self {
            type_id: TypeId::of::<M>(),
            name,
        }
    }

    /// The readable module name (the short type name, e.g. `UsersModule`).
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

/// Key of a provider in the container: a type plus an optional name, so
/// `Db` and `Db("analytics")` can coexist.
#[derive(Clone, Debug)]
pub struct ProviderKey {
    type_id: TypeId,
    type_name: &'static str,
    name: Option<Cow<'static, str>>,
}

impl ProviderKey {
    /// The unnamed key of type `T` (which may be a trait object such as `dyn Mailer`).
    pub fn of<T: ?Sized + 'static>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            name: None,
        }
    }

    /// The key of type `T` registered under `name`.
    pub fn named<T: ?Sized + 'static>(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: Some(name.into()),
            ..Self::of::<T>()
        }
    }

    /// The full Rust type name of the provided type (e.g. `navree_db::Db`).
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }

    /// The provider name, if the key is named.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub(crate) fn type_id(&self) -> TypeId {
        self.type_id
    }
}

// `type_name` is derived from `type_id`, so identity is (type, name).
impl PartialEq for ProviderKey {
    fn eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id && self.name == other.name
    }
}

impl Eq for ProviderKey {}

impl Hash for ProviderKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.type_id.hash(state);
        self.name.hash(state);
    }
}

impl fmt::Display for ProviderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.name {
            Some(name) => write!(f, "{}(\"{name}\")", self.type_name),
            None => f.write_str(self.type_name),
        }
    }
}

/// How long a provided instance lives.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum Scope {
    /// Built once per app (eagerly at boot) and shared as `Arc<T>`.
    #[default]
    Singleton,
    /// Built once per unit of work ([`RequestScope`](crate::RequestScope)).
    Request,
    /// Built on every resolution.
    Transient,
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Scope::Singleton => "singleton",
            Scope::Request => "request",
            Scope::Transient => "transient",
        })
    }
}

/// Strips the module path from a type name, keeping generic arguments:
/// `app::users::UsersModule` → `UsersModule`, `a::Wrap<b::C>` → `Wrap<b::C>`.
pub(crate) fn short_type_name(full: &'static str) -> &'static str {
    let generics = full.find('<').unwrap_or(full.len());
    match full[..generics].rfind("::") {
        Some(pos) => &full[pos + 2..],
        None => full,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_type_name_strips_paths_but_keeps_generics() {
        assert_eq!(short_type_name("app::users::UsersModule"), "UsersModule");
        assert_eq!(short_type_name("Plain"), "Plain");
        assert_eq!(short_type_name("a::Wrap<b::C>"), "Wrap<b::C>");
    }

    #[test]
    fn named_keys_differ_from_unnamed_and_display_their_name() {
        let plain = ProviderKey::of::<u8>();
        let named = ProviderKey::named::<u8>("analytics");
        assert_ne!(plain, named);
        assert_eq!(named, ProviderKey::named::<u8>(String::from("analytics")));
        assert_eq!(named.to_string(), "u8(\"analytics\")");
        assert_eq!(named.name(), Some("analytics"));
    }
}
