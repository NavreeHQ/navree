use navis_macros::module;

#[module(guards = [AdminGuard])]
struct AdminModule;

fn main() {}
