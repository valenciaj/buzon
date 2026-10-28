import { useTranslation } from 'react-i18next'
import { useId } from 'react'
import type { AccountInput, Security } from '../lib/api'
import { Field } from './ui'

export const EMPTY_ACCOUNT: AccountInput = {
  name: '', email: '', imap_host: '', imap_port: 993, imap_security: 'tls',
  smtp_host: '', smtp_port: 465, smtp_security: 'tls', login: '', password: '',
}

const PORTS: Record<'imap' | 'smtp', Record<Security, number>> = {
  imap: { tls: 993, starttls: 143, none: 143 },
  smtp: { tls: 465, starttls: 587, none: 25 },
}

/** Cambia el cifrado y, si el puerto era el habitual del anterior, también el puerto. */
export function withSecurity(a: AccountInput, kind: 'imap' | 'smtp', security: Security): AccountInput {
  const portKey = `${kind}_port` as const
  const secKey = `${kind}_security` as const
  const usual = Object.values(PORTS[kind]).includes(a[portKey])
  return { ...a, [secKey]: security, [portKey]: usual ? PORTS[kind][security] : a[portKey] }
}

/** Sugiere servidores y usuario a partir de la dirección (imap./smtp. del dominio). */
export function guessFromEmail(a: AccountInput, email: string): AccountInput {
  const domain = email.split('@')[1] ?? ''
  return {
    ...a,
    email,
    login: !a.login || a.login === a.email ? email : a.login,
    imap_host: !a.imap_host || a.imap_host === `imap.${a.email.split('@')[1] ?? ''}` ? (domain ? `imap.${domain}` : '') : a.imap_host,
    smtp_host: !a.smtp_host || a.smtp_host === `smtp.${a.email.split('@')[1] ?? ''}` ? (domain ? `smtp.${domain}` : '') : a.smtp_host,
  }
}

function SecuritySelect({ kind, value, onChange }: {
  kind: 'imap' | 'smtp'
  value: AccountInput
  onChange: (a: AccountInput) => void
}) {
  const { t } = useTranslation()
  const id = useId()
  return (
    <div className="flex flex-col gap-1.5 sm:col-span-2">
      <label htmlFor={id} className="label">{t(`account.${kind}Security`)}</label>
      <select id={id} className="input" value={value[`${kind}_security`]}
        onChange={(e) => onChange(withSecurity(value, kind, e.target.value as Security))}>
        {(['tls', 'starttls', 'none'] as const).map((s) => (
          <option key={s} value={s}>{t(`account.security.${s}`)}</option>
        ))}
      </select>
    </div>
  )
}

export function AccountForm({ value, onChange }: { value: AccountInput; onChange: (a: AccountInput) => void }) {
  const { t } = useTranslation()
  const set = (k: keyof AccountInput) => (e: React.ChangeEvent<HTMLInputElement>) =>
    onChange({ ...value, [k]: e.target.type === 'number' ? Number(e.target.value) : e.target.value })
  return (
    <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_7rem]">
      <div className="sm:col-span-2">
        <Field label={t('account.name')} placeholder={t('account.namePlaceholder')} value={value.name} onChange={set('name')} required />
      </div>
      <div className="sm:col-span-2">
        <Field label={t('account.email')} type="email" autoComplete="off" value={value.email}
          onChange={(e) => onChange(guessFromEmail(value, e.target.value))} required />
      </div>
      <Field label={t('account.imapHost')} value={value.imap_host} onChange={set('imap_host')} required />
      <Field label={t('account.imapPort')} type="number" min={1} max={65535} value={value.imap_port} onChange={set('imap_port')} required />
      <SecuritySelect kind="imap" value={value} onChange={onChange} />
      <Field label={t('account.smtpHost')} value={value.smtp_host} onChange={set('smtp_host')} required />
      <Field label={t('account.smtpPort')} type="number" min={1} max={65535} value={value.smtp_port} onChange={set('smtp_port')} required />
      <SecuritySelect kind="smtp" value={value} onChange={onChange} />
      <div className="sm:col-span-2">
        <Field label={t('account.login')} autoComplete="off" value={value.login} onChange={set('login')} required />
      </div>
      <div className="sm:col-span-2">
        <Field label={t('account.password')} type="password" autoComplete="new-password" value={value.password} onChange={set('password')} required />
      </div>
    </div>
  )
}
