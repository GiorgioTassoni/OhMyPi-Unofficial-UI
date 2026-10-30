//! The shell's entry point. Everything else lives in the library so it can be
//! tested without a window.

// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
fn sanitize_appimage_env() {
    // AppImage's standard AppRun binary blindly sets:
    //   PYTHONHOME=$APPDIR/usr/
    //   PYTHONPATH=$APPDIR/usr/share/pyshared/:...
    // simply because the directory $APPDIR/usr exists, even though Oh My Pi does not bundle Python.
    // When child processes (omp sidecar, bash, subagents, python3) inherit these variables,
    // Python searches for standard libraries in /tmp/.mount_.../usr/lib/... and immediately crashes:
    // "ModuleNotFoundError: No module named 'encodings'".
    let is_appimage =
        std::env::var_os("APPIMAGE").is_some() || std::env::var_os("APPDIR").is_some();
    for var in ["PYTHONHOME", "PYTHONPATH", "PERLLIB"] {
        if let Ok(val) = std::env::var(var) {
            if is_appimage || val.contains(".mount_") {
                std::env::remove_var(var);
            }
        }
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    sanitize_appimage_env();

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
