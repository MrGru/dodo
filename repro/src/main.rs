//! Standalone macOS reproduction for the dodo tray-reopen IOSurface leak.
//!
//! Background: dodo grows ~65 MB of `IOSurface` (graphics surfaces) per *true*
//! tray close/reopen cycle — the cycles that destroy and recreate the native
//! window — while malloc stays nearly flat and the memory never reclaims. A
//! prior fix that drained the macOS window autorelease pool did NOT change the
//! staircase, so the owner that fails to release the surface has not been pinned.
//!
//! This binary drives exactly the shape that leaks, on the same GPUI backend
//! dodo ships, WITHOUT dodo: open a window, let it draw a frame, then *true*
//! close it via `Window::remove_window` (the same path dodo's red-close / Cmd-W
//! reach — NOT hide/minimize), in a loop of N cycles, all on the app run loop
//! (never on process exit).
//!
//! It depends on the vendored `../patches/gpui-pre-macos`, which carries the fix
//! (`MacWindow::drop` now removes the `GPUIView` from its superview so the view
//! deallocates and releases its `Arc<MacWindowState>` — freeing the renderer's
//! `CAMetalLayer` and IOSurfaces). This binary prints `[REPRO-SAMPLE]` footprint
//! rows after each open+draw and again after each true close.
//!
//! Expected with the fix: IOSurface returns to a small baseline after every true
//! close instead of climbing — no per-cycle staircase.
//!
//! To reproduce the ORIGINAL leak, revert the one `NSView::removeFromSuperview`
//! line in `../patches/gpui-pre-macos/src/window.rs`: IOSurface then staircases
//! (~7-8 MB per cycle for this tiny window) and never reclaims. The owner was
//! pinned with temporary `eprintln!` counters at `MacWindowState::drop`,
//! `dealloc_window` and `dealloc_view`: without the fix the view's `dealloc`
//! (and thus `MacWindowState::drop`) never fires, so the surfaces leak.
//!
//! Env knobs (all optional): `REPRO_CYCLES` (default 3), `REPRO_OPEN_WAIT` secs
//! (1.0), `REPRO_CLOSE_WAIT` secs (10.0), `REPRO_FINAL_IDLE` secs (0.0 — set to
//! 300 to check the 5-minute-idle plateau).

use std::time::Duration;

use gpui::{
    App, Bounds, Context, Window, WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px,
    rgb, size,
};
use gpui_platform::application;

/// Root view: two solid rectangles, no text — enough to make the Metal renderer
/// present a real drawable (and thus an IOSurface) without needing fonts loaded.
struct Frame;

impl Render for Frame {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0x1e2030))
            .child(div().size(px(240.0)).bg(rgb(0xa03040)))
    }
}

fn main() {
    let cycles = usize_env("REPRO_CYCLES", 3).max(1);
    let open_wait = secs_env("REPRO_OPEN_WAIT", 1.0);
    let close_wait = secs_env("REPRO_CLOSE_WAIT", 10.0);
    let final_idle = secs_env("REPRO_FINAL_IDLE", 0.0);

    let pid = std::process::id();
    eprintln!(
        "[REPRO] pid={pid} cycles={cycles} open_wait={open_wait:?} close_wait={close_wait:?} \
         final_idle={final_idle:?}"
    );
    eprintln!(
        "[REPRO] to sample from another shell:  footprint -p {pid} | \
         egrep -i 'IOSurface|IOAccelerator|Owned physical|MALLOC_(TINY|SMALL|LARGE)|physical footprint'"
    );

    application().run(move |cx: &mut App| {
        cx.activate(true);
        cx.spawn(async move |cx| {
            sample(pid, "baseline (no window yet)");

            for n in 1..=cycles {
                // --- open + draw a frame ---
                let handle: WindowHandle<Frame> = cx.update(|cx| {
                    let options = window_options(cx);
                    cx.open_window(options, |_, cx| cx.new(|_| Frame))
                        .expect("open_window failed")
                });
                let _ = cx.update(|cx| cx.activate(true));
                cx.background_executor().timer(open_wait).await;
                sample(pid, &format!("cycle {n}: after open + draw"));

                // --- TRUE close: same path as dodo's red-close / Cmd-W ---
                // `Window::remove_window` removes the window from the app map and
                // drops the platform `MacWindow`, reaching `MacWindow::drop`.
                cx.update(|cx| {
                    handle
                        .update(cx, |_, window, _| window.remove_window())
                        .ok();
                });
                cx.background_executor().timer(close_wait).await;
                sample(pid, &format!("cycle {n}: {close_wait:?} after true close"));
            }

            if final_idle > Duration::ZERO {
                cx.background_executor().timer(final_idle).await;
                sample(pid, &format!("final: {final_idle:?} idle after last close"));
            }

            eprintln!("[REPRO] done; quitting");
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

/// Fixed geometry so every cycle recreates the same-size window/renderer.
fn window_options(cx: &mut App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(600.0), px(400.0)), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
    }
}

/// Print the footprint category rows that matter for this leak. Shells out to
/// `footprint` (works for one's own pid without privileges); falls back to a
/// hint if it is unavailable.
fn sample(pid: u32, label: &str) {
    println!("\n===== [REPRO-SAMPLE] {label} =====");
    match std::process::Command::new("footprint")
        .arg("-p")
        .arg(pid.to_string())
        .output()
    {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut printed = false;
            for line in text.lines() {
                let l = line.to_ascii_lowercase();
                if l.contains("iosurface")
                    || l.contains("ioaccelerator")
                    || l.contains("owned physical")
                    || l.contains("physical footprint")
                    || l.contains("malloc_tiny")
                    || l.contains("malloc_small")
                    || l.contains("malloc_large")
                {
                    println!("{}", line.trim_end());
                    printed = true;
                }
            }
            if !printed {
                println!(
                    "(footprint returned no matching rows; stderr: {})",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
        }
        Err(e) => println!("(footprint unavailable: {e}; try:  vmmap --summary {pid})"),
    }
}

fn usize_env(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn secs_env(key: &str, default: f64) -> Duration {
    let secs = std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default);
    Duration::from_secs_f64(secs)
}
