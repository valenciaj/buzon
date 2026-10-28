import { Fragment, useEffect, useRef } from 'react'
import { useInfiniteQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { api, type Account, type ThreadFilter, type ThreadSummary } from '../../lib/api'
import { ACCOUNT_COLORS, dayGroup, initials, shortDate } from '../../lib/mail'
import { Alert } from '../ui'
import { Icon } from './Icons'

export function ThreadList({ title, filter, accounts, selected, grouped, showAccount, onSelect, onMenu, onCompose, before }: {
  title: string
  filter: ThreadFilter
  accounts: Account[]
  selected: string | null
  /** Agrupar por día (modo Enfoque). */
  grouped: boolean
  /** Marcar de qué cuenta es cada hilo (vista unificada). */
  showAccount: boolean
  onSelect: (id: string) => void
  onMenu: () => void
  onCompose?: () => void
  /** Algo que mostrar antes de los hilos (los borradores de Buzón). */
  before?: React.ReactNode
}) {
  const { t, i18n } = useTranslation()
  const q = useInfiniteQuery({
    queryKey: ['threads', filter],
    queryFn: ({ pageParam }) => api.threads(filter, pageParam),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (p) => p.next_cursor ?? undefined,
  })
  const threads = q.data?.pages.flatMap((p) => p.threads) ?? []
  const sentinel = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = sentinel.current
    if (!el || !q.hasNextPage) return
    const io = new IntersectionObserver((e) => {
      if (e[0]?.isIntersecting && !q.isFetchingNextPage) q.fetchNextPage()
    }, { rootMargin: '400px' })
    io.observe(el)
    return () => io.disconnect()
  }, [q.hasNextPage, q.isFetchingNextPage, q.fetchNextPage, threads.length])

  const color = (id: string) => ACCOUNT_COLORS[accounts.findIndex((a) => a.id === id) % ACCOUNT_COLORS.length]
  const syncing = accounts.some((a) => a.state === 'connecting' || a.state === 'syncing')

  let empty: React.ReactNode = null
  if (accounts.length === 0) {
    empty = (
      <>
        <b className="block text-fg">{t('home.noAccounts')}</b>
        <Link to="/settings/accounts" className="btn btn-primary mt-3">{t('home.addAccount')}</Link>
      </>
    )
  } else if (syncing) {
    empty = <><b className="block text-fg">{t('mail.downloading')}</b>{t('mail.downloadingBody')}</>
  } else {
    empty = <><b className="block text-fg">{t('mail.empty')}</b>{t('mail.emptyBody')}</>
  }

  return (
    <>
      <header className="flex items-center gap-2 border-b border-line px-[var(--pad)] py-3">
        <button className="btn btn-ghost -ml-2 px-2 lg:hidden" onClick={onMenu} aria-label={t('home.menu')}>
          <Icon name="menu" />
        </button>
        <h1 className="min-w-0 flex-1 truncate text-xl font-bold">{title}</h1>
        {onCompose && (
          <button className="btn btn-primary px-3 lg:hidden" onClick={onCompose} aria-label={t('compose.new')}>
            <Icon name="compose" /> <span className="max-sm:sr-only">{t('compose.newShort')}</span>
          </button>
        )}
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto pb-4" role="list" aria-label={title}>
        {q.isError && <div className="p-4"><Alert>{t('common.error')}</Alert></div>}
        {q.isLoading && <p className="p-6 text-muted">{t('common.loading')}</p>}
        {before}
        {q.isSuccess && threads.length === 0 && !before && <div className="px-6 py-12 text-center text-muted">{empty}</div>}
        {threads.map((th, i) => {
          const group = grouped ? dayGroup(th.last_date) : null
          const newGroup = grouped && (i === 0 || dayGroup(threads[i - 1].last_date) !== group)
          return (
            <Fragment key={th.id}>
              {newGroup && <p className="eyebrow px-[var(--pad)] pb-1 pt-4">{t(`mail.groups.${group}`)}</p>}
              <ThreadItem thread={th} lang={i18n.language} selected={selected === th.id}
                dot={showAccount ? color(th.account_id) : undefined} onClick={() => onSelect(th.id)} />
            </Fragment>
          )
        })}
        <div ref={sentinel} />
        {q.isFetchingNextPage && <p className="p-4 text-center text-sm text-muted">{t('common.loading')}</p>}
      </div>
    </>
  )
}

function ThreadItem({ thread: th, lang, selected, dot, onClick }: {
  thread: ThreadSummary
  lang: string
  selected: boolean
  dot?: string
  onClick: () => void
}) {
  const { t } = useTranslation()
  const unread = th.unread > 0
  const who = th.participants.length ? th.participants : ['?']
  const last = who[who.length - 1]
  return (
    <button role="listitem" aria-current={selected} onClick={onClick}
      className={`grid w-full grid-cols-[2.5rem_minmax(0,1fr)_auto] items-start gap-3 border-b border-line px-[var(--pad)] py-3 text-left hover:bg-surface-2 ${selected ? 'bg-[color-mix(in_srgb,var(--accent)_10%,var(--surface))]' : ''}`}>
      <span className="relative grid size-10 place-items-center rounded-full bg-surface-2 text-sm font-bold" aria-hidden="true">
        {initials(last)}
        {dot && <span className="absolute -bottom-px -right-px size-3 rounded-full border-2 border-surface" style={{ background: dot }} />}
      </span>
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className={`flex min-w-0 items-center gap-1.5 ${unread ? 'font-bold' : 'font-semibold'}`}>
          {unread && <span className="size-2 shrink-0 rounded-full bg-accent" aria-label={t('mail.unread')} />}
          <span className="truncate">{who.slice(-3).join(', ')}</span>
          {th.messages > 1 && <span className="shrink-0 text-sm font-normal text-muted tabular-nums">{th.messages}</span>}
        </span>
        <span className={`truncate ${unread ? 'font-semibold' : ''}`}>{th.subject || t('mail.noSubject')}</span>
        <span className="line-clamp-2 text-sm text-muted">{th.snippet}</span>
      </span>
      <span className="flex flex-col items-end gap-1 text-sm text-muted tabular-nums">
        <span className="whitespace-nowrap">{shortDate(th.last_date, lang)}</span>
        <span className="flex gap-1">
          {th.has_attachments && <Icon name="clip" className="size-4" />}
          {th.flagged && <Icon name="star" filled className="size-4 text-warn" />}
        </span>
      </span>
    </button>
  )
}
