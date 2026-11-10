//! Helpers shared by the kernel integration tests.

#![allow(unused_macros, unused_imports, dead_code)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Boots an app with one root module and returns the boot errors.
pub async fn boot_errors(root: impl navree_kernel::Module) -> navree_kernel::BootErrors {
    navree_kernel::Navree::app()
        .module(root)
        .boot()
        .await
        .expect_err("boot must fail")
}

/// Counts how often something happened (constructions, register calls, ...).
#[derive(Default, Debug)]
pub struct Counter(AtomicUsize);

impl Counter {
    pub fn hit(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// Records events in order (lifecycle hooks, command runs, ...).
#[derive(Default, Debug)]
pub struct Journal(Mutex<Vec<String>>);

impl Journal {
    pub fn push(&self, entry: impl Into<String>) {
        self.0.lock().unwrap().push(entry.into());
    }

    pub fn entries(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// Declares a struct whose fields are `Arc<Dep>` and implements `Injectable`
/// for it, declaring every field type as a dependency.
///
/// `injectable!(Service { repo: Repo })` or
/// `injectable!(Service [Request] { repo: Repo })` for a non-default scope.
macro_rules! injectable {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        injectable!($name [Singleton] { $($field : $ty),* });
    };
    ($name:ident [$scope:ident] { $($field:ident : $ty:ty),* $(,)? }) => {
        #[allow(dead_code)]
        pub struct $name {
            $(pub $field: ::std::sync::Arc<$ty>,)*
        }

        impl ::navree_kernel::Injectable for $name {
            fn scope() -> ::navree_kernel::Scope {
                ::navree_kernel::Scope::$scope
            }

            fn dependencies() -> Vec<::navree_kernel::ProviderKey> {
                vec![$(::navree_kernel::ProviderKey::of::<$ty>()),*]
            }

            #[allow(unused_variables)]
            fn construct(
                r: ::navree_kernel::Resolver,
            ) -> ::navree_kernel::BoxFuture<'static, Result<Self, ::navree_kernel::KernelError>> {
                Box::pin(async move {
                    Ok(Self {
                        $($field: r.get::<$ty>().await?,)*
                    })
                })
            }
        }
    };
}

/// Declares a unit-struct module whose `register` runs the given body.
///
/// `module!(UsersModule, |m| { m.provider::<UserService>(); });`
macro_rules! module {
    ($name:ident, |$m:ident| $body:block) => {
        pub struct $name;

        impl ::navree_kernel::Module for $name {
            fn register(&self, $m: &mut ::navree_kernel::ModuleBuilder) $body
        }
    };
}

pub(crate) use {injectable, module};
