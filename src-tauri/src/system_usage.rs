//! Live system-wide CPU/RAM usage, shown in the footer. Polled by the
//! frontend rather than pushed, so there's no background timer running when
//! no window is open to look at it.

use std::sync::Mutex;

use serde::Serialize;
use sysinfo::System;

pub struct SystemUsageState(Mutex<System>);

impl SystemUsageState {
    pub fn new() -> Self {
        Self(Mutex::new(System::new_all()))
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemUsage {
    pub cpu_percent: f32,
    pub ram_percent: f32,
}

#[tauri::command]
pub fn get_system_usage(state: tauri::State<SystemUsageState>) -> SystemUsage {
    let mut sys = state.0.lock().unwrap_or_else(|e| e.into_inner());
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let cpu_percent = sys.global_cpu_usage();
    let ram_percent = if sys.total_memory() > 0 {
        (sys.used_memory() as f32 / sys.total_memory() as f32) * 100.0
    } else {
        0.0
    };

    SystemUsage {
        cpu_percent,
        ram_percent,
    }
}
