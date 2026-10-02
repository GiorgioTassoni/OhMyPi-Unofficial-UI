//! The shell's entry point. Everything else lives in the library so it can be
//! tested without a window.

// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let options = omp_desktop::launch_options(std::env::args().skip(1));
    #[cfg(target_os = "linux")]
    {
        // GDK selects its backend during initialization. An in-app setting would be too late;
        // X11 is opt-in and Wayland remains a fallback when XWayland is unavailable.
        if options.prefer_x11 {
            std::env::set_var("GDK_BACKEND", "x11,wayland");
        }
        if std::env::var_os("GTK_USE_PORTAL").is_none() {
            std::env::set_var("GTK_USE_PORTAL", "1");
        }
    }
    #[cfg(not(target_os = "linux"))]
    if options.prefer_x11 {
        eprintln!("[omp-desktop] --prefer-x11 only applies on Linux");
    }
    omp_desktop::run();
}
