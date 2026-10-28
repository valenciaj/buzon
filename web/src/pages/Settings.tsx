import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Link, NavLink, useParams } from 'react-router'
import { api, ApiError, type AccountInput, type Language, type Me, type Preferences } from '../lib/api'
import { applyLook } from '../lib/look'
import { useSetMe } from '../lib/session'
import { setLanguage } from '../i18n'
import { Alert, Field, Logo, Segmented } from '../components/ui'
import { LookPicker } from '../components/LookPicker'
import { TotpPanel } from '../components/TotpPanel'
import { AccountForm, EMPTY_ACCOUNT } from '../components/AccountForm'

const TABS = ['look', 'security', 'accounts', 'invitations'] as const
type Tab = (typeof TABS)[number]

export default function Settings({ me }: { me: Me }) {
  const { t } = useTranslation()
  const { tab = 'look' } = useParams()
  const tabs = TABS.filter((x) => x !== 'invitations' || me.is_admin)
  const current: Tab = (tabs as readonly string[]).includes(tab) ? (tab as Tab) : 'look'

  return (
    <div className="mx-auto flex min-h-full max-w-3xl flex-col gap-6 px-4 py-6 sm:py-10">
      <div className="flex items-center justify-between gap-4">
        <Logo />
        <Link to="/" className="btn btn-ghost">{t('settings.back')}</Link>
      </div>
      <h1 className="text-3xl font-bold">{t('settings.title')}</h1>
      <nav className="-mx-4 flex gap-1 overflow-x-auto border-b border-line px-4">
        {tabs.map((x) => (
          <NavLink key={x} to={`/settings/${x}`}
            className={`whitespace-nowrap border-b-2 px-3 py-2 ${current === x ? 'border-accent font-semibold' : 'border-transparent text-muted'}`}>
            {t(`settings.tabs.${x}`)}
          </NavLink>
        ))}
      </nav>
      {current === 'look' && <LookTab me={me} />}
      {current === 'security' && <SecurityTab me={me} />}
      {current === 'accounts' && <AccountsTab />}
      {current === 'invitations' && me.is_admin && <InvitationsTab />}
    </div>
  )
}

function LookTab({ me }: { me: Me }) {
  const { t } = useTranslation()
  const setMe = useSetMe()
  const [error, setError] = useState<string | null>(null)

  async function save(change: Partial<Preferences> & { language?: Language }) {
    setError(null)
    const optimistic: Me = {
      ...me,
      language: change.language ?? me.language,
      preferences: { ...me.preferences, ...change },
    }
    applyLook(optimistic.preferences)
    if (change.language) setLanguage(change.language)
    setMe(optimistic)
    try {
      setMe(await api.updatePreferences(change))
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t('common.error'))
      setMe(me)
      applyLook(me.preferences)
    }
  }

  return (
    <section className="flex flex-col gap-6">
      {error && <Alert>{error}</Alert>}
      <Segmented label={t('fields.language')} value={me.language} onChange={(language) => save({ language })}
        options={(['es', 'en'] as const).map((l) => ({ value: l, label: t(`languages.${l}`) }))} />
      <LookPicker value={me.preferences} onChange={save} />
    </section>
  )
}

function SecurityTab({ me }: { me: Me }) {
  const setMe = useSetMe()
  return <TotpPanel me={me} onChange={setMe} />
}

