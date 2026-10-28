import { useTranslation } from 'react-i18next'
import type { AccountInput } from '../lib/api'
import { Field } from './ui'

export const EMPTY_ACCOUNT: AccountInput = {
  name: '', email: '', imap_host: '', imap_port: 993, smtp_host: '', smtp_port: 465, login: '', password: '',
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
      <Field label={t('account.smtpHost')} value={value.smtp_host} onChange={set('smtp_host')} required />
      <Field label={t('account.smtpPort')} type="number" min={1} max={65535} value={value.smtp_port} onChange={set('smtp_port')} required />
      <div className="sm:col-span-2">
        <Field label={t('account.login')} autoComplete="off" value={value.login} onChange={set('login')} required />
      </div>
      <div className="sm:col-span-2">
        <Field label={t('account.password')} type="password" autoComplete="new-password" value={value.password} onChange={set('password')} required />
      </div>
    </div>
  )
}
