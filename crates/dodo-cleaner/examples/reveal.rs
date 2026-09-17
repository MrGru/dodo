//! Isolation probe for "Reveal in Finder does nothing" (macOS only).
//!
//! `cargo run -p dodo-cleaner --example reveal --locked -- <path>`
//!
//! Calls [`dodo_cleaner::reveal_in_finder`] directly on the path given as an
//! argument, outside dodo's GPUI event loop. If a Finder window comes forward
//! with the item selected here, the reveal call itself is sound and any
//! remaining app-side failure would be runtime context; if nothing appears
//! here either, the reveal command is the problem. It prints the same
//! `Result` the app acts on so a silent failure is visible on the terminal.
//!
//! Compiles on every platform (`--all-targets` cross-checks) but only does
//! anything on macOS, where the reveal function exists.

#[cfg(target_os = "macos")]
fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: cargo run -p dodo-cleaner --example reveal --locked -- <path>");
        std::process::exit(2);
    });

    match dodo_cleaner::reveal_in_finder(std::path::Path::new(&path)) {
        Ok(()) => println!("reveal_in_finder({path}) -> Ok (Finder should now show it)"),
        Err(error) => {
            eprintln!("reveal_in_finder({path}) -> Err: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("the reveal probe is macOS-only");
    std::process::exit(2);
}
