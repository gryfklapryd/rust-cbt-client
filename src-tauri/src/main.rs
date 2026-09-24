// Sembunyikan jendela konsol di Windows (build release).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    cbt_client_lib::run()
}
