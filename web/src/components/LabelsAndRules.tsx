import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError, type Label, type Rule, type RuleInput } from '../lib/api'
import { LABEL_COLORS } from '../lib/mail'
import { Alert, Field } from './ui'
import { useConfirm } from './Confirm'

const errorText = (e: unknown, fallback: string) => (e instanceof ApiError ? e.message : fallback)

/** Ajustes › Etiquetas y filtros. */
export function LabelsAndRules() {
  return (
    <div className="flex flex-col gap-10">
      <Labels />
      <Rules />
    </div>
  )
}

function Labels() {
  const { t } = useTranslation()
  const labels = useQuery({ queryKey: ['labels'], queryFn: api.labels })
  const [adding, setAdding] = useState(false)
  const list = labels.data ?? []
  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-xl font-bold">{t('labels.title')}</h2>
        {!adding && <button className="btn" onClick={() => setAdding(true)}>{t('labels.add')}</button>}
      </div>
      {adding && <LabelForm color={LABEL_COLORS[list.length % LABEL_COLORS.length]} onDone={() => setAdding(false)} />}
      {labels.isSuccess && list.length === 0 && !adding && <p className="text-muted">{t('labels.none')}</p>}
      {list.length > 0 && (
        <ul className="card divide-y divide-line">
          {list.map((l) => <li key={l.id}><LabelRow label={l} /></li>)}
        </ul>
      )}
    </section>
  )
}

function LabelRow({ label }: { label: Label }) {
  const { t } = useTranslation()
  const confirm = useConfirm()
  const qc = useQueryClient()
  const [editing, setEditing] = useState(false)
  const remove = useMutation({
    mutationFn: () => api.deleteLabel(label.id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['labels'] })
      qc.invalidateQueries({ queryKey: ['threads'] })
      qc.invalidateQueries({ queryKey: ['rules'] })
    },
  })
  if (editing) return <div className="p-2"><LabelForm label={label} onDone={() => setEditing(false)} /></div>
  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <span className="size-3.5 shrink-0 rounded-full" style={{ background: label.color }} />
      <span className="min-w-0 flex-1">
        <span className="block truncate font-semibold">{label.name}</span>
        <span className="text-sm text-muted">{t('labels.count', { count: label.threads })}</span>
      </span>
      <button className="btn btn-ghost px-2.5" onClick={() => setEditing(true)}>{t('labels.edit')}</button>
      <button className="btn btn-ghost px-2.5 text-crit" disabled={remove.isPending}
        onClick={async () => (await confirm({ text: t('labels.deleteConfirm', { name: label.name }), ok: t('common.delete'), danger: true })) && remove.mutate()}>
        {t('labels.delete')}
      </button>
    </div>
  )
}

function LabelForm({ label, color, onDone }: { label?: Label; color?: string; onDone: () => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const [name, setName] = useState(label?.name ?? '')
  const [hue, setHue] = useState(label?.color ?? color ?? LABEL_COLORS[0])
  const save = useMutation({
    mutationFn: () => (label ? api.updateLabel(label.id, name.trim(), hue) : api.createLabel(name.trim(), hue)),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['labels'] })
      onDone()
    },
  })
  return (
    <form className="card flex flex-col gap-4 p-4" onSubmit={(e) => { e.preventDefault(); save.mutate() }}>
      {save.error && <Alert>{errorText(save.error, t('common.error'))}</Alert>}
      <Field label={t('labels.name')} required maxLength={60} value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      <fieldset className="flex flex-col gap-1.5">
        <legend className="label mb-1.5">{t('labels.color')}</legend>
        <div className="flex flex-wrap gap-2">
          {LABEL_COLORS.map((c) => (
            <button key={c} type="button" aria-pressed={hue === c} aria-label={c} onClick={() => setHue(c)}
              className={`size-9 rounded-full border-2 ${hue === c ? 'border-fg' : 'border-transparent'}`}>
              <span className="m-auto block size-6 rounded-full" style={{ background: c }} />
            </button>
          ))}
        </div>
      </fieldset>
      <div className="flex gap-2">
        <button className="btn btn-primary" disabled={!name.trim() || save.isPending}>{t('labels.save')}</button>
        <button type="button" className="btn btn-ghost" onClick={onDone}>{t('common.cancel')}</button>
      </div>
    </form>
  )
}

