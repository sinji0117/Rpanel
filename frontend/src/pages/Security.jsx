import { useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { confirmDialog } from '../components/Dialog'
import { CopyButton, ErrorBox, Field, Loading, Modal, PageHeader, RefreshButton, Section, Tabs } from '../components/ui'
import { useAuth } from '../context/AuthContext'
import { useToast } from '../context/ToastContext'
import { t } from '../i18n'
import { del, get, post, put } from '../lib/api'
import { ago, dateTime } from '../lib/format'
import { qrPath } from '../lib/qr'
import { useFetch } from '../lib/useFetch'
import { PanelAccess } from './Settings'

const TABS = [
  { id: 'check', label: '보안 점검' },
  { id: 'firewall', label: '방화벽' },
  { id: 'logins', label: '로그인 보안' },
  { id: '2fa', label: '2단계 인증' },
]

const LEVEL = {
  ok: { icon: '✔', cls: 'text-signal border-signal/30 bg-signal/5', label: '안전' },
  info: { icon: 'i', cls: 'text-sky-300 border-sky-400/30 bg-sky-400/5', label: '참고' },
  warn: { icon: '!', cls: 'text-warn border-warn/30 bg-warn/5', label: '주의' },
  fail: { icon: '✕', cls: 'text-fail border-fail/40 bg-fail/10', label: '위험' },
}

// ───────────────────────── 보안 점검 ─────────────────────────

/** 점검 항목 → 제목 · 설명 · 고칠 곳 */
function describe(it) {
  const d = it.data ?? {}
  const ok = it.level === 'ok'
  switch (it.id) {
    case 'firewall':
      return {
        title: t('방화벽 (ufw)'),
        desc: ok ? t('켜져 있습니다. 허용한 포트로만 들어올 수 있습니다.') : d.state === 'missing' ? t('ufw 가 설치되어 있지 않습니다.') : t('꺼져 있습니다. 서버의 모든 포트가 열려 있을 수 있습니다.'),
        tab: 'firewall',
      }
    case 'ssh_root':
      return {
        title: t('SSH root 로그인'),
        desc: it.level === 'warn' ? t('root 계정에 비밀번호로 로그인할 수 있습니다. sshd_config 에 PermitRootLogin prohibit-password 를 권장합니다.') : ok ? t('root 비밀번호 로그인이 막혀 있습니다.') : t('확인하지 못했습니다.'),
      }
    case 'ssh_password':
      return {
        title: t('SSH 비밀번호 로그인'),
        desc: d.value === 'no' ? t('SSH 키로만 로그인할 수 있습니다.') : d.value === 'yes' ? t('비밀번호로 로그인할 수 있습니다. SSH 키로 바꾸고 PasswordAuthentication no 를 권장합니다.') : t('확인하지 못했습니다.'),
      }
    case 'fail2ban':
      return {
        title: t('fail2ban (SSH 무차별 대입 차단)'),
        desc: ok ? t('동작 중입니다.') : t('없거나 꺼져 있습니다. 설치: sudo apt install fail2ban'),
      }
    case 'ssh_port':
      return {
        title: t('SSH 포트'),
        desc: d.port === '22' ? t('기본 포트(22)라 자동 공격이 많습니다. SSH 키 로그인과 fail2ban 이면 충분합니다.') : t('포트 {port}', { port: d.port }),
      }
    case 'auto_updates':
      return {
        title: t('자동 보안 업데이트'),
        desc: ok ? t('켜져 있습니다.') : t('꺼져 있습니다. sudo apt install unattended-upgrades 후 sudo dpkg-reconfigure -plow unattended-upgrades'),
      }
    case 'panel_access':
      return {
        title: t('패널 접속 IP 제한'),
        desc: ok ? t('허용한 IP 에서만 패널을 열 수 있습니다.') : t('어디서나 패널 로그인 화면을 열 수 있습니다. 내 IP({ip})만 허용하는 것을 권장합니다.', { ip: d.my_ip }),
        tab: 'logins',
      }
    case 'two_factor':
      return {
        title: t('관리자 2단계 인증'),
        desc: ok
          ? t('모든 관리자({total}명)가 켰습니다.', { total: d.total })
          : `${t('관리자 {total}명 중 {enabled}명이 켰습니다.', { total: d.total, enabled: d.enabled })}${d.me ? '' : ` ${t('내 계정부터 켜세요.')}`}`,
        tab: '2fa',
      }
    case 'https':
      return {
        title: t('사이트 HTTPS'),
        desc: ok ? t('도메인이 있는 사이트 {n}개 모두 HTTPS 입니다.', { n: d.total }) : t('HTTPS 가 없는 사이트: {sites}', { sites: (d.sites ?? []).join(', ') }),
        link: '/sites',
      }
    case 'certificates':
      return {
        title: t('SSL 인증서 만료'),
        desc:
          it.level === 'fail'
            ? t('만료됨: {sites}', { sites: (d.expired ?? []).join(', ') })
            : it.level === 'warn'
              ? t('14일 안에 만료: {sites}', { sites: (d.soon ?? []).map((s) => `${s.name} (${s.days})`).join(', ') })
              : t('인증서 {n}개 모두 14일 넘게 남았습니다.', { n: d.total }),
        link: '/sites',
      }
    case 'login_failures':
      return {
        title: t('로그인 실패'),
        desc: t('최근 24시간 {count}회 · 지금 차단 {blocked}개', { count: d.count, blocked: d.blocked }),
        tab: 'logins',
      }
    default:
      return { title: t('서버 점검'), desc: t('서버 상태를 읽지 못했습니다. rpanel-helper 를 업데이트하세요.') }
  }
}

function CheckTab({ go }) {
  const c = useFetch(() => get('/security/check'), [])
  if (c.loading) return <Loading label={t('점검하는 중…')} />
  if (c.error) return <ErrorBox error={c.error} />
  const order = { fail: 0, warn: 1, info: 2, ok: 3 }
  const items = [...c.data.items].sort((a, b) => order[a.level] - order[b.level])
  const s = c.data.summary
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2">
        {['fail', 'warn', 'info', 'ok'].map((lv) => (
          <span key={lv} className={`inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-sm ${LEVEL[lv].cls}`}>
            {t(LEVEL[lv].label)} <b>{s[lv]}</b>
          </span>
        ))}
        <span className="ml-auto">
          <RefreshButton onClick={() => c.reload()} busy={c.refreshing} />
        </span>
      </div>
      <ul className="space-y-2" aria-label={t('보안 점검')}>
        {items.map((it) => {
          const x = describe(it)
          const lv = LEVEL[it.level] ?? LEVEL.info
          return (
            <li key={it.id} className="card flex items-start gap-3 p-4" data-testid={`check-${it.id}`} data-level={it.level}>
              <span className={`mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full border text-xs font-bold ${lv.cls}`} aria-label={t(lv.label)}>
                {lv.icon}
              </span>
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium">{x.title}</p>
                <p className="mt-0.5 text-sm break-words text-ink-400">{x.desc}</p>
              </div>
              {it.level !== 'ok' && x.tab && (
                <button className="btn-ghost shrink-0 px-2.5 py-1 text-xs" onClick={() => go(x.tab)}>
                  {t('설정하기')}
                </button>
              )}
              {it.level !== 'ok' && x.link && (
                <Link to={x.link} className="btn-ghost shrink-0 px-2.5 py-1 text-xs">
                  {t('설정하기')}
                </Link>
              )}
            </li>
          )
        })}
      </ul>
    </div>
  )
}

