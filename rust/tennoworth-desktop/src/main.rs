#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod overlay;
mod persistence;
mod services;
mod shell;

fn main() {
    shell::run();
}
