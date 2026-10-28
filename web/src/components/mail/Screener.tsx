import { useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError, type Bucket, type SenderSummary } from '../../lib/api'
import { initials, shortDate } from '../../lib/mail'
import { useToast } from '../Toaster'
import { Alert } from '../ui'
import { Icon } from './Icons'

const BUCKETS: Bucket[] = ['principal', 'news', 'receipts']

/** Decidir sobre un remitente: aprobar (a una bandeja) o rechazar. */
export function useScreen() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const toast = useToast()
  return async (s: { email: string; name: string }, approve: boolean, bucket?: Bucket) => {
    try {
      const r = await api.decideSender(s.email, approve ? 'approved' : 'blocked', bucket)
      const name = s.name || s.email
      toast({
        text: approve ? t('flow.toast.approved', { name, bucket: t(`flow.buckets.${r.bucket}`) }) : t('flow.toast.blocked', { name }),
      })
    } catch (e) {
      toast({ text: e instanceof ApiError ? e.message : t('common.error') })
    }
    for (const k of ['senders', 'flow', 'threads', 'thread']) qc.invalidateQueries({ queryKey: [k] })
  }
}

/** «Por aprobar»: los remitentes nuevos esperan aquí hasta que se decide sobre ellos. */
export function Screener({ selected, onOpen, onMenu, onBlocked }: {
  selected: string | null
  onOpen: (thread: string) => void
  onMenu: () => void
  onBlocked: () => void
}) {
  const { t } = useTranslation()
  const q = useQuery({ queryKey: ['senders', 'pending'], queryFn: () => api.senders('pending') })
  const list = q.data ?? []
  return (
    <>
      <header className="flex flex-col gap-1 border-b border-line px-[var(--pad)] py-3">
        <div className="flex items-center gap-2">
          <button className="btn btn-ghost -ml-2 px-2 lg:hidden" onClick={onMenu} aria-label={t('home.menu')}>
            <Icon name="menu" />
          </button>
          <h1 className="min-w-0 flex-1 truncate text-xl font-bold">{t('flow.views.screener')}</h1>
          <button className="btn btn-ghost px-2.5 text-sm" onClick={onBlocked}>{t('flow.seeBlocked')}</button>
        </div>
        <p className="text-sm text-muted">{t('flow.screenerIntro')}</p>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto pb-4" role="list" aria-label={t('flow.views.screener')}>
        {q.isError && <div className="p-4"><Alert>{t('common.error')}</Alert></div>}
        {q.isLoading && <p className="p-6 text-muted">{t('common.loading')}</p>}
        {q.isSuccess && list.length === 0 && (
          <div className="px-6 py-12 text-center text-muted">
            <b className="block text-fg">{t('flow.screenerEmpty')}</b>
            {t('flow.screenerEmptyBody')}
          </div>
        )}
        {list.map((s) => <SenderRow key={s.email} sender={s} selected={selected === s.last_thread} onOpen={onOpen} />)}
      </div>
    </>
  )
}

function SenderRow({ sender: s, selected, onOpen }: { sender: SenderSummary; selected: boolean; onOpen: (t: string) => void }) {
  const { t, i18n } = useTranslation()
  const screen = useScreen()
  const [bucket, setBucket] = useState<Bucket>(s.bucket)
  const [busy, setBusy] = useState(false)
  const name = s.name || s.email
  const decide = async (approve: boolean) => {
    setBusy(true)
    await screen(s, approve, bucket)
    setBusy(false)
  }
  return (
    <div role="listitem" className={`flex flex-col gap-2.5 border-b border-line px-[var(--pad)] py-3 ${selected ? 'bg-[color-mix(in_srgb,var(--accent)_10%,var(--surface))]' : ''}`}>
      <button className="grid grid-cols-[2.5rem_minmax(0,1fr)_auto] items-start gap-3 text-left" onClick={() => s.last_thread && onOpen(s.last_thread)}>
        <span className="grid size-10 place-items-center rounded-full bg-surface-2 text-sm font-bold" aria-hidden="true">{initials(name)}</span>
        <span className="flex min-w-0 flex-col">
          <span className="truncate font-semibold">{name}</span>
          <span className="truncate text-sm text-muted">{s.email}</span>
          <span className="truncate">{s.last_subject || t('mail.noSubject')}</span>
          {s.threads > 1 && <span className="text-sm text-muted">{t('flow.threads', { count: s.threads })}</span>}
        </span>
        <span className="text-sm text-muted tabular-nums">{shortDate(s.last_date, i18n.language)}</span>
      </button>
      <div className="flex flex-wrap items-center gap-1.5 pl-[3.25rem]">
        <div className="flex rounded-lg border border-line p-0.5" role="radiogroup" aria-label={t('flow.goesTo')}>
          {BUCKETS.map((b) => (
            <button key={b} role="radio" aria-checked={bucket === b} onClick={() => setBucket(b)}
              className={`rounded-md px-2 py-1 text-sm ${bucket === b ? 'bg-surface-2 font-semibold' : 'text-muted'}`}>
              {t(`flow.buckets.${b}`)}
            </button>
          ))}
        </div>
        <div className="ml-auto flex gap-1.5">
          <button className="btn btn-primary min-h-9 px-3 py-1" disabled={busy} onClick={() => decide(true)}
            aria-label={`${t('flow.yes')}: ${name}`}>
            <Icon name="check" /> {t('flow.yes')}
          </button>
          <button className="btn min-h-9 px-3 py-1" disabled={busy} onClick={() => decide(false)} aria-label={`${t('flow.no')}: ${name}`}>
            <Icon name="close" /> {t('flow.no')}
          </button>
        </div>
      </div>
    </div>
  )
}