function AccountsTab() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts })
  const [draft, setDraft] = useState<AccountInput>(EMPTY_ACCOUNT)
  const add = useMutation({
    mutationFn: api.addAccount,
    onSuccess: () => {
      setDraft(EMPTY_ACCOUNT)
      qc.invalidateQueries({ queryKey: ['accounts'] })
    },
  })

  return (
    <section className="flex flex-col gap-6">
      {accounts.data && accounts.data.length > 0 && (
        <ul className="card divide-y divide-line">
          {accounts.data.map((a) => (
            <li key={a.id} className="flex flex-col gap-0.5 px-4 py-3">
              <span className="font-semibold">{a.name}</span>
              <span className="text-sm text-muted [overflow-wrap:anywhere]">{a.email} · {a.imap_host}:{a.imap_port}</span>
            </li>
          ))}
        </ul>
      )}
      <form className="card flex flex-col gap-5 p-4 sm:p-6" onSubmit={(e) => { e.preventDefault(); add.mutate(draft) }}>
        <h2 className="text-lg font-bold">{t('account.add')}</h2>
        {add.isError && <Alert>{add.error instanceof ApiError ? add.error.message : t('common.error')}</Alert>}
        {add.isSuccess && <Alert kind="ok">{t('account.added')}</Alert>}
        <AccountForm value={draft} onChange={setDraft} />
        <Alert kind="info">{t('setup.account.later')}</Alert>
        <button className="btn btn-primary self-start" disabled={add.isPending}>{t('account.add')}</button>
      </form>
    </section>
  )
}

function InvitationsTab() {
  const { t, i18n } = useTranslation()
  const qc = useQueryClient()
  const list = useQuery({ queryKey: ['invitations'], queryFn: api.invitations })
  const [note, setNote] = useState('')
  const [copied, setCopied] = useState(false)
  const create = useMutation({
    mutationFn: () => api.createInvitation(note || undefined),
    onSuccess: () => {
      setNote('')
      setCopied(false)
      qc.invalidateQueries({ queryKey: ['invitations'] })
    },
  })
  const revoke = useMutation({
    mutationFn: api.revokeInvitation,
    onSuccess: () => qc.invalidateQueries({ queryKey: ['invitations'] }),
  })
  const date = (s: number) => new Date(s * 1000).toLocaleString(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })

  async function copy(url: string) {
    try {
      await navigator.clipboard.writeText(url)
      setCopied(true)
    } catch {
      setCopied(false)
    }
  }

  return (
    <section className="flex flex-col gap-6">
      <form className="card flex flex-col gap-4 p-4 sm:p-6" onSubmit={(e) => { e.preventDefault(); create.mutate() }}>
        <h2 className="text-lg font-bold">{t('invitations.title')}</h2>
        <p className="text-muted">{t('invitations.body')}</p>
        <Field label={`${t('invitations.note')} (${t('common.optional')})`} placeholder={t('invitations.notePlaceholder')}
          value={note} onChange={(e) => setNote(e.target.value)} />
        <button className="btn btn-primary self-start" disabled={create.isPending}>{t('invitations.create')}</button>
        {create.data && (
          <div className="flex flex-col gap-2">
            <Alert kind="ok">{t('invitations.created')}</Alert>
            <div className="flex gap-2">
              <input className="input font-mono text-sm" readOnly value={create.data.url} onFocus={(e) => e.target.select()} aria-label="URL" />
              <button type="button" className="btn" onClick={() => copy(create.data.url)}>
                {copied ? t('common.copied') : t('common.copy')}
              </button>
            </div>
          </div>
        )}
      </form>
      <div className="flex flex-col gap-3">
        <h2 className="text-lg font-bold">{t('invitations.list')}</h2>
        {list.data?.length === 0 && <p className="text-muted">{t('invitations.none')}</p>}
        {list.data && list.data.length > 0 && (
          <ul className="card divide-y divide-line">
            {list.data.map((i) => (
              <li key={i.id} className="flex flex-wrap items-center gap-x-4 gap-y-1 px-4 py-3">
                <span className="min-w-0 flex-1 [overflow-wrap:anywhere]">{i.note ?? '—'}</span>
                <span className={`rounded px-2 py-0.5 text-xs font-semibold ${i.status === 'pending' ? 'bg-surface-2 text-accent' : 'bg-surface-2 text-muted'}`}>
                  {t(`invitations.status.${i.status}`)}
                </span>
                <span className="text-sm text-muted tabular-nums">{t('invitations.expires', { date: date(i.expires_at) })}</span>
                {i.status === 'pending' && (
                  <button className="btn btn-ghost text-crit" onClick={() => revoke.mutate(i.id)}>{t('invitations.revoke')}</button>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  )
}
