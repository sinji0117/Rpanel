// 앱 종류 — 서비스(systemd, 포트)로 실행하는 언어
export const SERVICE_KINDS = ['rust', 'python', 'node', 'java']

export const isServiceKind = (kind) => SERVICE_KINDS.includes(kind)

export const KIND_LABEL = { rust: 'Rust', python: 'Python', node: 'Node.js', java: 'Java' }
