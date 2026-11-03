//! The examples, one module per group. Each example is a function whose body is
//! the code the docs show, so what a reader copies is what drew the preview.

use crate::catalog::Example;

mod actions;
mod activity;
mod data;
mod dates;
mod display;
mod feedback;
mod icons;
mod inputs;
mod layout;
mod menus;
mod navigation;
mod overlays;
mod patterns;
mod phone;
mod pickers;

/// Every example, in the order the docs list them.
pub fn all() -> Vec<Example> {
    let mut all = Vec::new();
    all.extend(actions::all());
    all.extend(inputs::all());
    all.extend(pickers::all());
    all.extend(dates::all());
    all.extend(phone::all());
    all.extend(display::all());
    all.extend(activity::all());
    all.extend(feedback::all());
    all.extend(overlays::all());
    all.extend(layout::all());
    all.extend(navigation::all());
    all.extend(menus::all());
    all.extend(data::all());
    all.extend(patterns::all());
    all.extend(icons::all());
    all
}
