//! The examples, one module per group. Each example is a function whose body is
//! the code the docs show, so what a reader copies is what drew the preview.

use crate::catalog::Example;

mod actions;
mod data;
mod display;
mod feedback;
mod inputs;
mod layout;
mod navigation;
mod overlays;
mod patterns;

/// Every example, in the order the docs list them.
pub fn all() -> Vec<Example> {
    let mut all = Vec::new();
    all.extend(actions::all());
    all.extend(inputs::all());
    all.extend(display::all());
    all.extend(feedback::all());
    all.extend(overlays::all());
    all.extend(layout::all());
    all.extend(navigation::all());
    all.extend(data::all());
    all.extend(patterns::all());
    all
}
