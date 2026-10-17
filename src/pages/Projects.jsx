import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { Empty, ErrorBox, Field, Modal, PageHeader, Spinner } from '../components/ui'
import { useToast } from '../context/ToastContext'
import { get, post } from '../lib/api'
import { useFetch } from '../lib/useFetch'
import { t, tx } from '../i18n'

export function ProjectFormModal({ open, onClose, onSaved, initial }) {
  const [name, setName] = useState(initial?.name ?? '')
  const [description, setDescription] = useState(initial?.description ?? '')
  const [error, setError] = useState(null)
  const [busy, setBusy] = useState(false)
  return (
    <Modal open={open} onClose={onClose} title={initial ? t('프로젝트 수정') : t('새 프로젝트')}>
      <form
        className="space-y-4"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            await onSaved({ name, description })
          } catch (err) {
            setError(err)
          } finally {
            setBusy(false)
          }
        }}
      >
        <Field label={t('이름')} htmlFor="p-name">
          <input id="p-name" className="input" required value={name} onChange={(e) => setName(e.target.value)} placeholder="Beripick" />
        </Field>
        <Field label={t('설명')} htmlFor="p-desc">
          <textarea id="p-desc" className="input min-h-20" value={description} onChange={(e) => setDescription(e.target.value)} />
        </Field>
        <ErrorBox error={error} />
        <div className="flex justify-end">
          <button className="btn-primary" disabled={busy}>
            {busy ? t('저장 중…') : t('저장')}
          </button>
        </div>
      </form>
    </Modal>
  )
}

export default function Projects() {
  const { data, error, loading } = useFetch(() => get('/projects'), [])
  const [open, setOpen] = useState(false)
  const nav = useNavigate()
  const toast = useToast()

  const create = async (body) => {
    const p = await post('/projects', body)
    toast.push(t("프로젝트 '{name}' 생성됨", { name: p.name }), 'success')
    nav(`/projects/${p.id}`)
  }

  return (
    <>
      <PageHeader
        title={t('프로젝트')}
        help={
          <>
            {t('서비스 단위로 앱(백엔드·프런트)을 묶어 관리합니다')}
            <br />
            {tx('예: {name} 프로젝트 안에 Rust·Python API 앱과 React 프런트 앱을 둡니다.', { name: <span className="kbd">beripick</span> })}
          </>
        }
        actions={
          <button className="btn-primary" onClick={() => setOpen(true)}>
            {t('+ 새 프로젝트')}
          </button>
        }
      />
      <ErrorBox error={error} />
      {loading ? (
        <Spinner className="text-ink-400" />
      ) : data?.length ? (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {data.map((p) => (
            <Link key={p.id} to={`/projects/${p.id}`} className="card group p-5 transition hover:border-ink-600">
              <div className="flex items-start justify-between gap-3">
                <h2 className="font-medium group-hover:text-signal">{p.name}</h2>
                <span className="text-xs text-ink-400">{t('{running_count}/{app_count} 실행', { running_count: p.running_count, app_count: p.app_count })}</span>
              </div>
              <p className="mt-2 line-clamp-2 min-h-10 text-sm text-ink-400">{p.description || t('설명 없음')}</p>
            </Link>
          ))}
        </div>
      ) : (
        <Empty
          title={t('프로젝트가 없습니다')}
          action={
            <button className="btn-primary" onClick={() => setOpen(true)}>
              {t('첫 프로젝트 만들기')}
            </button>
          }
        />
      )}
      {open && <ProjectFormModal open onClose={() => setOpen(false)} onSaved={create} />}
    </>
  )
}
