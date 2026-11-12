//! The examples, one module per group. Each example is a function whose body is
//! the code the docs show, so what a reader copies is what drew the preview.

use crate::catalog::Example;

mod actions;
mod activity;
mod board;
mod charts;
mod chat;
mod data;
mod dates;
mod display;
mod feedback;
mod flow;
mod grids;
mod icons;
mod inbox;
mod inputs;
mod layout;
mod linear;
mod markdown;
mod marketing;
mod menu_bars;
mod menus;
mod navigation;
mod otp_carousel;
mod overlays;
mod page_layouts;
mod panes;
mod parts;
mod patterns;
mod phone;
mod pickers;
mod radial;
mod sortable;
mod states;
mod store;
mod themes;
mod upload;

/// Every example, in the order the docs list them.
pub fn all() -> Vec<Example> {
    let mut all = Vec::new();
    all.extend(actions::all());
    all.extend(inputs::all());
    all.extend(upload::all());
    all.extend(otp_carousel::all());
    all.extend(parts::all());
    all.extend(pickers::all());
    all.extend(dates::all());
    all.extend(phone::all());
    all.extend(display::all());
    all.extend(markdown::all());
    all.extend(activity::all());
    all.extend(feedback::all());
    all.extend(states::all());
    all.extend(overlays::all());
    all.extend(layout::all());
    all.extend(page_layouts::all());
    all.extend(panes::all());
    all.extend(navigation::all());
    all.extend(menus::all());
    all.extend(menu_bars::all());
    all.extend(flow::all());
    all.extend(data::all());
    all.extend(sortable::all());
    all.extend(charts::all());
    all.extend(radial::all());
    all.extend(linear::all());
    all.extend(grids::all());
    all.extend(chat::all());
    all.extend(patterns::all());
    all.extend(themes::all());
    all.extend(icons::all());
    all.extend(store::all());
    all.extend(inbox::all());
    all.extend(board::all());
    all.extend(marketing::all());
    all
}
