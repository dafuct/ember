// 🦀 `#![cfg_attr(...)]` is an *inner attribute* (the `!` means it applies to
//    the enclosing item — here the whole crate, not just the next item).
//    `cfg_attr(not(debug_assertions), windows_subsystem = "windows")` tells
//    the Windows linker to build a GUI subsystem binary in release mode, which
//    prevents a console window from appearing alongside the app.
//    On macOS/Linux this attribute is ignored entirely.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 🦀 `ember_lib::run()` calls the `run` function from our library crate
    //    (`lib.rs`).  Separating the app logic into a `lib` crate (rather than
    //    putting everything in `main.rs`) lets the same code be tested and
    //    reused, since library crates are more flexible than binary crates.
    ember_lib::run();
}