const EMPTY_RULE: RuleInput = {
  name: '', enabled: true, account_id: null, from_has: '', to_has: '', subject_has: '', body_has: '',
  label_id: null, mark_read: false, star: false, move_to: '',
}

function Rules() {
  const { t } = useTranslation()
  const rules = useQuery({ queryKey: ['rules'], queryFn: api.rules })
  const [adding, setAdding] = useState(false)
  const [editing, setEditing] = useState<string | null>(null)
  const list = rules.data ?? []
  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-xl font-bold">{t('rules.title')}</h2>
        {!adding && <button className="btn" onClick={() => { setAdding(true); setEditing(null) }}>{t('rules.add')}</button>}
      </div>
      <p className="text-muted">{t('rules.intro')}</p>
      {adding && <RuleForm onDone={() => setAdding(false)} />}
      {rules.isSuccess && list.length === 0 && !adding && <p className="text-muted">{t('rules.none')}</p>}
      {list.length > 0 && (
        <ul className="card divide-y divide-line">
          {list.map((r) => (
            <li key={r.id}>
              {editing === r.id
                ? <div className="p-2"><RuleForm rule={r} onDone={() => setEditing(null)} /></div>
                : <RuleRow rule={r} onEdit={() => { setEditing(r.id); setAdding(false) }} />}
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

function RuleRow({ rule: r, onEdit }: { rule: Rule; onEdit: () => void }) {
  const { t } = useTranslation()
  const confirm = useConfirm()
  const qc = useQueryClient()
  const labels = useQuery({ queryKey: ['labels'], queryFn: api.labels })
  const remove = useMutation({ mutationFn: () => api.deleteRule(r.id), onSuccess: () => qc.invalidateQueries({ queryKey: ['rules'] }) })
  const when = [
    r.from_has && `${t('rules.fromHas')} «${r.from_has}»`,
    r.to_has && `${t('rules.toHas')} «${r.to_has}»`,
    r.subject_has && `${t('rules.subjectHas')} «${r.subject_has}»`,
    r.body_has && `${t('rules.bodyHas')} «${r.body_has}»`,
  ].filter(Boolean)
  const label = labels.data?.find((l) => l.id === r.label_id)
  const then = [
    label && `${t('rules.label')} ${label.name}`,
    r.mark_read && t('rules.markRead'),
    r.star && t('rules.star'),
    r.move_to && `${t('rules.moveTo')} ${t(`mail.roles.${r.move_to}`)}`,
  ].filter(Boolean)
  return (
    <div className="flex flex-wrap items-center gap-3 px-4 py-3">
      <div className={`min-w-0 flex-1 ${r.enabled ? '' : 'opacity-60'}`}>
        <p className="font-semibold">{r.name}{!r.enabled && <span className="font-normal text-muted"> · {t('rules.off')}</span>}</p>
        <p className="text-sm text-muted">{when.join(' · ')} → {then.join(' · ')}</p>
      </div>
      <button className="btn btn-ghost px-2.5" onClick={onEdit}>{t('rules.edit')}</button>
      <button className="btn btn-ghost px-2.5 text-crit" disabled={remove.isPending}
        onClick={async () => (await confirm({ text: t('rules.deleteConfirm', { name: r.name }), ok: t('common.delete'), danger: true })) && remove.mutate()}>
        {t('rules.delete')}
      </button>
    </div>
  )
}

function RuleForm({ rule, onDone }: { rule?: Rule; onDone: () => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts })
  const labels = useQuery({ queryKey: ['labels'], queryFn: api.labels })
  const [value, setValue] = useState<RuleInput>(() => {
    if (!rule) return EMPTY_RULE
    const { id: _id, ...rest } = rule
    return rest
  })
  const [problem, setProblem] = useState<string | null>(null)
  const set = (p: Partial<RuleInput>) => setValue((v) => ({ ...v, ...p }))
  const save = useMutation({
    mutationFn: () => (rule ? api.updateRule(rule.id, value) : api.createRule(value)),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['rules'] })
      onDone()
    },
  })

  function submit() {
    const v = value
    if (![v.from_has, v.to_has, v.subject_has, v.body_has].some((x) => x.trim())) return setProblem(t('rules.needCondition'))
    if (!v.label_id && !v.mark_read && !v.star && !v.move_to) return setProblem(t('rules.needAction'))
    setProblem(null)
    save.mutate()
  }

  const select = 'input'
  return (
    <form className="card flex flex-col gap-5 p-4" onSubmit={(e) => { e.preventDefault(); submit() }}>
      <h3 className="font-bold">{rule ? rule.name : t('rules.new')}</h3>
      {(problem || save.error) && <Alert>{problem ?? errorText(save.error, t('common.error'))}</Alert>}
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label={t('rules.name')} required value={value.name} onChange={(e) => set({ name: e.target.value })} autoFocus />
        {(accounts.data?.length ?? 0) > 1 && (
          <div className="flex flex-col gap-1.5">
            <label className="label" htmlFor="rule-account">{t('rules.account')}</label>
            <select id="rule-account" className={select} value={value.account_id ?? ''}
              onChange={(e) => set({ account_id: e.target.value || null })}>
              <option value="">{t('rules.anyAccount')}</option>
              {accounts.data?.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
            </select>
          </div>
        )}
      </div>
      <fieldset className="flex flex-col gap-3">
        <legend className="label mb-2">{t('rules.when')}</legend>
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t('rules.fromHas')} value={value.from_has} onChange={(e) => set({ from_has: e.target.value })} />
          <Field label={t('rules.toHas')} value={value.to_has} onChange={(e) => set({ to_has: e.target.value })} />
          <Field label={t('rules.subjectHas')} value={value.subject_has} onChange={(e) => set({ subject_has: e.target.value })} />
          <Field label={t('rules.bodyHas')} value={value.body_has} onChange={(e) => set({ body_has: e.target.value })} />
        </div>
      </fieldset>
      <fieldset className="flex flex-col gap-3">
        <legend className="label mb-2">{t('rules.then')}</legend>
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="flex flex-col gap-1.5">
            <label className="label" htmlFor="rule-label">{t('rules.label')}</label>
            <select id="rule-label" className={select} value={value.label_id ?? ''} onChange={(e) => set({ label_id: e.target.value || null })}>
              <option value="">{t('rules.noLabel')}</option>
              {labels.data?.map((l) => <option key={l.id} value={l.id}>{l.name}</option>)}
            </select>
          </div>
          <div className="flex flex-col gap-1.5">
            <label className="label" htmlFor="rule-move">{t('rules.moveTo')}</label>
            <select id="rule-move" className={select} value={value.move_to}
              onChange={(e) => set({ move_to: e.target.value as RuleInput['move_to'] })}>
              <option value="">{t('rules.stay')}</option>
              {(['archive', 'trash', 'junk'] as const).map((r) => <option key={r} value={r}>{t(`mail.roles.${r}`)}</option>)}
            </select>
          </div>
        </div>
        <div className="flex flex-wrap gap-x-6 gap-y-2">
          <Check label={t('rules.markRead')} checked={value.mark_read} onChange={(mark_read) => set({ mark_read })} />
          <Check label={t('rules.star')} checked={value.star} onChange={(star) => set({ star })} />
          <Check label={t('rules.enabled')} checked={value.enabled} onChange={(enabled) => set({ enabled })} />
        </div>
      </fieldset>
      <div className="flex gap-2">
        <button className="btn btn-primary" disabled={save.isPending}>{t('rules.save')}</button>
        <button type="button" className="btn btn-ghost" onClick={onDone}>{t('common.cancel')}</button>
      </div>
    </form>
  )
}

function Check({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex min-h-10 items-center gap-2.5">
      <input type="checkbox" className="size-5 accent-[var(--accent)]" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      {label}
    </label>
  )
}
