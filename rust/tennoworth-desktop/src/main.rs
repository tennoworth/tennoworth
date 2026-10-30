#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod contracts;
mod game_events;
mod overlay;
mod persistence;
mod poison;
mod services;
mod shell;
mod vision;

fn main() {
    shell::run(commands::auth::publish_access_changes);
}
