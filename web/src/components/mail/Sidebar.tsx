import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import type { Account, Mailbox, Me, Role } from '../../lib/api'
import { ACCOUNT_COLORS, boxKey, customFolders, ROLE_ORDER, unreadByRole, type Box } from '../../lib/mail'
import { Logo } from '../ui'
import { Icon } from './Icons'

/** En Enfoque se ven pocas bandejas; el resto queda en «Más carpetas». */
const FOCUS_MAIN: Role[] = ['inbox', 'sent', 'drafts']

export function Sidebar({ me, accounts, mailboxes, account, box, onAccount, onBox, onLogout }: {
  me: Me
  accounts: Account[]
  mailboxes: Mailbox[]
  /** Cuenta seleccionada; sin ella, todas combinadas. */
  account?: string
  box: Box
  onAccount: (id: string | null) => void
  onBox: (b: Box) => void
  onLogout: () => void
}) {
  const { t } = useTranslation()
  const focus = me.preferences.mode === 'focus'
  const [more, setMore] = useState(false)
  const unread = unreadByRole(mailboxes, account)
  const present = new Set(mailboxes.filter((m) => !account || m.account_id === account).map((m) => m.role))
  const roles = ROLE_ORDER.filter((r) => r === 'inbox' || present.has(r))
  const main = focus ? roles.filter((r) => FOCUS_MAIN.includes(r)) : roles
  const extraRoles = focus ? roles.filter((r) => !FOCUS_MAIN.includes(r)) : []
  const folders = customFolders(mailboxes, account)
  const current = boxKey(box)
  const color = (id: string) => ACCOUNT_COLORS[accounts.findIndex((a) => a.id === id) % ACCOUNT_COLORS.length]
  const roleLabel = (r: Role) => t(`mail.roles.${focus && r === 'inbox' ? 'principal' : r}`)
  const showMore = !focus || more || extraRoles.some((r) => boxKey({ role: r }) === current)
    || folders.some((f) => boxKey({ mailbox: f.id }) === current)

  const item = (key: string, label: string, icon: string, count: number, onClick: () => void, dot?: string) => (
    <li key={key}>
      <button aria-current={current === key} onClick={onClick}
        className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-surface ${current === key ? 'bg-surface font-semibold shadow-[inset_3px_0_0_var(--accent)]' : ''}`}>
        <Icon name={icon} className="shrink-0 text-muted" />
        <span className="min-w-0 flex-1 truncate">{label}</span>
        {dot && <span className="size-2 shrink-0 rounded-full" style={{ background: dot }} />}
        {count > 0 && <span className={`text-sm tabular-nums ${key === 'inbox' ? 'font-bold text-accent' : 'text-muted'}`}>{count}</span>}
      </button>
    </li>
  )

  const errors = accounts.filter((a) => a.state === 'error')
  const syncing = accounts.some((a) => a.state === 'connecting' || a.state === 'syncing')

  return (
    <nav className="flex h-full flex-col gap-5 overflow-y-auto p-4" aria-label={t('home.menu')}>
      <Logo />
      {accounts.length > 0 && (
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
      <ul className="flex flex-col gap-0.5">
        {main.map((r) => item(r, roleLabel(r), r, r === 'inbox' || r === 'junk' || !focus ? unread[r] ?? 0 : 0, () => onBox({ role: r })))}
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
        <p className="px-3 text-sm text-muted">
          {t(`look.modes.${me.preferences.mode}`)} · {me.username}
        </p>
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
