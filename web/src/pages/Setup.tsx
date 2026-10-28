import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { api, ApiError, type AccountInput, type Language, type Preferences } from '../lib/api'
import { applyLook, DEFAULT_PREFERENCES } from '../lib/look'
import { useSetMe } from '../lib/session'
import { setLanguage } from '../i18n'
import { Alert, Field, Narrow, Segmented } from '../components/ui'
import { LookPicker } from '../components/LookPicker'
import { AccountForm, EMPTY_ACCOUNT } from '../components/AccountForm'

const STEPS = ['welcome', 'admin', 'server', 'look', 'account'] as const

export default function Setup() {
  const { t, i18n } = useTranslation()
  const navigate = useNavigate()
  const setMe = useSetMe()
  const [step, setStep] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const [language, setLang] = useState<Language>((i18n.language as Language) ?? 'es')
  const [admin, setAdmin] = useState({ username: '', password: '', repeat: '', email: '' })
  const [server, setServer] = useState({
    public_url: window.location.origin,
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone ?? 'Europe/Madrid',
  })
  const [prefs, setPrefs] = useState<Preferences>(DEFAULT_PREFERENCES)
  const [account, setAccount] = useState<AccountInput>(EMPTY_ACCOUNT)

  useEffect(() => applyLook(prefs), [prefs])

  const id = STEPS[step]
  const next = (e?: React.FormEvent) => {
    e?.preventDefault()
    setError(null)
    if (id === 'admin' && admin.password !== admin.repeat) return setError(t('fields.passwordMismatch'))
    setStep((s) => s + 1)
  }

  async function finish(withAccount: boolean) {
    setBusy(true)
    setError(null)
    try {
      const me = await api.setup({
        admin: { username: admin.username, password: admin.password, email: admin.email || null },
        server: { public_url: server.public_url, language, timezone: server.timezone },
        preferences: prefs,
        account: withAccount ? account : null,
      })
      setMe(me)
      navigate('/', { replace: true })
    } catch (e) {
      setError(e instanceof ApiError ? e.message : t('common.error'))
      if (e instanceof ApiError && e.status === 400 && !withAccount) setStep(1)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Narrow>
      <ol className="flex flex-wrap gap-x-4 gap-y-1 text-sm" aria-label={t('setup.title')}>
        {STEPS.map((s, i) => (
          <li key={s} aria-current={i === step ? 'step' : undefined}
            className={i === step ? 'font-bold text-accent' : i < step ? 'text-fg' : 'text-muted'}>
            {i + 1}. {t(`setup.steps.${s}`)}
          </li>
        ))}
      </ol>
      {error && <Alert>{error}</Alert>}

      {id === 'welcome' && (
        <form onSubmit={next} className="flex flex-col gap-6">
          <h1 className="text-3xl font-bold">{t('setup.welcome.heading')}</h1>
          <p className="text-muted">{t('setup.welcome.body')}</p>
          <Segmented label={t('fields.language')} value={language}
            onChange={(l) => { setLang(l); setLanguage(l) }}
            options={(['es', 'en'] as const).map((l) => ({ value: l, label: t(`languages.${l}`) }))} />
          <button className="btn btn-primary self-start">{t('common.next')}</button>
        </form>
      )}

      {id === 'admin' && (
        <form onSubmit={next} className="flex flex-col gap-5">
          <h1 className="text-2xl font-bold">{t('setup.admin.heading')}</h1>
          <p className="text-muted">{t('setup.admin.body')}</p>
          <Field label={t('fields.username')} hint={t('fields.usernameHint')} autoComplete="username" required
            minLength={3} maxLength={32} pattern="[A-Za-z0-9._\-]+" value={admin.username}
            onChange={(e) => setAdmin({ ...admin, username: e.target.value })} />
          <Field label={t('fields.password')} hint={t('fields.passwordHint')} type="password" autoComplete="new-password"
            required minLength={10} value={admin.password} onChange={(e) => setAdmin({ ...admin, password: e.target.value })} />
          <Field label={t('fields.passwordRepeat')} type="password" autoComplete="new-password" required minLength={10}
            value={admin.repeat} onChange={(e) => setAdmin({ ...admin, repeat: e.target.value })} />
          <Field label={`${t('fields.email')} (${t('common.optional')})`} type="email" autoComplete="email"
            value={admin.email} onChange={(e) => setAdmin({ ...admin, email: e.target.value })} />
          <Nav onBack={() => setStep(0)} />
        </form>
      )}

      {id === 'server' && (
        <form onSubmit={next} className="flex flex-col gap-5">
          <h1 className="text-2xl font-bold">{t('setup.server.heading')}</h1>
          <Field label={t('setup.server.publicUrl')} hint={t('setup.server.publicUrlHint')} type="url" required
            value={server.public_url} onChange={(e) => setServer({ ...server, public_url: e.target.value })} />
          <Field label={t('setup.server.timezone')} value={server.timezone} required
            onChange={(e) => setServer({ ...server, timezone: e.target.value })} />
          <Nav onBack={() => setStep(1)} />
        </form>
      )}

      {id === 'look' && (
        <form onSubmit={next} className="flex flex-col gap-5">
          <h1 className="text-2xl font-bold">{t('setup.look.heading')}</h1>
          <p className="text-muted">{t('setup.look.body')}</p>
          <LookPicker value={prefs} onChange={(p) => setPrefs({ ...prefs, ...p })} />
          <Nav onBack={() => setStep(2)} />
        </form>
      )}

      {id === 'account' && (
        <form onSubmit={(e) => { e.preventDefault(); finish(true) }} className="flex flex-col gap-5">
          <h1 className="text-2xl font-bold">{t('setup.account.heading')}</h1>
          <p className="text-muted">{t('setup.account.body')}</p>
          <AccountForm value={account} onChange={setAccount} />
          <Alert kind="info">{t('setup.account.later')}</Alert>
          <div className="flex flex-wrap gap-2">
            <button type="button" className="btn btn-ghost" onClick={() => setStep(3)}>{t('common.back')}</button>
            <button type="button" className="btn" disabled={busy} onClick={() => finish(false)}>{t('common.skip')}</button>
            <button className="btn btn-primary" disabled={busy}>{t('setup.finish')}</button>
          </div>
        </form>
      )}
    </Narrow>
  )
}

function Nav({ onBack }: { onBack: () => void }) {
  const { t } = useTranslation()
  return (
    <div className="flex gap-2">
      <button type="button" className="btn btn-ghost" onClick={onBack}>{t('common.back')}</button>
      <button className="btn btn-primary">{t('common.next')}</button>
    </div>
  )
}
