#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod game_events;
mod notification_contract;
mod overlay;
mod persistence;
mod services;
mod trading_contract;
mod vision;
mod shell;

fn main() {
    shell::run();
}
