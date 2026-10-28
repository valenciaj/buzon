import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import {
  api, ApiError, HOOK_EVENTS, SCOPES,
  type ApiToken, type Delivery, type HookEvent, type Scope, type Webhook,
} from '../lib/api'
import { Alert, Field } from './ui'

const EXPIRY = [0, 30, 90, 365] as const

function errorText(e: unknown, fallback: string) {
  return e instanceof ApiError ? e.message : fallback
}

function useDate() {
  const { i18n } = useTranslation()
  return (s: number) => new Date(s * 1000).toLocaleString(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })
}

/** Un secreto que solo se enseña una vez, con su botón de copiar. */
function OneTimeSecret({ label, value }: { label: string; value: string }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)
  async function copy() {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(true)
    } catch {
      setCopied(false)
    }
  }
  return (
    <div className="flex flex-col gap-2">
      <Alert kind="ok">{t('api.copyNow')}</Alert>
      <div className="flex gap-2">
        <input className="input font-mono text-sm" readOnly value={value} aria-label={label} onFocus={(e) => e.target.select()} />
        <button type="button" className="btn" onClick={copy}>{copied ? t('common.copied') : t('common.copy')}</button>
      </div>
    </div>
  )
}

function Checks<T extends string>({ legend, options, value, onChange, labelOf }: {
  legend: string
  options: readonly T[]
  value: T[]
  onChange: (v: T[]) => void
  labelOf: (o: T) => string
}) {
  return (
    <fieldset className="flex flex-col gap-2">
      <legend className="label mb-1.5">{legend}</legend>
      {options.map((o) => (
        <label key={o} className="flex items-start gap-2">
          <input type="checkbox" className="mt-1 size-4 shrink-0 accent-[var(--accent)]" checked={value.includes(o)}
            onChange={(e) => onChange(e.target.checked ? [...value, o] : value.filter((x) => x !== o))} />
          <span><code className="text-sm">{o}</code> <span className="text-muted">· {labelOf(o)}</span></span>
        </label>
      ))}
    </fieldset>
  )
}

/** Ajustes → API: tokens personales y webhooks. */
export function ApiPanel() {
  const { t } = useTranslation()
  return (
    <section className="flex flex-col gap-8">
      <p className="text-muted">
        {t('api.intro')}{' '}
        <a className="font-semibold text-accent underline" href="/api/docs" target="_blank" rel="noreferrer">{t('api.docs')}</a>
      </p>
      <Tokens />
      <Webhooks />
    </section>
  )
}

