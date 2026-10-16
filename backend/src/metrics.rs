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

/// 그래프용으로 저장하는 작은 지표 (1분마다 한 개)
#[derive(Debug, Serialize, serde::Deserialize, Clone, PartialEq)]
pub struct Sample {
    /// 초 단위 시각
    pub t: i64,
    /// CPU %
    pub c: f32,
    /// 메모리 사용 바이트
    pub m: u64,
    /// 메모리 전체 바이트
    pub mt: u64,
    /// load average 1·5·15분
    pub l: [f64; 3],
    /// 디스크 [마운트, 사용, 전체]
    pub d: Vec<(String, u64, u64)>,
}

impl From<&Snapshot> for Sample {
    fn from(s: &Snapshot) -> Self {
        let r2 = |v: f64| (v * 100.0).round() / 100.0;
        Self {
            t: s.ts / 1000,
            c: (s.cpu_percent * 10.0).round() / 10.0,
            m: s.mem_used,
            mt: s.mem_total,
            l: [r2(s.load[0]), r2(s.load[1]), r2(s.load[2])],
            d: s.disks
                .iter()
                .map(|d| (d.mount.clone(), d.used, d.total))
                .collect(),
        }
    }
}

/// 보관 기간 7일 (1분 간격)
pub const HISTORY_MAX: isize = 7 * 24 * 60;
pub const SAMPLE_SECS: u64 = 60;

/// range 문자열 → 초
pub fn range_secs(range: &str) -> Option<i64> {
    Some(match range {
        "30m" => 30 * 60,
        "1h" => 3600,
        "6h" => 6 * 3600,
        "24h" => 24 * 3600,
        "7d" => 7 * 24 * 3600,
        _ => return None,
    })
}

/// 기간 안의 표본을 최대 `max_points` 개로 평균 내어 줄임 (시간순)
pub fn downsample(mut samples: Vec<Sample>, since: i64, max_points: usize) -> Vec<Sample> {
    samples.retain(|s| s.t >= since);
    samples.sort_by_key(|s| s.t);
    if samples.len() <= max_points || max_points == 0 {
        return samples;
    }
    let per = samples.len().div_ceil(max_points);
    samples
        .chunks(per)
        .map(|ch| {
            let n = ch.len() as f64;
            let last = ch.last().unwrap();
            let avg_l =
                |i: usize| ((ch.iter().map(|s| s.l[i]).sum::<f64>() / n) * 100.0).round() / 100.0;
            Sample {
                t: last.t,
                c: ((ch.iter().map(|s| s.c as f64).sum::<f64>() / n) * 10.0).round() as f32 / 10.0,
                m: (ch.iter().map(|s| s.m as f64).sum::<f64>() / n) as u64,
                mt: last.mt,
                l: [avg_l(0), avg_l(1), avg_l(2)],
                d: last.d.clone(),
            }
        })
        .collect()
}

/// 부팅 시각 (초). sysinfo 가 0 을 주면 /proc/uptime 으로 계산
pub fn boot_time() -> i64 {
    let b = System::boot_time() as i64;
    if b > 0 {
        return b;
    }
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .map(|up| chrono::Utc::now().timestamp() - up as i64)
        .unwrap_or(0)
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

#[cfg(test)]
mod history_tests {
    use super::*;

    fn s(t: i64, c: f32) -> Sample {
        Sample {
            t,
            c,
            m: 100,
            mt: 1000,
            l: [1.0, 0.5, 0.25],
            d: vec![("/".into(), 1, 2)],
        }
    }

    #[test]
    fn downsample_averages_and_filters() {
        let v: Vec<Sample> = (0..100).map(|i| s(1000 + i * 60, i as f32)).collect();
        let out = downsample(v.clone(), 1000 + 50 * 60, 1000);
        assert_eq!(out.len(), 50);
        assert_eq!(out[0].t, 1000 + 50 * 60);
        let out = downsample(v, 0, 10);
        assert_eq!(out.len(), 10);
        assert_eq!(out[0].c, 4.5);
        assert_eq!(out[0].t, 1000 + 9 * 60);
        assert!(out.windows(2).all(|w| w[0].t < w[1].t));
    }

    #[test]
    fn ranges() {
        assert_eq!(range_secs("30m"), Some(1800));
        assert_eq!(range_secs("7d"), Some(604800));
        assert_eq!(range_secs("1y"), None);
    }

    #[test]
    fn boot_time_is_past() {
        let b = boot_time();
        assert!(b > 0 && b <= chrono::Utc::now().timestamp());
    }
}
