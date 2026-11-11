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
    // Before anything can spawn a thread: capability sets are per thread and
    // new threads inherit them, so this is the only point that covers all.
    game_acquisition::capability::restrict_ptrace_at_startup();
    shell::run(commands::auth::publish_access_changes);
}
