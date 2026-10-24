//! 입력 검증. 여기서 통과한 값만 셸 인자·Nginx 설정·systemd 유닛에 들어갑니다.
//! (명령은 항상 인자 배열로 실행하고 셸 문자열 보간을 하지 않지만, 설정 파일 주입을 막기 위해
//!  모든 값을 화이트리스트로 검증합니다.)

use once_cell::sync::Lazy;
use regex::Regex;

static SLUG: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{0,39}$").unwrap());
static DOMAIN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$").unwrap());
static ENV_KEY: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]{0,127}$").unwrap());
static BRANCH: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._/-]{0,99}$").unwrap());
static BIN_NAME: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9_][A-Za-z0-9_.-]{0,99}$").unwrap());
// https://host/path(.git) | http://... | ssh://git@host:port/path | git@host:path
static GIT_URL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(?:(?:https?|ssh)://(?:[A-Za-z0-9._-]+@)?[A-Za-z0-9.-]+(?::\d{1,5})?/[A-Za-z0-9._/~-]+|[A-Za-z0-9._-]+@[A-Za-z0-9.-]+:[A-Za-z0-9._/~-]+)$",
    )
    .unwrap()
});

pub fn slug(s: &str) -> Result<(), String> {
    if SLUG.is_match(s) {
        Ok(())
    } else {
        Err("slug는 소문자·숫자·하이픈 1~40자이며 문자/숫자로 시작해야 합니다".into())
    }
}

/// 이름 → slug 후보 (사용자가 slug를 비워둔 경우)
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    out.chars()
        .take(40)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

pub fn domain(s: &str) -> Result<(), String> {
    if s.len() <= 253 && DOMAIN.is_match(s) {
        Ok(())
    } else {
        Err(format!("올바르지 않은 도메인: {s}"))
    }
}

/// 공백/쉼표 구분 도메인 목록 → 정규화된 공백 구분 문자열
pub fn domains(input: &str) -> Result<String, String> {
    let mut list: Vec<String> = Vec::new();
    for d in input
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
    {
        let d = d.trim().trim_end_matches('.').to_ascii_lowercase();
        domain(&d)?;
        if !list.contains(&d) {
            list.push(d);
        }
    }
    if list.len() > 20 {
        return Err("도메인은 최대 20개까지 등록할 수 있습니다".into());
    }
    Ok(list.join(" "))
}

pub fn env_key(s: &str) -> Result<(), String> {
    if ENV_KEY.is_match(s) {
        Ok(())
    } else {
        Err(format!("환경변수 이름이 올바르지 않습니다: {s}"))
    }
}

pub fn env_value(s: &str) -> Result<(), String> {
    if s.contains('\n') || s.contains('\r') || s.contains('\0') {
        Err("환경변수 값에 줄바꿈/NUL 문자를 넣을 수 없습니다".into())
    } else if s.len() > 8192 {
        Err("환경변수 값은 8KB 이하여야 합니다".into())
    } else {
        Ok(())
    }
}

pub fn git_url(s: &str) -> Result<(), String> {
    if s.len() <= 500 && GIT_URL.is_match(s) && !s.contains("..") {
        Ok(())
    } else {
        Err("Git URL 형식이 올바르지 않습니다 (https://…, ssh://git@host:port/group/repo.git, git@host:group/repo.git)".into())
    }
}

pub fn branch(s: &str) -> Result<(), String> {
    if BRANCH.is_match(s) && !s.contains("..") {
        Ok(())
    } else {
        Err("브랜치 이름이 올바르지 않습니다".into())
    }
}

pub fn binary_name(s: &str) -> Result<(), String> {
    if BIN_NAME.is_match(s) {
        Ok(())
    } else {
        Err("바이너리 이름이 올바르지 않습니다".into())
    }
}

/// 저장소 안의 상대 경로 (root_dir, output_dir). 빈 문자열 허용.
pub fn rel_path(s: &str) -> Result<(), String> {
    if s.is_empty() {
        return Ok(());
    }
    let ok = !s.starts_with('/')
        && s.len() <= 200
        && s.split('/').all(|seg| {
            !seg.is_empty()
                && seg != ".."
                && seg != "."
                && seg
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        });
    if ok {
        Ok(())
    } else {
        Err(format!("상대 경로가 올바르지 않습니다: {s}"))
    }
}

/// URL 경로 (health_path)
pub fn url_path(s: &str) -> Result<(), String> {
    if s.starts_with('/')
        && s.len() <= 200
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-~?=&".contains(c))
    {
        Ok(())
    } else {
        Err("헬스체크 경로는 /로 시작하는 URL 경로여야 합니다".into())
    }
}

/// 빌드/시작 명령. `bash -lc`로 실행되므로 줄바꿈만 금지하고 길이 제한.
pub fn command(s: &str) -> Result<(), String> {
    if s.contains('\n') || s.contains('\r') || s.contains('\0') {
        Err("명령에 줄바꿈을 넣을 수 없습니다 (&&로 연결하세요)".into())
    } else if s.len() > 500 {
        Err("명령은 500자 이하여야 합니다".into())
    } else {
        Ok(())
    }
}

pub fn name(s: &str) -> Result<(), String> {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > 100 || t.chars().any(|c| c.is_control()) {
        Err("이름은 1~100자여야 합니다".into())
    } else {
        Ok(())
    }
}

pub fn email(s: &str) -> Result<(), String> {
    let ok = s.len() <= 190
        && {
            let mut parts = s.split('@');
            matches!((parts.next(), parts.next(), parts.next()), (Some(a), Some(b), None) if !a.is_empty() && b.contains('.') && !b.starts_with('.') && !b.ends_with('.'))
        }
        && !s.chars().any(char::is_whitespace);
    if ok {
        Ok(())
    } else {
        Err("이메일 형식이 올바르지 않습니다".into())
    }
}

