// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::var_os("GTK_THEME").is_none() {
        std::env::set_var("GTK_THEME", "Adwaita");
    }

    sessionsmith_desktop_lib::run()
}
