import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from 'react-router'
import { api, ApiError, type Contact, type ContactInput } from '../lib/api'
import { initials } from '../lib/mail'
import { Alert, Field, Logo } from '../components/ui'
import { Icon } from '../components/mail/Icons'
import { useToast } from '../components/Toaster'

const EMPTY: ContactInput = { name: '', email: '', phone: '', notes: '' }

/** La libreta de direcciones: lo que se añade a mano, lo importado y a quien se ha escrito. */
export default function Contacts() {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const toast = useToast()
  const [typed, setTyped] = useState('')
  const [term, setTerm] = useState('')
  const [adding, setAdding] = useState(false)
  const [editing, setEditing] = useState<string | null>(null)
  const file = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const h = setTimeout(() => setTerm(typed.trim()), 200)
    return () => clearTimeout(h)
  }, [typed])
  const list = useQuery({ queryKey: ['book', term], queryFn: () => api.contacts(term || undefined) })
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ['book'] })
    qc.invalidateQueries({ queryKey: ['contacts'] })
  }

  async function importFile(f: File) {
    try {
      const r = await api.importContacts(await f.text())
      toast({ text: t('contacts.imported', r) })
      refresh()
    } catch (e) {
      toast({ text: e instanceof ApiError ? e.message : t('common.error') })
    }
  }

  return (
    <div className="mx-auto flex min-h-full max-w-3xl flex-col gap-6 px-4 py-6 sm:py-10">
      <div className="flex items-center justify-between gap-4">
        <Logo />
        <Link to="/" className="btn btn-ghost">{t('settings.back')}</Link>
      </div>
      <h1 className="text-3xl font-bold">{t('contacts.title')}</h1>
      <div className="flex flex-wrap gap-2">
        <div className="relative min-w-[12rem] flex-1">
          <Icon name="search" className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
          <input type="search" className="input pl-10" placeholder={t('contacts.search')} aria-label={t('contacts.search')}
            value={typed} onChange={(e) => setTyped(e.target.value)} />
        </div>
        <button className="btn" onClick={() => file.current?.click()}>{t('contacts.import')}</button>
        <input ref={file} type="file" accept=".vcf,text/vcard,text/x-vcard" className="hidden"
          onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ''; if (f) importFile(f) }} />
        <button className="btn btn-primary" onClick={() => { setAdding(true); setEditing(null) }}>{t('contacts.add')}</button>
      </div>
      {adding && <ContactForm onDone={() => { setAdding(false); refresh() }} onCancel={() => setAdding(false)} />}
      {list.isError && <Alert>{t('common.error')}</Alert>}
      {list.isSuccess && list.data.length === 0 && !adding && (
        <div className="card px-6 py-10 text-center text-muted">
          <b className="mb-1 block text-fg">{t('contacts.none')}</b>
          {t('contacts.noneBody')}
        </div>
      )}
      {list.data && list.data.length > 0 && (
        <ul className="card divide-y divide-line">
          {list.data.map((c) => (
            <li key={c.id}>
              {editing === c.id
                ? <div className="p-2"><ContactForm contact={c} onDone={() => { setEditing(null); refresh() }} onCancel={() => setEditing(null)} /></div>
                : <ContactRow contact={c} onEdit={() => { setEditing(c.id); setAdding(false) }} onDeleted={refresh} />}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function ContactRow({ contact: c, onEdit, onDeleted }: { contact: Contact; onEdit: () => void; onDeleted: () => void }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const remove = useMutation({ mutationFn: () => api.deleteContact(c.id), onSuccess: onDeleted })
  const name = c.name || c.email
  return (
    <div className="flex flex-wrap items-center gap-3 px-4 py-3">
      <span className="grid size-10 shrink-0 place-items-center rounded-full bg-surface-2 text-sm font-bold" aria-hidden="true">{initials(name)}</span>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate font-semibold">{name}</span>
        <span className="truncate text-sm text-muted">
          {c.email}{c.phone && <> · {c.phone}</>} · {t(`contacts.sources.${c.source}`)}
        </span>
        {c.notes && <span className="truncate text-sm text-muted">{c.notes}</span>}
      </div>
      <div className="flex gap-1">
        <button className="btn btn-ghost px-2.5" title={t('contacts.write')}
          onClick={() => navigate(`/?${new URLSearchParams({ to: c.email, ...(c.name ? { name: c.name } : {}) })}`)}>
          <Icon name="compose" /> <span className="max-sm:sr-only">{t('contacts.write')}</span>
        </button>
        <button className="btn btn-ghost px-2.5" onClick={onEdit}>{t('contacts.edit')}</button>
        <button className="btn btn-ghost px-2.5 text-crit" disabled={remove.isPending}
          onClick={() => window.confirm(t('contacts.deleteConfirm', { name })) && remove.mutate()}>
          <Icon name="trash" /><span className="sr-only">{t('contacts.delete')}</span>
        </button>
      </div>
    </div>
  )
}

function ContactForm({ contact, onDone, onCancel }: { contact?: Contact; onDone: () => void; onCancel: () => void }) {
  const { t } = useTranslation()
  const [value, setValue] = useState<ContactInput>(contact
    ? { name: contact.name, email: contact.email, phone: contact.phone, notes: contact.notes }
    : EMPTY)
  const set = (p: Partial<ContactInput>) => setValue((v) => ({ ...v, ...p }))
  const save = useMutation({
    mutationFn: () => (contact ? api.updateContact(contact.id, value) : api.createContact(value)),
    onSuccess: onDone,
  })
  return (
    <form className="card flex flex-col gap-4 p-4" onSubmit={(e) => { e.preventDefault(); save.mutate() }}>
      <h2 className="font-bold">{contact ? contact.name || contact.email : t('contacts.new')}</h2>
      {save.error && <Alert>{save.error instanceof ApiError ? save.error.message : t('common.error')}</Alert>}
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label={t('contacts.name')} value={value.name} onChange={(e) => set({ name: e.target.value })} autoFocus />
        <Field label={t('contacts.email')} type="email" required value={value.email} onChange={(e) => set({ email: e.target.value })} />
        <Field label={`${t('contacts.phone')} (${t('common.optional')})`} type="tel" value={value.phone} onChange={(e) => set({ phone: e.target.value })} />
        <Field label={`${t('contacts.notes')} (${t('common.optional')})`} value={value.notes} onChange={(e) => set({ notes: e.target.value })} />
      </div>
      <div className="flex gap-2">
        <button className="btn btn-primary" disabled={save.isPending}>{t('contacts.save')}</button>
        <button type="button" className="btn btn-ghost" onClick={onCancel}>{t('common.cancel')}</button>
      </div>
    </form>
  )
}
