//! Everything OS-specific: global input hooks, accessibility queries, screen
//! capture, overlay window behavior and opening System Settings panes.
//!
//! The traits from docs/PLAN.md section 5.4 arrive in M1. Each OS implements
//! them in its own submodule; nothing outside `platform` uses OS APIs.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
