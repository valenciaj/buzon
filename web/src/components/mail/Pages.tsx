import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError, type Draft, type Me } from '../../lib/api'
import { fromDraft } from '../../lib/compose'
import { shortDate, type Box, type Page } from '../../lib/mail'
import { useToast } from '../Toaster'
import { Alert } from '../ui'
import { useCompose } from '../compose/ComposerProvider'
import { Icon } from './Icons'

function Header({ title, intro, onMenu }: { title: string; intro?: string; onMenu: () => void }) {
  const { t } = useTranslation()
  return (
    <header className="flex flex-col gap-1 border-b border-line px-[var(--pad)] py-3">
      <div className="flex items-center gap-2">
        <button className="btn btn-ghost -ml-2 px-2 lg:hidden" onClick={onMenu} aria-label={t('home.menu')}>
          <Icon name="menu" />
        </button>
        <h1 className="min-w-0 flex-1 truncate text-xl font-bold">{title}</h1>
      </div>
      {intro && <p className="text-sm text-muted">{intro}</p>}
    </header>
  )
}

const recipients = (d: { to: { name: string | null; email: string }[] }) => d.to.map((a) => a.name || a.email).join(', ')

/** Páginas que no son listas de hilos: Hoy, Programados y Sin respuesta. */
export function PageView({ page, ...rest }: {
  page: Page
  me: Me
  account?: string
  onMenu: () => void
  onBox: (b: Box) => void
  onOpen: (thread: string) => void
}) {
  if (page === 'today') return <Today {...rest} />
  if (page === 'scheduled') return <Scheduled onMenu={rest.onMenu} />
  return <FollowUps onMenu={rest.onMenu} onOpen={rest.onOpen} />
}

