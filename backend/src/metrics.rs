use serde::Serialize;
use sysinfo::{Disks, System};

#[derive(Debug, Serialize, Clone)]
pub struct DiskInfo {
    pub mount: String,
    pub total: u64,
    pub used: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct Snapshot {
    pub ts: i64,
    pub cpu_percent: f32,
    pub cpu_per_core: Vec<f32>,
    pub mem_total: u64,
    pub mem_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub load: [f64; 3],
    pub uptime: u64,
    pub disks: Vec<DiskInfo>,
}

#[derive(Debug, Serialize)]
pub struct HostInfo {
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub kernel: Option<String>,
    pub cpu_brand: String,
    pub cpu_count: usize,
}

pub struct Collector {
    sys: System,
    disks: Disks,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        Self {
            sys,
            disks: Disks::new_with_refreshed_list(),
        }
    }

    /// 두 번째 호출부터 CPU 사용률이 의미 있는 값이 됩니다 (구간 평균)
    pub fn snapshot(&mut self) -> Snapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.disks.refresh(true);
        let load = System::load_average();
        let mut disks: Vec<DiskInfo> = self
            .disks
            .list()
            .iter()
            .filter(|d| {
                let m = d.mount_point().to_string_lossy();
                !m.starts_with("/snap") && !m.starts_with("/boot") && d.total_space() > 0
            })
            .map(|d| DiskInfo {
                mount: d.mount_point().to_string_lossy().into_owned(),
                total: d.total_space(),
                used: d.total_space().saturating_sub(d.available_space()),
            })
            .collect();
        disks.sort_by(|a, b| a.mount.cmp(&b.mount));
        disks.dedup_by(|a, b| a.mount == b.mount);
        Snapshot {
            ts: chrono::Utc::now().timestamp_millis(),
            cpu_percent: self.sys.global_cpu_usage(),
            cpu_per_core: self.sys.cpus().iter().map(|c| c.cpu_usage()).collect(),
            mem_total: self.sys.total_memory(),
            mem_used: self.sys.used_memory(),
            swap_total: self.sys.total_swap(),
            swap_used: self.sys.used_swap(),
            load: [load.one, load.five, load.fifteen],
            uptime: System::uptime(),
            disks,
        }
    }
}

pub fn host_info() -> HostInfo {
    let mut sys = System::new();
    sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
    HostInfo {
        hostname: System::host_name(),
        os: System::long_os_version(),
        kernel: System::kernel_version(),
        cpu_brand: sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .unwrap_or_default(),
        cpu_count: sys.cpus().len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_has_sane_values() {
        let mut c = Collector::new();
        std::thread::sleep(std::time::Duration::from_millis(250));
        let s = c.snapshot();
        assert!(s.mem_total > 0);
        assert!(s.mem_used <= s.mem_total);
        assert!(s.cpu_percent >= 0.0 && s.cpu_percent <= 100.0 + f32::EPSILON);
        assert!(!s.cpu_per_core.is_empty());
        let h = host_info();
        assert!(h.cpu_count > 0);
    }
}