/// Content-Security-Policy 정책. Nginx `add_header ... "<정책>" always;` 의 큰따옴표 안에 들어가므로
/// 따옴표·`$`·`\`·중괄호·줄바꿈 등을 막고 CSP 에 쓰는 문자만 받습니다 (rpanel-helper 와 같은 규칙).
/// 줄바꿈·연속 공백은 공백 하나로 정리한 값을 돌려줍니다.
pub fn csp(input: &str) -> Result<String, String> {
    let s = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        return Err("CSP 정책을 입력하세요".into());
    }
    if s.len() > 2000 {
        return Err("CSP 정책은 2000자 이하여야 합니다".into());
    }
    if let Some(c) = s
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || " :;/.*'_+=,-".contains(*c)))
    {
        return Err(format!("CSP 정책에 쓸 수 없는 문자가 있습니다: {c}"));
    }
    for d in s.split(';').map(str::trim).filter(|d| !d.is_empty()) {
        let name = d.split(' ').next().unwrap_or("");
        if name.is_empty()
            || !name.starts_with(|c: char| c.is_ascii_lowercase())
            || !name.chars().all(|c| c.is_ascii_lowercase() || c == '-')
        {
            return Err(format!("CSP 지시어 이름이 올바르지 않습니다: {name}"));
        }
    }
    Ok(s)
}

pub fn password(s: &str) -> Result<(), String> {
    if s.chars().count() < 10 {
        Err("비밀번호는 10자 이상이어야 합니다".into())
    } else if s.len() > 256 {
        Err("비밀번호가 너무 깁니다".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_policies() {
        assert_eq!(
            csp("default-src 'self';\n  img-src 'self' https: data: blob:; frame-ancestors 'none'")
                .unwrap(),
            "default-src 'self'; img-src 'self' https: data: blob:; frame-ancestors 'none'"
        );
        assert!(csp("script-src 'self' 'sha256-abc+/=' https://cdn.jsdelivr.net").is_ok());
        assert!(csp("  ").is_err());
        assert!(csp(&format!("default-src {}", "a".repeat(2000))).is_err());
        // Nginx 설정 주입
        assert!(csp("default-src 'self'\" always; include /etc/passwd; #").is_err());
        assert!(csp("default-src $host").is_err());
        assert!(csp("default-src 'self'; } server {").is_err());
        assert!(csp("default-src \\x").is_err());
        // 지시어 이름
        assert!(csp("Default-Src 'self'").is_err());
        assert!(csp("'self' default-src").is_err());
    }

    #[test]
    fn slugs() {
        assert!(slug("beripick-api").is_ok());
        assert!(slug("a").is_ok());
        assert!(slug("-bad").is_err());
        assert!(slug("Bad").is_err());
        assert!(slug("has space").is_err());
        assert!(slug("../etc").is_err());
        assert!(slug(&"a".repeat(41)).is_err());
        assert_eq!(slugify("Beripick API v2!"), "beripick-api-v2");
        assert_eq!(slugify("  --hello--  "), "hello");
        assert_eq!(slugify("한글만"), "");
    }

    #[test]
    fn domain_lists() {
        assert_eq!(
            domains("Beripick.com, www.beripick.com").unwrap(),
            "beripick.com www.beripick.com"
        );
        assert_eq!(domains("a.com a.com").unwrap(), "a.com");
        assert_eq!(domains("").unwrap(), "");
        assert!(domains("bad_domain.com").is_err());
        assert!(domains("evil.com;").is_err());
        assert!(domains("x.com\nserver_name y").is_err());
        assert!(domains("localhost").is_err());
    }

    #[test]
    fn git_urls() {
        assert!(git_url("https://github.com/sinji0117/rpanel.git").is_ok());
        assert!(git_url("ssh://git@100.100.199.123:2222/group/repo.git").is_ok());
        assert!(git_url("git@gitlab-internal:group/repo.git").is_ok());
        assert!(git_url("http://100.100.199.123/group/repo.git").is_ok());
        assert!(git_url("file:///etc/passwd").is_err());
        assert!(git_url("--upload-pack=touch /tmp/x").is_err());
        assert!(git_url("https://host/a b").is_err());
        assert!(git_url("ext::sh -c id").is_err());
    }

    #[test]
    fn env() {
        assert!(env_key("DATABASE_URL").is_ok());
        assert!(env_key("1BAD").is_err());
        assert!(env_key("BAD-KEY").is_err());
        assert!(env_value("mysql://u:p@127.0.0.1/db").is_ok());
        assert!(env_value("a\nb").is_err());
    }

    #[test]
    fn paths() {
        assert!(rel_path("").is_ok());
        assert!(rel_path("frontend").is_ok());
        assert!(rel_path("apps/web").is_ok());
        assert!(rel_path("../x").is_err());
        assert!(rel_path("/etc").is_err());
        assert!(rel_path("a//b").is_err());
        assert!(url_path("/api/health").is_ok());
        assert!(url_path("health").is_err());
        assert!(url_path("/a b").is_err());
    }

    #[test]
    fn misc() {
        assert!(branch("main").is_ok());
        assert!(branch("feature/x-1").is_ok());
        assert!(branch("-x").is_err());
        assert!(command("npm ci && npm run build").is_ok());
        assert!(command("a\nb").is_err());
        assert!(email("a@b.com").is_ok());
        assert!(email("bad").is_err());
        assert!(email("a@b").is_err());
        assert!(password("short").is_err());
        assert!(password("long-enough-pw").is_ok());
        assert!(binary_name("hello-api").is_ok());
        assert!(binary_name("../x").is_err());
    }
}
