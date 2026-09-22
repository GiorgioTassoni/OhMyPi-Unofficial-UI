//! The shell's entry point. Everything else lives in the library so it can be
//! tested without a window.

// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("GTK_USE_PORTAL").is_none() {
            std::env::set_var("GTK_USE_PORTAL", "1");
        }
    }
    omp_desktop::run();
}