function Tokens() {
  const { t } = useTranslation()
  const date = useDate()
  const qc = useQueryClient()
  const list = useQuery({ queryKey: ['tokens'], queryFn: api.tokens })
  const [name, setName] = useState('')
  const [scopes, setScopes] = useState<Scope[]>(['mail:read'])
  const [days, setDays] = useState<number>(90)
  const refresh = () => qc.invalidateQueries({ queryKey: ['tokens'] })
  const create = useMutation({
    mutationFn: () => api.createToken(name.trim(), scopes, days || undefined),
    onSuccess: () => { setName(''); refresh() },
  })
  const revoke = useMutation({ mutationFn: api.revokeToken, onSuccess: refresh })

  return (
    <div className="flex flex-col gap-4">
      <h2 className="text-xl font-bold">{t('api.tokens.title')}</h2>
      <form className="card flex flex-col gap-4 p-4 sm:p-6" onSubmit={(e) => { e.preventDefault(); create.mutate() }}>
        <p className="text-muted">{t('api.tokens.body')}</p>
        <Field label={t('api.tokens.name')} placeholder={t('api.tokens.namePlaceholder')} required maxLength={60}
          value={name} onChange={(e) => { setName(e.target.value); create.reset() }} />
        <Checks legend={t('api.tokens.scopes')} options={SCOPES} value={scopes} onChange={setScopes}
          labelOf={(s) => t(`api.scopes.${s.replace(':', '_')}`)} />
        <div className="flex flex-col gap-1.5">
          <label htmlFor="token-expiry" className="label">{t('api.tokens.expiry')}</label>
          <select id="token-expiry" className="input sm:max-w-xs" value={days} onChange={(e) => setDays(Number(e.target.value))}>
            {EXPIRY.map((d) => <option key={d} value={d}>{d ? t('api.tokens.days', { count: d }) : t('api.tokens.never')}</option>)}
          </select>
        </div>
        {create.isError && <Alert>{errorText(create.error, t('common.error'))}</Alert>}
        {create.data && <OneTimeSecret label={t('api.tokens.token')} value={create.data.token} />}
        <button className="btn btn-primary self-start" disabled={create.isPending || scopes.length === 0 || !name.trim()}>
          {t('api.tokens.create')}
        </button>
      </form>
      {revoke.isError && <Alert>{errorText(revoke.error, t('common.error'))}</Alert>}
      {list.data?.length === 0 && <p className="text-muted">{t('api.tokens.none')}</p>}
      {list.data && list.data.length > 0 && (
        <ul className="card divide-y divide-line">
          {list.data.map((k: ApiToken) => (
            <li key={k.id} className="flex flex-col gap-1.5 px-4 py-3">
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                <b className="min-w-0 [overflow-wrap:anywhere]">{k.name}</b>
                <code className="text-sm text-muted">{k.prefix}…</code>
                <button className="btn btn-ghost ml-auto px-2.5 text-crit" disabled={revoke.isPending}
                  onClick={() => { if (window.confirm(t('api.tokens.revokeConfirm', { name: k.name }))) revoke.mutate(k.id) }}>
                  {t('api.tokens.revoke')}
                </button>
              </div>
              <div className="flex flex-wrap gap-1.5">
                {k.scopes.map((s) => <span key={s} className="rounded bg-surface-2 px-2 py-0.5 font-mono text-xs">{s}</span>)}
              </div>
              <span className="text-sm text-muted">
                {k.last_used_at ? t('api.tokens.used', { date: date(k.last_used_at) }) : t('api.tokens.unused')}
                {' · '}
                {k.expires_at ? t('api.tokens.expires', { date: date(k.expires_at) }) : t('api.tokens.noExpiry')}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function Webhooks() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const list = useQuery({ queryKey: ['webhooks'], queryFn: api.webhooks })
  const [url, setUrl] = useState('')
  const [description, setDescription] = useState('')
  const [events, setEvents] = useState<HookEvent[]>(['message.received'])
  const create = useMutation({
    mutationFn: () => api.createWebhook({ url: url.trim(), description, events }),
    onSuccess: () => { setUrl(''); setDescription(''); qc.invalidateQueries({ queryKey: ['webhooks'] }) },
  })

  return (
    <div className="flex flex-col gap-4">
      <h2 className="text-xl font-bold">{t('api.hooks.title')}</h2>
      <form className="card flex flex-col gap-4 p-4 sm:p-6" onSubmit={(e) => { e.preventDefault(); create.mutate() }}>
        <p className="text-muted">{t('api.hooks.body')}</p>
        <Field label={t('api.hooks.url')} type="url" inputMode="url" required placeholder="https://"
          value={url} onChange={(e) => { setUrl(e.target.value); create.reset() }} />
        <Field label={`${t('api.hooks.description')} (${t('common.optional')})`} maxLength={200}
          placeholder={t('api.hooks.descriptionPlaceholder')} value={description} onChange={(e) => setDescription(e.target.value)} />
        <Checks legend={t('api.hooks.events')} options={HOOK_EVENTS} value={events} onChange={setEvents}
          labelOf={(e) => t(`api.events.${e.replace('.', '_')}`)} />
        {create.isError && <Alert>{errorText(create.error, t('common.error'))}</Alert>}
        {create.data && <OneTimeSecret label={t('api.hooks.secret')} value={create.data.secret} />}
        <button className="btn btn-primary self-start" disabled={create.isPending || events.length === 0 || !url.trim()}>
          {t('api.hooks.create')}
        </button>
      </form>
      {list.data?.length === 0 && <p className="text-muted">{t('api.hooks.none')}</p>}
      {list.data?.map((w) => <WebhookCard key={w.id} hook={w} />)}
    </div>
  )
}

const STATUS_TONE = { ok: 'text-good', failed: 'text-crit', pending: 'text-muted' } as const

function WebhookCard({ hook: w }: { hook: Webhook }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [open, setOpen] = useState(false)
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ['webhooks'] })
    qc.invalidateQueries({ queryKey: ['deliveries', w.id] })
  }
  const toggle = useMutation({ mutationFn: () => api.updateWebhook(w.id, { active: !w.active }), onSuccess: refresh })
  const test = useMutation({ mutationFn: () => api.testWebhook(w.id), onSuccess: () => { setOpen(true); refresh() } })
  const remove = useMutation({ mutationFn: () => api.deleteWebhook(w.id), onSuccess: refresh })
  const failed = [toggle, test, remove].find((m) => m.isError)?.error

  return (
    <div className="card flex flex-col gap-3 p-4">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <b className="min-w-0 flex-1 font-mono text-sm [overflow-wrap:anywhere]">{w.url}</b>
        {!w.active && <span className="rounded bg-surface-2 px-2 py-0.5 text-xs font-semibold text-muted">{t('api.hooks.paused')}</span>}
        {w.last_status && (
          <span className={`rounded bg-surface-2 px-2 py-0.5 text-xs font-semibold ${STATUS_TONE[w.last_status]}`}>
            {t(`api.status.${w.last_status}`)}
          </span>
        )}
      </div>
      {w.description && <span className="text-muted [overflow-wrap:anywhere]">{w.description}</span>}
      <div className="flex flex-wrap gap-1.5">
        {w.events.map((e) => <span key={e} className="rounded bg-surface-2 px-2 py-0.5 font-mono text-xs">{e}</span>)}
      </div>
      {failed && <Alert>{errorText(failed, t('common.error'))}</Alert>}
      <div className="flex flex-wrap gap-2">
        <button className="btn" disabled={test.isPending || !w.active} onClick={() => test.mutate()}>{t('api.hooks.test')}</button>
        <button className="btn" aria-expanded={open} onClick={() => setOpen(!open)}>{t('api.hooks.history')}</button>
        <button className="btn btn-ghost" disabled={toggle.isPending} onClick={() => toggle.mutate()}>
          {w.active ? t('api.hooks.pause') : t('api.hooks.resume')}
        </button>
        <button className="btn btn-ghost text-crit" disabled={remove.isPending}
          onClick={() => { if (window.confirm(t('api.hooks.deleteConfirm'))) remove.mutate() }}>
          {t('api.hooks.delete')}
        </button>
      </div>
      {open && <Deliveries hook={w.id} />}
    </div>
  )
}

function Deliveries({ hook }: { hook: string }) {
  const { t } = useTranslation()
  const date = useDate()
  const qc = useQueryClient()
  const list = useQuery({
    queryKey: ['deliveries', hook],
    queryFn: () => api.deliveries(hook),
    // Mientras haya algo por enviar, se mira a menudo.
    refetchInterval: (q) => (q.state.data?.some((d) => d.status === 'pending') ? 2000 : 15000),
  })
  const again = useMutation({
    mutationFn: api.redeliver,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['deliveries', hook] })
      qc.invalidateQueries({ queryKey: ['webhooks'] })
    },
  })

  if (list.isLoading) return <p className="text-muted">{t('common.loading')}</p>
  if (list.isError) return <Alert>{t('common.error')}</Alert>
  if (!list.data?.length) return <p className="text-muted">{t('api.hooks.noDeliveries')}</p>
  return (
    <ul className="flex flex-col divide-y divide-line border-t border-line" aria-label={t('api.hooks.history')}>
      {list.data.map((d: Delivery) => (
        <li key={d.id} className="flex flex-col gap-1 py-2.5">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <code className="text-sm">{d.event}</code>
            <span className={`text-sm font-semibold ${STATUS_TONE[d.status]}`}>{t(`api.status.${d.status}`)}</span>
            {d.response !== null && <span className="text-sm text-muted tabular-nums">HTTP {d.response}</span>}
            <span className="ml-auto text-sm text-muted tabular-nums">{date(d.created_at)}</span>
          </div>
          <span className="text-sm text-muted">
            {t('api.hooks.attempts', { count: d.attempts })}
            {d.status === 'pending' && d.next_at && d.attempts > 0 && <> · {t('api.hooks.nextAt', { date: date(d.next_at) })}</>}
          </span>
          {d.error && <span className="text-sm text-crit [overflow-wrap:anywhere]">{d.error}</span>}
          <div className="flex flex-wrap items-center gap-2">
            <details className="min-w-0 flex-1">
              <summary className="cursor-pointer text-sm text-muted">{t('api.hooks.payload')}</summary>
              <pre className="mt-1 max-h-60 overflow-auto rounded-lg bg-surface-2 p-2 text-xs">{pretty(d.payload)}</pre>
            </details>
            {d.status !== 'pending' && (
              <button className="btn btn-ghost px-2.5 text-sm" disabled={again.isPending} onClick={() => again.mutate(d.id)}>
                {t('api.hooks.redeliver')}
              </button>
            )}
          </div>
        </li>
      ))}
    </ul>
  )
}

function pretty(json: string) {
  try {
    return JSON.stringify(JSON.parse(json), null, 2)
  } catch {
    return json
  }
}