// ───────────────────────── 방화벽 ─────────────────────────

function FirewallTab() {
  const toast = useToast()
  const fw = useFetch(() => get('/security/firewall'), [])
  const [rule, setRule] = useState({ port: '', proto: 'tcp', from: '' })
  const [blockIp, setBlockIp] = useState('')
  const [busy, setBusy] = useState(null)
  const [error, setError] = useState(null)

  const act = async (key, fn, done) => {
    setBusy(key)
    setError(null)
    try {
      fw.setData(await fn())
      if (done) toast.push(done, 'success')
      return true
    } catch (e) {
      setError(e)
      return false
    } finally {
      setBusy(null)
    }
  }

  if (fw.loading) return <Loading />
  if (fw.error) return <ErrorBox error={fw.error} />
  const f = fw.data

  if (!f.installed)
    return (
      <Section title={t('방화벽 (ufw)')}>
        <p className="text-sm text-ink-400">{t('ufw 가 설치되어 있지 않습니다.')}</p>
        <p className="mt-2 font-mono text-xs text-ink-300">sudo apt install ufw</p>
      </Section>
    )

  return (
    <div className="space-y-4">
      <Section
        title={t('방화벽 (ufw)')}
        actions={
          <>
            <RefreshButton onClick={() => fw.reload()} busy={fw.refreshing} />
            {f.active ? (
              <button
                className="btn-danger px-3 py-1.5 text-xs"
                disabled={!!busy}
                onClick={async () => (await confirmDialog(t('방화벽을 끌까요? 서버의 모든 포트가 열리게 됩니다.'), { danger: true })) && act('off', () => post('/security/firewall/disable'), t('방화벽을 껐습니다'))}
              >
                {t('끄기')}
              </button>
            ) : (
              <button
                className="btn-primary px-3 py-1.5 text-xs"
                disabled={!!busy}
                onClick={async () =>
                  (await confirmDialog(t('방화벽을 켤까요? SSH({ssh}) · 패널({panel}) 포트는 먼저 열어 둡니다.', { ssh: f.ssh_port, panel: f.panel_port }))) &&
                  act('on', () => post('/security/firewall/enable'), t('방화벽을 켰습니다'))
                }
              >
                {t('켜기')}
              </button>
            )}
          </>
        }
      >
        <p className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
          <span className={f.active ? 'text-signal' : 'text-fail'} data-testid="firewall-state">
            ● {f.active ? t('켜짐') : t('꺼짐')}
          </span>
          <span className="text-xs text-ink-400">
            {t('보호 포트 (지울 수 없음)')}: <span className="font-mono">{f.protected.join(', ')}</span>
          </span>
        </p>
        <ErrorBox error={error} />
      </Section>

      <div className="grid gap-4 lg:grid-cols-2">
        <Section title={t('포트 열기')}>
          <form
            className="grid grid-cols-[1fr_auto] gap-2 sm:grid-cols-[1fr_auto_1fr_auto]"
            onSubmit={async (e) => {
              e.preventDefault()
              if (await act('allow', () => post('/security/firewall/allow', rule), t('포트를 열었습니다'))) setRule({ port: '', proto: 'tcp', from: '' })
            }}
          >
            <input className="input font-mono" required placeholder="3000 · 3000:3010" aria-label={t('포트')} value={rule.port} onChange={(e) => setRule({ ...rule, port: e.target.value })} />
            <select className="input w-auto" aria-label={t('프로토콜')} value={rule.proto} onChange={(e) => setRule({ ...rule, proto: e.target.value })}>
              <option value="tcp">TCP</option>
              <option value="udp">UDP</option>
              <option value="any">TCP+UDP</option>
            </select>
            <input className="input font-mono" placeholder={t('출발지 IP (비우면 어디서나)')} aria-label={t('출발지 IP (비우면 어디서나)')} value={rule.from} onChange={(e) => setRule({ ...rule, from: e.target.value })} />
            <button className="btn-primary" disabled={!!busy}>
              {t('열기')}
            </button>
          </form>
        </Section>
        <Section title={t('IP 차단')}>
          <form
            className="flex gap-2"
            onSubmit={async (e) => {
              e.preventDefault()
              if (!(await confirmDialog(t('{ip} 에서 오는 모든 접속을 막을까요?', { ip: blockIp }), { danger: true }))) return
              if (await act('block', () => post('/security/firewall/block', { ip: blockIp }), t('차단했습니다'))) setBlockIp('')
            }}
          >
            <input className="input font-mono" required placeholder="198.51.100.7 · 198.51.100.0/24" aria-label={t('IP 차단')} value={blockIp} onChange={(e) => setBlockIp(e.target.value)} />
            <button className="btn-danger" disabled={!!busy}>
              {t('차단')}
            </button>
          </form>
        </Section>
      </div>

      <Section title={t('규칙')}>
        {f.active ? (
          f.rules.length ? (
            <div className="overflow-x-auto">
              <table className="w-full text-sm" aria-label={t('규칙')}>
                <thead className="text-left text-xs text-ink-400">
                  <tr>
                    <th className="py-2 pr-3 font-medium">#</th>
                    <th className="px-3 py-2 font-medium">{t('대상')}</th>
                    <th className="px-3 py-2 font-medium">{t('동작')}</th>
                    <th className="px-3 py-2 font-medium">{t('출발지')}</th>
                    <th className="py-2 pl-3" />
                  </tr>
                </thead>
                <tbody className="divide-y divide-ink-800">
                  {f.rules.map((r) => (
                    <tr key={r.num}>
                      <td className="py-2 pr-3 font-mono text-xs text-ink-400">{r.num}</td>
                      <td className="px-3 py-2 font-mono text-xs">
                        {r.to} {r.v6 && <span className="text-ink-400">(IPv6)</span>}
                        {r.comment && <span className="ml-2 font-sans text-ink-400"># {r.comment}</span>}
                      </td>
                      <td className={`px-3 py-2 text-xs ${r.action.startsWith('DENY') || r.action.startsWith('REJECT') ? 'text-fail' : 'text-signal'}`}>{r.action}</td>
                      <td className="px-3 py-2 font-mono text-xs text-ink-300">{r.from}</td>
                      <td className="py-2 pl-3 text-right">
                        {r.protected ? (
                          <span className="text-xs text-ink-400" title={t('SSH·패널·웹 포트는 잠기지 않도록 지울 수 없습니다')}>
                            {t('보호됨')}
                          </span>
                        ) : (
                          <button
                            className="btn-ghost px-2.5 py-1 text-xs"
                            disabled={!!busy}
                            onClick={async () =>
                              (await confirmDialog(t('규칙 #{num} ({to}) 을(를) 지울까요?', { num: r.num, to: r.to }))) &&
                              act(`del-${r.num}`, () => post('/security/firewall/delete', { num: r.num, to: r.to }), t('규칙을 지웠습니다'))
                            }
                          >
                            {t('지우기')}
                          </button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <p className="text-sm text-ink-400">{t('규칙이 없습니다')}</p>
          )
        ) : (
          <>
            <p className="mb-2 text-sm text-ink-400">{t('방화벽이 꺼져 있습니다. 켜면 아래 규칙이 적용됩니다.')}</p>
            <pre className="overflow-x-auto rounded-lg bg-ink-950 p-3 font-mono text-xs text-ink-300">{f.added.length ? f.added.join('\n') : t('규칙이 없습니다')}</pre>
          </>
        )}
      </Section>
    </div>
  )
}

// ───────────────────────── 로그인 보안 ─────────────────────────

const KIND = { ip: 'IP', acct: '계정', otp: '2단계 코드' }

/** User-Agent → "Chrome · Windows" 같은 짧은 이름 */
export function deviceName(ua) {
  if (!ua) return t('알 수 없는 기기')
  const browser = /Edg\//.test(ua)
    ? 'Edge'
    : /OPR\//.test(ua)
      ? 'Opera'
      : /Firefox\//.test(ua)
        ? 'Firefox'
        : /Chrome\//.test(ua)
          ? 'Chrome'
          : /Safari\//.test(ua)
            ? 'Safari'
            : ua.split(/[/ ]/)[0]
  const os = /Windows/.test(ua) ? 'Windows' : /Android/.test(ua) ? 'Android' : /iPhone|iPad/.test(ua) ? 'iOS' : /Mac OS X/.test(ua) ? 'macOS' : /Linux/.test(ua) ? 'Linux' : ''
  return os ? `${browser} · ${os}` : browser
}

const fromUnix = (s) => (s ? new Date(s * 1000).toISOString() : null)

/** 내 로그인 세션 — 기기 · IP, 다른 기기 로그아웃 */
function SessionsSection() {
  const toast = useToast()
  const d = useFetch(() => get('/auth/sessions'), [])
  const run = async (fn, msg) => {
    try {
      const r = await fn()
      toast.push(typeof msg === 'function' ? msg(r) : msg, 'success')
      d.reload(true)
    } catch (e) {
      toast.push(e.message, 'error')
    }
  }
  if (d.loading) return <Loading compact />
  if (d.error) return <ErrorBox error={d.error} />
  const { sessions, current } = d.data
  const others = sessions.filter((x) => x.id !== current)
  return (
    <Section
      title={t('내 로그인 세션')}
      desc={t('지금 로그인되어 있는 기기입니다. 모르는 기기가 있으면 로그아웃시키고 비밀번호를 바꾸세요.')}
      actions={
        others.length > 0 && (
          <button
            className="btn-danger px-3 py-1.5 text-xs"
            onClick={async () =>
              (await confirmDialog(t('다른 기기를 모두 로그아웃시킬까요?'), { danger: true })) &&
              run(
                () => post('/auth/sessions/revoke-others'),
                (r) => t('다른 기기 {n}개를 로그아웃시켰습니다', { n: r.revoked }),
              )
            }
          >
            {t('다른 기기 모두 로그아웃')}
          </button>
        )
      }
    >
      <ul className="divide-y divide-ink-800" aria-label={t('내 로그인 세션')}>
        {sessions.map((x) => (
          <li key={x.id} className="flex flex-wrap items-center gap-x-4 gap-y-1 py-2.5 text-sm" data-testid="session-row">
            <span className="min-w-0 flex-1">
              <span className="font-medium">{deviceName(x.user_agent)}</span>
              {x.id === current && <span className="ml-2 rounded border border-signal/40 px-1.5 text-[11px] text-signal">{t('이 기기')}</span>}
              <span className="block font-mono text-xs text-ink-400">{x.ip || '–'}</span>
            </span>
            <span className="text-xs text-ink-400" title={dateTime(fromUnix(x.created_at))}>
              {t('마지막 사용')} {x.last_seen ? ago(fromUnix(x.last_seen)) : '–'}
            </span>
            {x.id !== current && (
              <button
                className="btn-ghost px-2.5 py-1 text-xs"
                onClick={async () => (await confirmDialog(t('이 기기를 로그아웃시킬까요?'))) && run(() => del(`/auth/sessions/${x.id}`), t('로그아웃시켰습니다'))}
              >
                {t('로그아웃')}
              </button>
            )}
          </li>
        ))}
      </ul>
    </Section>
  )
}

function LoginsTab() {
  const toast = useToast()
  const d = useFetch(() => get('/security/logins'), [])
  if (d.loading) return <Loading />
  if (d.error) return <ErrorBox error={d.error} />
  const { counters, failures, limit } = d.data
  return (
    <div className="space-y-4">
      <SessionsSection />
      <PanelAccess />
      <Section title={t('로그인 시도 제한')} desc={t('IP·계정마다 5분에 {n}번까지 로그인을 시도할 수 있습니다. 넘으면 5분 동안 막힙니다.', { n: limit })} actions={<RefreshButton onClick={() => d.reload()} busy={d.refreshing} />}>
        {counters.length ? (
          <div className="overflow-x-auto">
            <table className="w-full text-sm" aria-label={t('로그인 시도 제한')}>
              <thead className="text-left text-xs text-ink-400">
                <tr>
                  <th className="py-2 pr-3 font-medium">{t('종류')}</th>
                  <th className="px-3 py-2 font-medium">{t('대상')}</th>
                  <th className="px-3 py-2 font-medium">{t('시도')}</th>
                  <th className="px-3 py-2 font-medium">{t('풀리기까지')}</th>
                  <th className="py-2 pl-3" />
                </tr>
              </thead>
              <tbody className="divide-y divide-ink-800">
                {counters.map((c) => (
                  <tr key={c.bucket}>
                    <td className="py-2 pr-3 text-xs">{t(KIND[c.kind] ?? c.kind)}</td>
                    <td className="px-3 py-2 font-mono text-xs break-all">{c.who}</td>
                    <td className={`px-3 py-2 text-xs ${c.blocked ? 'font-semibold text-fail' : 'text-ink-300'}`}>
                      {c.count} / {c.limit} {c.blocked && `· ${t('차단됨')}`}
                    </td>
                    <td className="px-3 py-2 text-xs text-ink-400">{t('{s}초', { s: c.ttl })}</td>
                    <td className="py-2 pl-3 text-right">
                      <button
                        className="btn-ghost px-2.5 py-1 text-xs"
                        onClick={async () => {
                          try {
                            const r = await post('/security/unblock', { bucket: c.bucket })
                            d.setData((x) => ({ ...x, counters: r.counters }))
                            toast.push(t('풀었습니다'), 'success')
                          } catch (e) {
                            toast.push(e.message, 'error')
                          }
                        }}
                      >
                        {t('해제')}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="text-sm text-ink-400">{t('지금 막힌 IP·계정이 없습니다')}</p>
        )}
      </Section>
      <Section title={t('최근 로그인 실패')}>
        {failures.length ? (
          <div className="max-h-[50vh] overflow-auto">
            <table className="w-full text-sm" aria-label={t('최근 로그인 실패')}>
              <thead className="sticky top-0 bg-ink-900 text-left text-xs text-ink-400">
                <tr>
                  <th className="py-2 pr-3 font-medium">{t('시각')}</th>
                  <th className="px-3 py-2 font-medium">{t('이메일')}</th>
                  <th className="px-3 py-2 font-medium">IP</th>
                  <th className="py-2 pl-3 font-medium">{t('이유')}</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-ink-800">
                {failures.map((f) => (
                  <tr key={f.id}>
                    <td className="py-2 pr-3 text-xs text-ink-400" title={dateTime(f.created_at)}>
                      {ago(f.created_at)}
                    </td>
                    <td className="px-3 py-2 text-xs break-all">{f.target}</td>
                    <td className="px-3 py-2 font-mono text-xs">{f.ip}</td>
                    <td className="py-2 pl-3 text-xs text-ink-300">{f.action === 'auth.otp_failed' ? t('인증 코드 틀림') : t('비밀번호 틀림')}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="text-sm text-ink-400">{t('로그인 실패 기록이 없습니다')}</p>
        )}
      </Section>
    </div>
  )
}

// ───────────────────────── 2단계 인증 ─────────────────────────

function QrCode({ text }) {
  const { d, size } = qrPath(text)
  const label = t('QR 코드')
  return (
    <svg viewBox={`0 0 ${size} ${size}`} className="h-48 w-48 rounded-lg bg-white" shapeRendering="crispEdges" role="img" aria-label={label}>
      <path d={d} fill="#000" />
    </svg>
  )
}

function RecoveryCodes({ codes, onDone }) {
  const text = codes.join('\n')
  return (
    <div className="space-y-3">
      <p className="text-sm text-warn">{t('복구 코드는 지금 한 번만 보입니다. 휴대폰을 잃었을 때 코드 대신 하나씩 쓸 수 있으니 안전한 곳에 저장하세요.')}</p>
      <ul className="grid grid-cols-2 gap-2 rounded-lg bg-ink-950 p-4 font-mono text-sm sm:grid-cols-5" data-testid="recovery-codes">
        {codes.map((c) => (
          <li key={c}>{c}</li>
        ))}
      </ul>
      <div className="flex flex-wrap gap-2">
        <CopyButton text={text} />
        <button
          type="button"
          className="btn-ghost px-2.5 py-1 text-xs"
          onClick={() => {
            const a = document.createElement('a')
            a.href = URL.createObjectURL(new Blob([`Rpanel recovery codes\n\n${text}\n`], { type: 'text/plain' }))
            a.download = 'rpanel-recovery-codes.txt'
            a.click()
            URL.revokeObjectURL(a.href)
          }}
        >
          {t('파일로 저장')}
        </button>
        <button type="button" className="btn-primary ml-auto px-3 py-1 text-xs" onClick={onDone}>
          {t('저장했습니다')}
        </button>
      </div>
    </div>
  )
}

/** 비밀번호 + 코드 확인 창 (끄기 · 복구 코드 다시 만들기) */
function ConfirmModal({ title, danger, onClose, onSubmit }) {
  const [v, setV] = useState({ password: '', code: '' })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  return (
    <Modal open title={title} onClose={onClose}>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            await onSubmit(v)
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <Field label={t('비밀번호')} htmlFor="tfa-pw">
          <input id="tfa-pw" type="password" autoComplete="current-password" required className="input" value={v.password} onChange={(e) => setV({ ...v, password: e.target.value })} />
        </Field>
        <Field label={t('인증 코드 또는 복구 코드')} htmlFor="tfa-code">
          <input id="tfa-code" required autoComplete="one-time-code" className="input font-mono" value={v.code} onChange={(e) => setV({ ...v, code: e.target.value })} />
        </Field>
        <ErrorBox error={error} />
        <div className="flex justify-end gap-2">
          <button type="button" className="btn-ghost" onClick={onClose}>
            {t('취소')}
          </button>
          <button className={danger ? 'btn-danger' : 'btn-primary'} disabled={busy}>
            {t('확인')}
          </button>
        </div>
      </form>
    </Modal>
  )
}

/** 모든 관리자에게 2단계 인증 필수 (관리자만) */
function PolicyCard({ myEnabled }) {
  const { user } = useAuth()
  const toast = useToast()
  const p = useFetch(() => (user?.role === 'admin' ? get('/security/policy') : Promise.resolve(null)), [user?.role])
  const [busy, setBusy] = useState(false)
  if (!p.data) return null
  const on = p.data.require_2fa
  const label = t('모든 관리자에게 2단계 인증 필수')
  return (
    <Section title={label} desc={t('켜면 2단계 인증을 등록하지 않은 관리자는 등록을 마칠 때까지 패널을 쓸 수 없습니다.')}>
      <label className="flex items-center gap-2.5 text-sm">
        <input
          type="checkbox"
          role="switch"
          aria-label={label}
          checked={on}
          disabled={busy || (!on && !myEnabled)}
          onChange={async (e) => {
            const next = e.target.checked
            if (next && !(await confirmDialog(t('2단계 인증 필수를 켤까요? 아직 등록하지 않은 관리자는 바로 등록 화면으로 넘어갑니다.')))) return
            setBusy(true)
            try {
              p.setData(await put('/security/policy', { require_2fa: next }))
              toast.push(next ? t('2단계 인증 필수를 켰습니다') : t('2단계 인증 필수를 껐습니다'), 'success')
            } catch (err) {
              toast.push(err.message, 'error')
            } finally {
              setBusy(false)
            }
          }}
        />
        <span data-testid="policy-state">{on ? t('켜짐') : t('꺼짐')}</span>
      </label>
      {!on && !myEnabled && <p className="mt-2 text-xs text-warn">{t('먼저 내 2단계 인증을 켜세요.')}</p>}
    </Section>
  )
}

/** forced: 필수 정책 때문에 등록하는 화면 (관리자 링크 · 정책 카드 숨김), onChange: 켜기·끄기를 마쳤을 때 */
export function TwoFactorTab({ forced = false, onChange }) {
  const toast = useToast()
  const st = useFetch(() => get('/auth/2fa'), [])
  const [setup, setSetup] = useState(null)
  const [code, setCode] = useState('')
  const [codes, setCodes] = useState(null)
  const [modal, setModal] = useState(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)

  if (st.loading) return <Loading />
  if (st.error) return <ErrorBox error={st.error} />

  if (codes)
    return (
      <Section title={t('복구 코드')}>
        <RecoveryCodes
          codes={codes}
          onDone={() => {
            setCodes(null)
            st.reload(true)
            onChange?.()
          }}
        />
      </Section>
    )

  const start = async () => {
    setBusy(true)
    setError(null)
    try {
      setSetup(await post('/auth/2fa/setup'))
      setCode('')
    } catch (e) {
      setError(e)
    } finally {
      setBusy(false)
    }
  }

  if (setup)
    return (
      <Section title={t('2단계 인증 켜기')}>
        <ol className="space-y-5 text-sm">
          <li>
            <p className="font-medium">1. {t('휴대폰의 인증 앱(Google Authenticator, Microsoft Authenticator, 1Password 등)으로 QR 코드를 찍으세요.')}</p>
            <div className="mt-3 flex flex-wrap items-center gap-5">
              <QrCode text={setup.uri} />
              <div className="space-y-1.5">
                <p className="text-xs text-ink-400">{t('QR 을 찍을 수 없으면 이 키를 직접 넣으세요')}</p>
                <code className="block font-mono text-sm tracking-wider break-all" data-testid="totp-secret">
                  {setup.secret.match(/.{1,4}/g).join(' ')}
                </code>
                <CopyButton text={setup.secret} />
              </div>
            </div>
          </li>
          <li>
            <p className="font-medium">2. {t('앱에 나온 6자리 코드를 넣으세요.')}</p>
            <form
              className="mt-2 flex gap-2"
              onSubmit={async (e) => {
                e.preventDefault()
                setBusy(true)
                setError(null)
                try {
                  const r = await post('/auth/2fa/enable', { code })
                  setSetup(null)
                  setCodes(r.recovery_codes)
                  toast.push(t('2단계 인증을 켰습니다'), 'success')
                } catch (err) {
                  setError(err)
                } finally {
                  setBusy(false)
                }
              }}
            >
              <input
                className="input w-40 text-center font-mono text-lg tracking-[0.3em]"
                inputMode="numeric"
                autoComplete="one-time-code"
                maxLength={6}
                required
                aria-label={t('인증 코드')}
                value={code}
                onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
              />
              <button className="btn-primary" disabled={busy || code.length !== 6}>
                {t('확인하고 켜기')}
              </button>
              <button type="button" className="btn-ghost" onClick={() => setSetup(null)}>
                {t('취소')}
              </button>
            </form>
          </li>
        </ol>
        <div className="mt-4">
          <ErrorBox error={error} />
        </div>
      </Section>
    )

  const s = st.data
  return (
    <div className="space-y-4">
    <Section title={t('내 계정의 2단계 인증')} desc={t('로그인할 때 비밀번호와 함께 휴대폰 인증 앱의 6자리 코드를 입력합니다. 비밀번호가 새어 나가도 로그인을 막아 줍니다.')}>
      <p className="flex flex-wrap items-center gap-3 text-sm">
        <span className={s.enabled ? 'text-signal' : 'text-warn'} data-testid="tfa-state">
          ● {s.enabled ? t('켜짐') : t('꺼짐')}
        </span>
        {s.enabled && <span className="text-xs text-ink-400">{t('남은 복구 코드 {n}개', { n: s.recovery_left })}</span>}
      </p>
      <ErrorBox error={error} />
      <div className="mt-4 flex flex-wrap gap-2">
        {s.enabled ? (
          <>
            <button className="btn-ghost" onClick={() => setModal('recovery')}>
              {t('복구 코드 새로 만들기')}
            </button>
            {!s.required && (
              <button className="btn-danger" onClick={() => setModal('disable')}>
                {t('끄기')}
              </button>
            )}
          </>
        ) : (
          <button className="btn-primary" disabled={busy} onClick={start}>
            {t('켜기')}
          </button>
        )}
      </div>
      {!forced && (
        <p className="mt-4 text-xs text-ink-400">
          {t('다른 관리자가 휴대폰을 잃었다면 관리자 화면에서 그 계정의 2단계 인증을 초기화할 수 있습니다.')}{' '}
          <Link to="/admins" className="text-signal hover:underline">
            {t('관리자')} →
          </Link>
        </p>
      )}
      {modal === 'disable' && (
        <ConfirmModal
          title={t('2단계 인증 끄기')}
          danger
          onClose={() => setModal(null)}
          onSubmit={async (v) => {
            await post('/auth/2fa/disable', v)
            setModal(null)
            toast.push(t('2단계 인증을 껐습니다'), 'success')
            st.reload(true)
            onChange?.()
          }}
        />
      )}
      {modal === 'recovery' && (
        <ConfirmModal
          title={t('복구 코드 새로 만들기')}
          onClose={() => setModal(null)}
          onSubmit={async (v) => {
            const r = await post('/auth/2fa/recovery', v)
            setModal(null)
            setCodes(r.recovery_codes)
          }}
        />
      )}
    </Section>
    {!forced && <PolicyCard myEnabled={s.enabled} />}
    </div>
  )
}

/** 2단계 인증 필수인데 아직 등록하지 않은 관리자: 등록을 마칠 때까지 이 화면만 */
export function ForcedTwoFactor({ onDone }) {
  const { logout } = useAuth()
  return (
    <div className="min-h-screen bg-ink-950 px-4 py-10">
      <div className="mx-auto max-w-2xl space-y-4">
        <div className="card border-warn/40 p-5">
          <h1 className="text-lg font-semibold">{t('2단계 인증 등록이 필요합니다')}</h1>
          <p className="mt-1 text-sm text-ink-400">{t('관리자가 모든 계정에 2단계 인증을 필수로 정했습니다. 등록을 마쳐야 패널을 쓸 수 있습니다.')}</p>
        </div>
        <TwoFactorTab forced onChange={onDone} />
        <button type="button" className="text-xs text-ink-400 hover:text-ink-100" onClick={logout}>
          {t('로그아웃')}
        </button>
      </div>
    </div>
  )
}

export default function SecurityPage() {
  const [params, setParams] = useSearchParams()
  const tab = TABS.some((x) => x.id === params.get('tab')) ? params.get('tab') : 'check'
  const go = (id) => setParams({ tab: id }, { replace: true })
  return (
    <>
      <PageHeader title={t('보안')} help={t('서버와 패널의 보안 상태를 점검하고, 방화벽 · 로그인 제한 · 2단계 인증을 관리합니다.')} />
      <Tabs tabs={TABS.map((x) => ({ id: x.id, label: t(x.label) }))} active={tab} onChange={go} />
      {tab === 'check' && <CheckTab go={go} />}
      {tab === 'firewall' && <FirewallTab />}
      {tab === 'logins' && <LoginsTab />}
      {tab === '2fa' && <TwoFactorTab />}
    </>
  )
}
