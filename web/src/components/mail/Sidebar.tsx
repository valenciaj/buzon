import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { api, type Account, type Mailbox, type Me, type Role, type View } from '../../lib/api'
import { ACCOUNT_COLORS, boxKey, customFolders, ROLE_ORDER, unreadByRole, type Box } from '../../lib/mail'
import { Logo } from '../ui'
import { Icon } from './Icons'
import { manualUrl } from '../../lib/manual'

/** En Enfoque, la entrada se reparte en bandejas y pilas; el resto queda en «Más carpetas». */
const FOCUS_MAIN: Role[] = ['sent', 'drafts']
const FOCUS_BOXES: View[] = ['principal', 'news', 'receipts', 'screener']
const FOCUS_PILES: View[] = ['reply_later', 'aside', 'snoozed']
export const VIEW_ICONS: Record<View, string> = {
  principal: 'inbox', news: 'news', receipts: 'receipt', screener: 'screener', blocked: 'junk',
  reply_later: 'later', aside: 'pin', snoozed: 'clock', bubbled: 'clock',
}

export function Sidebar({ me, accounts, mailboxes, account, box, onAccount, onBox, onLogout, onCompose, onShortcuts }: {
  me: Me
  accounts: Account[]
  mailboxes: Mailbox[]
  /** Cuenta seleccionada; sin ella, todas combinadas. */
  account?: string
  box: Box
  onAccount: (id: string | null) => void
  onBox: (b: Box) => void
  onLogout: () => void
  onCompose?: () => void
  onShortcuts: () => void
}) {
  const { t, i18n } = useTranslation()
  const focus = me.preferences.mode === 'focus'
  const [more, setMore] = useState(false)
  const labels = useQuery({ queryKey: ['labels'], queryFn: api.labels })
  const flow = useQuery({ queryKey: ['flow', account ?? null], queryFn: () => api.flowCounts(account), enabled: focus })
  const scheduled = useQuery({ queryKey: ['scheduled'], queryFn: api.scheduled })
  const due = useQuery({ queryKey: ['followups', 'due'], queryFn: () => api.followups(true) })
  const counts = flow.data
  const viewCount = (v: View): number => {
    if (!counts) return 0
    switch (v) {
      case 'principal': return counts.principal_unread + counts.bubbled
      case 'news': return counts.news_unread
      case 'receipts': return counts.receipts_unread
      case 'screener': return counts.screener
      case 'reply_later': return counts.reply_later
      case 'aside': return counts.aside
      case 'snoozed': return counts.snoozed
      default: return 0
    }
  }
  const unread = unreadByRole(mailboxes, account)
  const present = new Set(mailboxes.filter((m) => !account || m.account_id === account).map((m) => m.role))
  const roles = ROLE_ORDER.filter((r) => r === 'inbox' || present.has(r))
  const main = focus ? roles.filter((r) => FOCUS_MAIN.includes(r)) : roles
  // En Enfoque, la entrada completa (sin repartir) también está, entre las demás carpetas.
  const extraRoles = focus ? roles.filter((r) => !FOCUS_MAIN.includes(r)) : []
  const folders = customFolders(mailboxes, account)
  const current = boxKey(box)
  const color = (id: string) => ACCOUNT_COLORS[accounts.findIndex((a) => a.id === id) % ACCOUNT_COLORS.length]
  const roleLabel = (r: Role) => t(`mail.roles.${r}`)
  const showMore = !focus || more || extraRoles.some((r) => boxKey({ role: r }) === current)
    || folders.some((f) => boxKey({ mailbox: f.id }) === current)

  const item = (key: string, label: string, icon: string, count: number, onClick: () => void, dot?: string, swatch?: string) => (
    <li key={key}>
      <button aria-current={current === key} onClick={onClick}
        className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-surface ${current === key ? 'bg-surface font-semibold shadow-[inset_3px_0_0_var(--accent)]' : ''}`}>
        {swatch
          ? <span className="grid size-[18px] shrink-0 place-items-center"><span className="size-3 rounded-full" style={{ background: swatch }} /></span>
          : <Icon name={icon} className="shrink-0 text-muted" />}
        <span className="min-w-0 flex-1 truncate">{label}</span>
        {dot && <span className="size-2 shrink-0 rounded-full" style={{ background: dot }} />}
        {count > 0 && <span className={`text-sm tabular-nums ${key === 'inbox' || key === 'v:principal' || key === 'v:screener' || key === 'p:followups' ? 'font-bold text-accent' : 'text-muted'}`}>{count}</span>}
      </button>
    </li>
  )

  const errors = accounts.filter((a) => a.state === 'error')
  const syncing = accounts.some((a) => a.state === 'connecting' || a.state === 'syncing')

  return (
    <nav className="flex h-full flex-col gap-5 overflow-y-auto p-4" aria-label={t('home.menu')}>
      <div className="flex items-center justify-between gap-2">
        <Logo />
        {onCompose && (
          <button className="btn btn-primary size-10 rounded-full px-0" onClick={onCompose}
            title={t('compose.new')} aria-label={t('compose.new')}>
            <Icon name="compose" />
          </button>
        )}
      </div>
      {/* En la vista unificada no hace falta elegir cuenta (se cambia en Ajustes › Apariencia). */}
      {accounts.length > 0 && me.preferences.view !== 'unified' && (
        <div className="flex flex-col gap-2">
          <p className="eyebrow">{t('home.accounts')}</p>
          <div className="flex flex-wrap gap-1.5">
            {accounts.length > 1 && <Chip active={!account} onClick={() => onAccount(null)}>{t('home.all')}</Chip>}
            {accounts.map((a) => (
              <Chip key={a.id} active={account === a.id || accounts.length === 1} onClick={() => onAccount(a.id)}>
                <span className="size-2 rounded-full" style={{ background: color(a.id) }} />
                {a.name}
              </Chip>
            ))}
          </div>
        </div>
      )}
      <ul className="-mb-3 flex flex-col gap-0.5">
        {item('p:today', t('pages.today'), 'sun', 0, () => onBox({ page: 'today' }))}
      </ul>
      {focus && (
        <>
          <ul className="flex flex-col gap-0.5">
            {FOCUS_BOXES.map((v) => item(`v:${v}`, t(`flow.views.${v}`), VIEW_ICONS[v], viewCount(v), () => onBox({ view: v })))}
          </ul>
          <div className="flex flex-col gap-1">
            <p className="eyebrow px-3">{t('flow.piles')}</p>
            <ul className="flex flex-col gap-0.5">
              {FOCUS_PILES.map((v) => item(`v:${v}`, t(`flow.views.${v}`), VIEW_ICONS[v], viewCount(v), () => onBox({ view: v })))}
            </ul>
          </div>
        </>
      )}
      <ul className="flex flex-col gap-0.5">
        {main.map((r) => item(r, roleLabel(r), r, r === 'inbox' || r === 'junk' || !focus ? unread[r] ?? 0 : 0, () => onBox({ role: r })))}
        {(scheduled.data?.length ?? 0) > 0 && item('p:scheduled', t('pages.scheduled'), 'clock', scheduled.data!.length, () => onBox({ page: 'scheduled' }))}
        {item('p:followups', t('pages.followups'), 'bell', due.data?.length ?? 0, () => onBox({ page: 'followups' }))}
      </ul>
      {(extraRoles.length > 0 || folders.length > 0) && (
        <div className="flex flex-col gap-1">
          {focus ? (
            <button className="eyebrow flex items-center gap-1 px-3 text-left" aria-expanded={showMore} onClick={() => setMore(!showMore)}>
              {t('mail.moreFolders')} <span aria-hidden="true">{showMore ? '▴' : '▾'}</span>
            </button>
          ) : (
            folders.length > 0 && <p className="eyebrow px-3">{t('mail.folders')}</p>
          )}
          {showMore && (
            <ul className="flex flex-col gap-0.5">
              {extraRoles.map((r) => item(r, roleLabel(r), r, 0, () => onBox({ role: r })))}
              {folders.map((f) => item(`m:${f.id}`, f.name, 'folder', f.unread, () => onBox({ mailbox: f.id }),
                !account && accounts.length > 1 ? color(f.account_id) : undefined))}
            </ul>
          )}
        </div>
      )}
      <div className="flex flex-col gap-1">
        <div className="flex items-center justify-between px-3">
          <p className="eyebrow">{t('labels.title')}</p>
          <Link to="/settings/labels" className="-mr-1 grid size-8 place-items-center rounded-md text-muted hover:bg-surface hover:text-fg"
            title={t('labels.manage')} aria-label={t('labels.manage')}>
            <Icon name="sliders" className="size-4" />
          </Link>
        </div>
        {(labels.data ?? []).length > 0 ? (
          <ul className="flex flex-col gap-0.5">
            {(labels.data ?? []).map((l) => item(`l:${l.id}`, l.name, 'tag', l.unread, () => onBox({ label: l.id }), undefined, l.color))}
          </ul>
        ) : (
          <Link to="/settings/labels" className="px-3 text-sm text-muted hover:text-fg">{t('labels.add')}</Link>
        )}
      </div>
      <div className="mt-auto flex flex-col gap-1">
        {errors.length > 0 && (
          <Link to="/settings/accounts" className="flex items-start gap-2 rounded-lg px-3 py-2 text-sm text-crit hover:bg-surface">
            <Icon name="alert" className="mt-0.5 shrink-0" />
            <span>{t('mail.syncError', { name: errors.map((a) => a.name).join(', ') })}</span>
          </Link>
        )}
        {syncing && errors.length === 0 && (
          <p className="flex items-center gap-2 px-3 text-sm text-muted" role="status">
            <Icon name="sync" className="shrink-0 motion-safe:animate-spin" /> {t('mail.syncing')}
          </p>
        )}
        <Link to="/contacts" className="flex items-center gap-3 rounded-lg px-3 py-2 hover:bg-surface">
          <Icon name="contacts" className="shrink-0 text-muted" /> {t('home.contacts')}
        </Link>
        <button onClick={onShortcuts} className="flex items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-surface">
          <Icon name="keyboard" className="shrink-0 text-muted" /> {t('shortcuts.title')}
        </button>
        <a href={manualUrl(i18n.language)} target="_blank" rel="noopener" className="flex items-center gap-3 rounded-lg px-3 py-2 hover:bg-surface">
          <Icon name="help" className="shrink-0 text-muted" /> {t('home.manual')}
        </a>
        <Link to="/settings" className="rounded-lg px-3 py-2 hover:bg-surface">{t('home.settings')}</Link>
        <button onClick={onLogout} className="rounded-lg px-3 py-2 text-left hover:bg-surface">{t('home.logout')}</button>
      </div>
    </nav>
  )
}

function Chip({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button aria-pressed={active} onClick={onClick}
      className={`inline-flex items-center gap-1.5 rounded-full border px-3 py-0.5 text-sm ${active ? 'border-accent font-semibold text-accent' : 'border-line bg-surface'}`}>
      {children}
    </button>
  )
}
