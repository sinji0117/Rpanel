// 마이그레이션(.sql)은 sqlx::migrate! 로 실행 파일 안에 들어갑니다.
// 새 마이그레이션 파일만 추가돼도 다시 빌드되도록 폴더 변경을 감시합니다.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
