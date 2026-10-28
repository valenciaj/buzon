import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Link, NavLink, useParams } from 'react-router'
import { api, ApiError, type AccountInput, type Identity, type IdentityInput, type Language, type Me, type Preferences } from '../lib/api'
import { applyLook } from '../lib/look'
import { useSetMe } from '../lib/session'
import { setLanguage } from '../i18n'
import { Alert, Field, Logo, Segmented } from '../components/ui'
import { LookPicker } from '../components/LookPicker'
import { TotpPanel } from '../components/TotpPanel'
import { AccountForm, EMPTY_ACCOUNT } from '../components/AccountForm'

const TABS = ['look', 'security', 'accounts', 'identities', 'invitations'] as const
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
      {current === 'identities' && <IdentitiesTab />}
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
  const { t, i18n } = useTranslation()
  const qc = useQueryClient()
  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts, refetchInterval: 5000 })
  const [draft, setDraft] = useState<AccountInput>(EMPTY_ACCOUNT)
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ['accounts'] })
    qc.invalidateQueries({ queryKey: ['mailboxes'] })
    qc.invalidateQueries({ queryKey: ['threads'] })
  }
  const add = useMutation({
    mutationFn: (verify: boolean) => api.addAccount({ ...draft, verify }),
    onSuccess: () => {
      setDraft(EMPTY_ACCOUNT)
      refresh()
    },
  })
  const remove = useMutation({ mutationFn: api.deleteAccount, onSuccess: refresh })
  const sync = useMutation({ mutationFn: api.syncAccount, onSuccess: refresh })
  const loginFailed = add.error instanceof ApiError && add.error.code === 'mail_login_failed'
  const date = (s: number) => new Date(s * 1000).toLocaleString(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })

  return (
    <section className="flex flex-col gap-6">
      {accounts.data && accounts.data.length > 0 && (
        <ul className="card divide-y divide-line">
          {accounts.data.map((a) => (
            <li key={a.id} className="flex flex-col gap-2 px-4 py-3">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                <span className="font-semibold">{a.name}</span>
                <span className={`rounded px-2 py-0.5 text-xs font-semibold bg-surface-2 ${a.state === 'error' ? 'text-crit' : a.state === 'idle' ? 'text-good' : 'text-muted'}`}>
                  {t(`account.states.${a.state}`)}
                </span>
              </div>
              <span className="text-sm text-muted [overflow-wrap:anywhere]">{a.email} · {a.imap_host}:{a.imap_port}</span>
              {a.synced_at && <span className="text-sm text-muted">{t('account.syncedAt', { date: date(a.synced_at) })}</span>}
              {a.state === 'error' && a.sync_error && <Alert>{a.sync_error}</Alert>}
              <div className="flex flex-wrap gap-2">
                <button className="btn" disabled={sync.isPending} onClick={() => sync.mutate(a.id)}>{t('account.syncNow')}</button>
                <button className="btn btn-ghost text-crit" disabled={remove.isPending}
                  onClick={() => { if (window.confirm(t('account.deleteConfirm', { name: a.name }))) remove.mutate(a.id) }}>
                  {t('account.delete')}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      <form className="card flex flex-col gap-5 p-4 sm:p-6" onSubmit={(e) => { e.preventDefault(); add.mutate(true) }}>
        <h2 className="text-lg font-bold">{t('account.add')}</h2>
        <AccountForm value={draft} onChange={(a) => { setDraft(a); add.reset() }} />
        {add.isPending && <Alert kind="info">{t('account.checking')}</Alert>}
        {add.isError && <Alert>{add.error instanceof ApiError ? add.error.message : t('common.error')}</Alert>}
        {add.isSuccess && <Alert kind="ok">{t('account.added')}</Alert>}
        <div className="flex flex-wrap gap-2">
          <button className="btn btn-primary" disabled={add.isPending}>{t('account.add')}</button>
          {loginFailed && (
            <button type="button" className="btn" disabled={add.isPending} onClick={() => add.mutate(false)}>
              {t('account.saveAnyway')}
            </button>
          )}
        </div>
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

/** Direcciones desde las que se escribe (la de cada cuenta y sus alias) y sus firmas. */
function IdentitiesTab() {
  const { t } = useTranslation()
  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts })
  const identities = useQuery({ queryKey: ['identities'], queryFn: api.identities })
  const [adding, setAdding] = useState<string | null>(null)

  if (!accounts.data || !identities.data) return <p className="text-muted">{t('common.loading')}</p>
  if (accounts.data.length === 0) return <Alert kind="info">{t('identity.noAccounts')}</Alert>

  return (
    <section className="flex flex-col gap-6">
      <p className="text-muted">{t('identity.intro')}</p>
      {accounts.data.map((a) => (
        <div key={a.id} className="flex flex-col gap-3">
          <h2 className="text-lg font-bold">{a.name}</h2>
          <ul className="flex flex-col gap-3">
            {identities.data.filter((i) => i.account_id === a.id).map((i) => (
              <li key={i.id}><IdentityCard identity={i} /></li>
            ))}
          </ul>
          {adding === a.id ? (
            <IdentityCard account={a.id} onDone={() => setAdding(null)} />
          ) : (
            <button className="btn self-start" onClick={() => setAdding(a.id)}>{t('identity.add')}</button>
          )}
        </div>
      ))}
    </section>
  )
}

function IdentityCard({ identity, account, onDone }: { identity?: Identity; account?: string; onDone?: () => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [value, setValue] = useState<IdentityInput>({
    account_id: identity?.account_id ?? account,
    name: identity?.name ?? '',
    email: identity?.email ?? '',
    signature: identity?.signature ?? '',
    is_default: identity?.is_default ?? false,
  })
  const [done, setDone] = useState(false)
  const refresh = () => qc.invalidateQueries({ queryKey: ['identities'] })
  const save = useMutation({
    mutationFn: (v: IdentityInput) => (identity ? api.updateIdentity(identity.id, v) : api.createIdentity(v)),
    onSuccess: () => { refresh(); setDone(true); onDone?.() },
  })
  const remove = useMutation({ mutationFn: () => api.deleteIdentity(identity!.id), onSuccess: refresh })
  const set = (c: Partial<IdentityInput>) => { setValue({ ...value, ...c }); setDone(false); save.reset() }
  const sigId = `sig-${identity?.id ?? 'new'}`

  return (
    <form className="card flex flex-col gap-4 p-4" onSubmit={(e) => { e.preventDefault(); save.mutate(value) }}>
      <div className="flex flex-wrap items-center gap-2">
        <b className="min-w-0 flex-1 [overflow-wrap:anywhere]">{identity ? identity.email : t('identity.new')}</b>
        {identity?.is_default && <span className="rounded bg-surface-2 px-2 py-0.5 text-xs font-semibold text-good">{t('identity.default')}</span>}
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label={t('identity.name')} value={value.name} onChange={(e) => set({ name: e.target.value })} hint={t('identity.nameHint')} />
        <Field label={t('identity.email')} type="email" required value={value.email} onChange={(e) => set({ email: e.target.value })} />
      </div>
      <div className="flex flex-col gap-1.5">
        <label htmlFor={sigId} className="label">{t('identity.signature')}</label>
        <textarea id={sigId} className="input min-h-24" rows={3} value={value.signature}
          onChange={(e) => set({ signature: e.target.value })} />
        <p className="hint">{t('identity.signatureHint')}</p>
      </div>
      {!identity?.is_default && (
        <label className="flex items-center gap-2">
          <input type="checkbox" className="size-4 accent-[var(--accent)]" checked={value.is_default}
            onChange={(e) => set({ is_default: e.target.checked })} />
          {t('identity.makeDefault')}
        </label>
      )}
      {save.isError && <Alert>{save.error instanceof ApiError ? save.error.message : t('common.error')}</Alert>}
      {remove.isError && <Alert>{remove.error instanceof ApiError ? remove.error.message : t('common.error')}</Alert>}
      {done && identity && <Alert kind="ok">{t('identity.saved')}</Alert>}
      <div className="flex flex-wrap gap-2">
        <button className="btn btn-primary" disabled={save.isPending}>{t('identity.save')}</button>
        {identity ? (
          <button type="button" className="btn btn-ghost text-crit" disabled={remove.isPending}
            onClick={() => { if (window.confirm(t('identity.deleteConfirm', { email: identity.email }))) remove.mutate() }}>
            {t('identity.delete')}
          </button>
        ) : (
          <button type="button" className="btn btn-ghost" onClick={onDone}>{t('common.cancel')}</button>
        )}
      </div>
    </form>
  )
}