/** «Hoy»: lo que pide atención, de un vistazo. */
function Today({ me, account, onMenu, onBox, onOpen }: {
  me: Me
  account?: string
  onMenu: () => void
  onBox: (b: Box) => void
  onOpen: (thread: string) => void
}) {
  const { t, i18n } = useTranslation()
  const focus = me.preferences.mode === 'focus'
  const flow = useQuery({ queryKey: ['flow', account ?? null], queryFn: () => api.flowCounts(account), enabled: focus })
  const mailboxes = useQuery({ queryKey: ['mailboxes'], queryFn: () => api.mailboxes() })
  const due = useQuery({ queryKey: ['followups', 'due'], queryFn: () => api.followups(true) })
  const scheduled = useQuery({ queryKey: ['scheduled'], queryFn: api.scheduled })
  const later = useQuery({
    queryKey: ['threads', { view: 'reply_later', account }],
    queryFn: () => api.threads({ view: 'reply_later', account }),
    enabled: focus,
  })
  const c = flow.data
  const inboxUnread = (mailboxes.data ?? [])
    .filter((m) => m.role === 'inbox' && (!account || m.account_id === account))
    .reduce((n, m) => n + m.unread, 0)
  const soon = (scheduled.data ?? []).filter((d) => (d.send_at ?? 0) > 0 && (d.send_at ?? 0) < Date.now() / 1000 + 86400)
  const today = new Date().toLocaleDateString(i18n.language, { weekday: 'long', day: 'numeric', month: 'long' })

  const tiles: { key: string; label: string; value: number; box: Box; strong?: boolean }[] = focus
    ? [
      { key: 'principal', label: t('today.unreadPrincipal'), value: (c?.principal_unread ?? 0) + (c?.bubbled ?? 0), box: { view: 'principal' }, strong: true },
      { key: 'screener', label: t('flow.views.screener'), value: c?.screener ?? 0, box: { view: 'screener' } },
      { key: 'later', label: t('flow.views.reply_later'), value: c?.reply_later ?? 0, box: { view: 'reply_later' } },
      { key: 'due', label: t('today.noReply'), value: due.data?.length ?? 0, box: { page: 'followups' } },
      { key: 'scheduled', label: t('today.scheduled'), value: scheduled.data?.length ?? 0, box: { page: 'scheduled' } },
    ]
    : [
      { key: 'inbox', label: t('today.unreadInbox'), value: inboxUnread, box: { role: 'inbox' }, strong: true },
      { key: 'due', label: t('today.noReply'), value: due.data?.length ?? 0, box: { page: 'followups' } },
      { key: 'scheduled', label: t('today.scheduled'), value: scheduled.data?.length ?? 0, box: { page: 'scheduled' } },
    ]
  const nothing = tiles.every((x) => x.value === 0)

  return (
    <>
      <Header title={t('today.title')} intro={today.charAt(0).toUpperCase() + today.slice(1)} onMenu={onMenu} />
      <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto px-[var(--pad)] py-4">
        {nothing && (
          <p role="status" className="flex items-center gap-2 font-semibold text-good">
            <Icon name="check" className="shrink-0" /> {t('today.allClear')}
          </p>
        )}
        <ul className="grid grid-cols-2 gap-2" aria-label={t('today.title')}>
          {tiles.map((x) => (
            <li key={x.key}>
              <button onClick={() => onBox(x.box)}
                className="flex h-full w-full flex-col items-start gap-0.5 rounded-xl border border-line bg-surface px-3 py-2.5 text-left hover:bg-surface-2">
                <span className={`text-2xl font-bold tabular-nums ${x.strong && x.value > 0 ? 'text-accent' : ''}`}>{x.value}</span>
                <span className="text-sm text-muted">{x.label}</span>
              </button>
            </li>
          ))}
        </ul>
        {(due.data?.length ?? 0) > 0 && (
          <section className="flex flex-col gap-1">
            <h2 className="eyebrow">{t('today.noReply')}</h2>
            <ul className="flex flex-col">
              {due.data!.slice(0, 5).map((f) => (
                <li key={f.id}>
                  <button className="flex w-full flex-col rounded-lg px-2 py-2 text-left hover:bg-surface-2 disabled:opacity-70"
                    disabled={!f.thread} onClick={() => f.thread && onOpen(f.thread)}>
                    <span className="truncate font-semibold">{f.subject || t('mail.noSubject')}</span>
                    <span className="truncate text-sm text-muted">{t('today.sentTo', { who: recipients(f), date: shortDate(f.sent_at, i18n.language) })}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}
        {focus && (later.data?.threads.length ?? 0) > 0 && (
          <section className="flex flex-col gap-1">
            <h2 className="eyebrow">{t('flow.views.reply_later')}</h2>
            <ul className="flex flex-col">
              {later.data!.threads.slice(0, 5).map((x) => (
                <li key={x.id}>
                  <button className="flex w-full flex-col rounded-lg px-2 py-2 text-left hover:bg-surface-2" onClick={() => onOpen(x.id)}>
                    <span className="truncate font-semibold">{x.subject || t('mail.noSubject')}</span>
                    <span className="truncate text-sm text-muted">{x.participants.join(', ')}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}
        {soon.length > 0 && (
          <section className="flex flex-col gap-1">
            <h2 className="eyebrow">{t('today.leavingSoon')}</h2>
            <ul className="flex flex-col">
              {soon.map((d) => (
                <li key={d.id} className="flex flex-col px-2 py-2">
                  <span className="truncate font-semibold">{d.subject || t('mail.noSubject')}</span>
                  <span className="truncate text-sm text-muted">{when(d, i18n.language)} · {recipients(d)}</span>
                </li>
              ))}
            </ul>
          </section>
        )}
      </div>
    </>
  )
}

function when(d: Draft, lang: string) {
  return new Date(Math.abs(d.send_at ?? 0) * 1000).toLocaleString(lang, { weekday: 'short', day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })
}

function Scheduled({ onMenu }: { onMenu: () => void }) {
  const { t, i18n } = useTranslation()
  const qc = useQueryClient()
  const compose = useCompose()
  const toast = useToast()
  const list = useQuery({ queryKey: ['scheduled'], queryFn: api.scheduled, refetchInterval: 15000 })

  async function cancel(d: Draft, edit: boolean) {
    try {
      const back = await api.cancelScheduled(d.id)
      if (edit) compose(fromDraft(back))
      else toast({ text: t('scheduled.cancelled') })
    } catch (e) {
      toast({ text: e instanceof ApiError ? e.message : t('common.error') })
    }
    qc.invalidateQueries({ queryKey: ['scheduled'] })
    qc.invalidateQueries({ queryKey: ['drafts'] })
  }

  return (
    <>
      <Header title={t('today.scheduled')} intro={t('scheduled.intro')} onMenu={onMenu} />
      <div className="min-h-0 flex-1 overflow-y-auto pb-4" role="list" aria-label={t('today.scheduled')}>
        {list.isError && <div className="p-4"><Alert>{t('common.error')}</Alert></div>}
        {list.isSuccess && list.data.length === 0 && (
          <p className="px-6 py-12 text-center text-muted">{t('scheduled.none')}</p>
        )}
        {list.data?.map((d) => (
          <div key={d.id} role="listitem" className="flex flex-col gap-2 border-b border-line px-[var(--pad)] py-3">
            <div className="flex items-start gap-3">
              <span className="grid size-10 shrink-0 place-items-center rounded-full bg-surface-2 text-muted" aria-hidden="true">
                <Icon name="clock" />
              </span>
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="truncate font-semibold">{d.subject || t('mail.noSubject')}</span>
                <span className="truncate text-sm text-muted">{recipients(d)}</span>
                <span className="text-sm text-accent">
                  {(d.send_at ?? 0) < 0 ? t('scheduled.leaving') : t('scheduled.at', { when: when(d, i18n.language) })}
                  {d.follow_up_days ? ` · ${t('compose.followUpOn', { count: d.follow_up_days })}` : ''}
                </span>
              </span>
            </div>
            {(d.send_at ?? 0) > 0 && (
              <div className="flex flex-wrap gap-2 pl-[3.25rem]">
                <button className="btn min-h-9 px-3 py-1" onClick={() => cancel(d, true)}>{t('scheduled.edit')}</button>
                <button className="btn btn-ghost min-h-9 px-3 py-1" onClick={() => cancel(d, false)}>{t('scheduled.cancel')}</button>
              </div>
            )}
          </div>
        ))}
      </div>
    </>
  )
}

function FollowUps({ onMenu, onOpen }: { onMenu: () => void; onOpen: (thread: string) => void }) {
  const { t, i18n } = useTranslation()
  const qc = useQueryClient()
  const list = useQuery({ queryKey: ['followups', 'all'], queryFn: () => api.followups() })
  const now = Date.now() / 1000

  async function dismiss(id: string) {
    await api.dismissFollowup(id).catch(() => undefined)
    qc.invalidateQueries({ queryKey: ['followups'] })
  }

  return (
    <>
      <Header title={t('today.noReply')} intro={t('followups.intro')} onMenu={onMenu} />
      <div className="min-h-0 flex-1 overflow-y-auto pb-4" role="list" aria-label={t('today.noReply')}>
        {list.isError && <div className="p-4"><Alert>{t('common.error')}</Alert></div>}
        {list.isSuccess && list.data.length === 0 && (
          <p className="px-6 py-12 text-center text-muted">{t('followups.none')}</p>
        )}
        {list.data?.map((f) => {
          const overdue = f.due_at <= now
          return (
            <div key={f.id} role="listitem" className="flex flex-col gap-2 border-b border-line px-[var(--pad)] py-3">
              <button className="flex items-start gap-3 text-left disabled:cursor-default" disabled={!f.thread} onClick={() => f.thread && onOpen(f.thread)}>
                <span className={`grid size-10 shrink-0 place-items-center rounded-full bg-surface-2 ${overdue ? 'text-crit' : 'text-muted'}`} aria-hidden="true">
                  <Icon name="bell" />
                </span>
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate font-semibold">{f.subject || t('mail.noSubject')}</span>
                  <span className="truncate text-sm text-muted">{t('today.sentTo', { who: recipients(f), date: shortDate(f.sent_at, i18n.language) })}</span>
                  <span className={`text-sm ${overdue ? 'font-semibold text-crit' : 'text-muted'}`}>
                    {overdue ? t('followups.overdue') : t('followups.waiting', { date: shortDate(f.due_at, i18n.language) })}
                  </span>
                </span>
              </button>
              <div className="pl-[3.25rem]">
                <button className="btn btn-ghost min-h-9 px-3 py-1" onClick={() => dismiss(f.id)}>{t('followups.dismiss')}</button>
              </div>
            </div>
          )
        })}
      </div>
    </>
  )
}
