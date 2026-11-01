//! 보안 메뉴에서 쓰는 순수 로직 — 방화벽(ufw) 출력 해석, 서버 점검 결과 해석, 인증서 만료일 계산
//!
//! 외부 명령 실행은 routes/security.rs 가 rpanel-helper 로 하고, 여기서는 그 출력만 다룹니다.

use std::collections::HashMap;

/// 방화벽 규칙 한 줄 (`ufw status numbered`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FwRule {
    /// ufw 규칙 번호 (지울 때 씀)
    pub num: u32,
    /// 대상 — "22/tcp", "3000:3010/tcp", "Nginx Full", "22 on eth0" …
    pub to: String,
    /// "ALLOW IN" · "DENY IN" · "LIMIT IN" · "REJECT IN" …
    pub action: String,
    /// 출발지 — "Anywhere", "203.0.113.0/24" …
    pub from: String,
    pub v6: bool,
    pub comment: Option<String>,
}

impl FwRule {
    /// 대상의 포트 범위 (첫 포트, 끝 포트). 앱 프로필 이름 등 포트가 아니면 None
    pub fn ports(&self) -> Option<(u16, u16)> {
        let head = self.to.split_whitespace().next()?;
        let spec = head.split('/').next()?;
        match spec.split_once(':') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => {
                // "22,80,443/tcp" 처럼 여러 포트: 가장 작은~큰 값
                let ports: Vec<u16> = spec
                    .split(',')
                    .map(|p| p.parse().ok())
                    .collect::<Option<_>>()?;
                Some((*ports.iter().min()?, *ports.iter().max()?))
            }
        }
    }

    pub fn covers(&self, port: u16) -> bool {
        match self.ports() {
            Some((a, b)) => a <= port && port <= b,
            None => {
                // 앱 프로필: 이름으로 대략 판단
                let t = self.to.to_ascii_lowercase();
                (port == 22 && t.contains("openssh"))
                    || ((port == 80 || port == 443) && (t.contains("nginx") || t.contains("www")))
            }
        }
    }

    pub fn is_allow(&self) -> bool {
        self.action.starts_with("ALLOW") || self.action.starts_with("LIMIT")
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct FwStatus {
    /// ufw 가 없으면 false
    pub installed: bool,
    pub active: bool,
    pub rules: Vec<FwRule>,
    /// 꺼져 있을 때: 켜면 적용될 규칙 ("ufw allow 22/tcp" …)
    pub added: Vec<String>,
}

/// `ufw status numbered` 출력 해석
pub fn parse_ufw(out: &str) -> FwStatus {
    let mut st = FwStatus::default();
    for line in out.lines() {
        let l = line.trim();
        if let Some(s) = l.strip_prefix("Status:") {
            // helper 는 ufw 가 없으면 "Status: missing" 을 줌
            st.installed = s.trim() != "missing";
            st.active = s.trim() == "active";
            continue;
        }
        if let Some(r) = l.strip_prefix("added=") {
            // 꺼져 있을 때 helper 가 주는 '켜면 적용될 규칙' (ufw show added)
            st.added.push(r.to_string());
            continue;
        }
        let Some(rest) = l.strip_prefix('[') else {
            continue;
        };
        let Some((num, rest)) = rest.split_once(']') else {
            continue;
        };
        let Ok(num) = num.trim().parse::<u32>() else {
            continue;
        };
        // 열은 공백 두 칸 이상으로 나뉨: 대상 | 동작 | 출발지 [# 메모]
        let (body, comment) = match rest.split_once(" # ") {
            Some((b, c)) => (b, Some(c.trim().to_string())),
            None => (rest, None),
        };
        let cols: Vec<&str> = body
            .split("  ")
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        if cols.len() < 3 {
            continue;
        }
        let to = cols[0].to_string();
        let action = cols[1].to_string();
        let from = cols[2..].join(" ");
        let v6 = to.contains("(v6)") || from.contains("(v6)");
        st.rules.push(FwRule {
            num,
            to: to.replace(" (v6)", ""),
            action,
            from: from.replace(" (v6)", ""),
            v6,
            comment,
        });
    }
    st
}

/// 열기·닫기 요청의 포트: "3000" 또는 "3000:3010" (1~65535)
pub fn valid_port_spec(s: &str) -> Option<(u16, u16)> {
    let s = s.trim();
    let parse = |p: &str| -> Option<u16> {
        if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        p.parse::<u16>().ok().filter(|v| *v > 0)
    };
    match s.split_once(':') {
        Some((a, b)) => {
            let (a, b) = (parse(a)?, parse(b)?);
            (a < b).then_some((a, b))
        }
        None => parse(s).map(|p| (p, p)),
    }
}

/// `rpanel-helper security-scan` 의 key=value 출력
pub fn parse_kv(out: &str) -> HashMap<String, String> {
    out.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

/// 1970-01-01 부터의 날짜 수 (그레고리력)
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// openssl 의 notAfter("Jan 13 00:00:00 2027 GMT")를 유닉스 시각(초)으로
pub fn parse_cert_time(s: &str) -> Option<i64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let m = MONTHS.iter().position(|x| *x == parts[0])? as u32 + 1;
    let d: u32 = parts[1].parse().ok().filter(|d| (1..=31).contains(d))?;
    let hms: Vec<i64> = parts[2]
        .split(':')
        .map(|x| x.parse().ok())
        .collect::<Option<_>>()?;
    if hms.len() != 3 {
        return None;
    }
    let y: i64 = parts[3].parse().ok()?;
    Some(days_from_civil(y, m, d) * 86_400 + hms[0] * 3600 + hms[1] * 60 + hms[2])
}

/// 인증서가 만료되기까지 남은 날 (지났으면 음수)
pub fn cert_days_left(not_after: &str, now_unix: i64) -> Option<i64> {
    let t = parse_cert_time(not_after)?;
    Some((t - now_unix).div_euclid(86_400))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UFW: &str = "Status: active

     To                         Action      From
     --                         ------      ----
[ 1] 22/tcp                     ALLOW IN    Anywhere
[ 2] 8443/tcp                   ALLOW IN    203.0.113.0/24
[ 3] 80/tcp                     ALLOW IN    Anywhere                   # web
[ 4] 3000:3010/tcp              DENY IN     Anywhere
[ 5] Anywhere                   DENY IN     198.51.100.7
[ 6] Nginx Full                 ALLOW IN    Anywhere
[ 7] 22 on eth0                 ALLOW IN    Anywhere
[ 8] 22/tcp (v6)                ALLOW IN    Anywhere (v6)
[10] 5432,6379/tcp              LIMIT IN    Anywhere
";

    #[test]
    fn ufw_numbered() {
        let s = parse_ufw(UFW);
        assert!(s.installed && s.active);
        assert_eq!(s.rules.len(), 9);
        assert_eq!(
            s.rules[0],
            FwRule {
                num: 1,
                to: "22/tcp".into(),
                action: "ALLOW IN".into(),
                from: "Anywhere".into(),
                v6: false,
                comment: None
            }
        );
        assert_eq!(s.rules[1].from, "203.0.113.0/24");
        assert_eq!(s.rules[2].comment.as_deref(), Some("web"));
        assert_eq!(s.rules[3].ports(), Some((3000, 3010)));
        assert!(!s.rules[3].is_allow());
        assert_eq!(s.rules[4].to, "Anywhere");
        assert_eq!(s.rules[4].from, "198.51.100.7");
        assert_eq!(s.rules[4].ports(), None);
        assert!(s.rules[5].covers(443) && !s.rules[5].covers(22));
        assert_eq!(s.rules[6].ports(), Some((22, 22)));
        assert!(s.rules[7].v6 && s.rules[7].to == "22/tcp" && s.rules[7].from == "Anywhere");
        assert_eq!(s.rules[8].num, 10);
        assert_eq!(s.rules[8].ports(), Some((5432, 6379)));
        assert!(s.rules[8].is_allow());
    }

    #[test]
    fn ufw_inactive_and_missing() {
        let s = parse_ufw(
            "ssh_port=22\nStatus: inactive\nadded=ufw allow 22/tcp\nadded=ufw allow 8443/tcp\n",
        );
        assert!(s.installed && !s.active && s.rules.is_empty());
        assert_eq!(s.added, vec!["ufw allow 22/tcp", "ufw allow 8443/tcp"]);
        let m = parse_ufw("Status: missing\n");
        assert!(!m.installed && !m.active);
        assert_eq!(parse_ufw(""), FwStatus::default());
    }

    #[test]
    fn port_spec() {
        assert_eq!(valid_port_spec("3000"), Some((3000, 3000)));
        assert_eq!(valid_port_spec(" 3000:3010 "), Some((3000, 3010)));
        assert_eq!(valid_port_spec("0"), None);
        assert_eq!(valid_port_spec("65536"), None);
        assert_eq!(valid_port_spec("3010:3000"), None);
        assert_eq!(valid_port_spec("22/tcp"), None);
        assert_eq!(valid_port_spec("1;rm"), None);
        assert_eq!(valid_port_spec(""), None);
    }

    #[test]
    fn kv() {
        let m = parse_kv("ufw=active\nssh_port=2222\nbad line\nssh_root = no\n");
        assert_eq!(m["ufw"], "active");
        assert_eq!(m["ssh_port"], "2222");
        assert_eq!(m["ssh_root"], "no");
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn cert_time() {
        // 2027-01-13T00:00:00Z = 1799798400
        assert_eq!(
            parse_cert_time("Jan 13 00:00:00 2027 GMT"),
            Some(1_799_798_400)
        );
        // openssl 은 한 자리 날짜를 공백 두 칸으로 씀
        assert_eq!(
            parse_cert_time("Mar  3 12:30:05 2026 GMT"),
            Some(1_772_541_005)
        );
        assert_eq!(
            parse_cert_time("Feb 29 00:00:00 2028 GMT"),
            Some(1_835_395_200)
        );
        assert_eq!(parse_cert_time("Foo 1 00:00:00 2027 GMT"), None);
        assert_eq!(parse_cert_time(""), None);
        let now = 1_799_798_400 - 10 * 86_400 - 5;
        assert_eq!(cert_days_left("Jan 13 00:00:00 2027 GMT", now), Some(10));
        assert_eq!(
            cert_days_left("Jan 13 00:00:00 2027 GMT", 1_799_798_400 + 1),
            Some(-1)
        );
    }
}
