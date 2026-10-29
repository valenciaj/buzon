import { useEffect, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { EditorContent, useEditor, type Editor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import { Placeholder } from '@tiptap/extensions'
import { api, ApiError, type Identity, type Upload } from '../../lib/api'
import { swapSignature, textFromDoc, toDraftInput, type ComposeState } from '../../lib/compose'
import { Alert } from '../ui'
import { Icon } from '../mail/Icons'
import { RecipientField } from './RecipientField'
import { Popover } from '../mail/Menus'
import { fileSize, snoozeOptions } from '../../lib/mail'
import { useConfirm } from '../Confirm'

/** Lo mismo que admite el servidor. */
const MAX_UPLOAD = 25 * 1024 * 1024
const AUTOSAVE_MS = 1500

type Pending = { key: string; name: string; size: number }

export interface Outgoing {
  state: ComposeState
  html: string
  text: string
  /** Programado para esta fecha (segundos Unix). */
  sendAt?: number
  /** Avisar si nadie responde en estos días. */
  followUp?: number
}

const FOLLOW_UP_DAYS = [1, 3, 7, 14]

/**
 * Ventana de redactar. En el móvil ocupa la pantalla; en el escritorio flota abajo a la
 * derecha y deja ver el correo. Guarda el borrador sola mientras escribes.
 */
export function Composer({ initial, error, onClose, onSend }: {
  initial: ComposeState
  error?: string | null
  onClose: (saved: boolean) => void
  onSend: (o: Outgoing) => void
}) {
  const { t, i18n } = useTranslation()
  const confirm = useConfirm()
  const qc = useQueryClient()
  const identities = useQuery({ queryKey: ['identities'], queryFn: api.identities })
  const [state, setState] = useState(initial)
  const stateRef = useRef(state)
  stateRef.current = state
  const [showCc, setShowCc] = useState(initial.cc.length > 0)
  const [showBcc, setShowBcc] = useState(initial.bcc.length > 0)
  const [pending, setPending] = useState<Pending[]>([])
  const [problem, setProblem] = useState<string | null>(error ?? null)
  const [saved, setSaved] = useState<'idle' | 'saving' | 'saved'>('idle')
  const [dragging, setDragging] = useState(false)
  const [followUp, setFollowUp] = useState(0)
  const dirty = useRef(false)
  const chain = useRef<Promise<unknown>>(Promise.resolve())
  const fileInput = useRef<HTMLInputElement>(null)

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: false,
        codeBlock: false,
        link: { openOnClick: false, autolink: true, defaultProtocol: 'https' },
      }),
      Placeholder.configure({ placeholder: t('compose.placeholder') }),
    ],
    content: initial.html,
    // Con destinatarios y asunto ya puestos (respuestas, borradores) se escribe directamente.
    autofocus: initial.to.length > 0 && initial.subject.trim() ? 'start' : false,
    editorProps: { attributes: { class: 'compose-body', 'aria-label': t('compose.body') } },
    onUpdate: ({ editor }) => change({ html: editor.getHTML() }),
  })

  // Si el remitente elegido no existe (borrador antiguo), se usa el predeterminado.
  const list = identities.data ?? []
  const identity = list.find((i) => i.id === state.identityId)
  useEffect(() => {
    if (list.length && !identity) setState((s) => ({ ...s, identityId: (list.find((i) => i.is_default) ?? list[0]).id }))
  }, [list.length, identity]) // eslint-disable-line react-hooks/exhaustive-deps

  function change(c: Partial<ComposeState>) {
    dirty.current = true
    setState((s) => ({ ...s, ...c }))
  }

  const text = (ed: Editor | null) => (ed ? textFromDoc(ed.getJSON()) : '')

  /** Guarda el borrador; las llamadas se encadenan para no crear dos. */
  function save(): Promise<unknown> {
    chain.current = chain.current.then(async () => {
      if (!dirty.current) return
      dirty.current = false
      setSaved('saving')
      const s = stateRef.current
      const input = toDraftInput(s, text(editor))
      try {
        const d = s.draftId ? await api.updateDraft(s.draftId, input) : await api.createDraft(input)
        setState((cur) => ({ ...cur, draftId: d.id }))
        stateRef.current = { ...stateRef.current, draftId: d.id }
        setSaved('saved')
        qc.invalidateQueries({ queryKey: ['drafts'] })
      } catch {
        dirty.current = true
        setSaved('idle')
      }
    })
    return chain.current
  }

  useEffect(() => {
    if (!dirty.current) return
    const h = setTimeout(save, AUTOSAVE_MS)
    return () => clearTimeout(h)
  }, [state]) // eslint-disable-line react-hooks/exhaustive-deps

  function chooseIdentity(next: Identity) {
    if (editor) editor.commands.setContent(swapSignature(editor.getJSON(), identity?.signature ?? '', next.signature))
    change({ identityId: next.id, html: editor?.getHTML() ?? state.html })
  }

  async function addFiles(files: FileList | File[]) {
    for (const f of Array.from(files)) {
      if (f.size > MAX_UPLOAD) {
        setProblem(t('compose.tooBig', { name: f.name }))
        continue
      }
      const key = `${f.name}-${Math.random()}`
      setPending((p) => [...p, { key, name: f.name, size: f.size }])
      try {
        const up = await api.upload(f)
        change({ uploads: [...stateRef.current.uploads, up] })
      } catch (e) {
        setProblem(e instanceof ApiError ? e.message : t('compose.uploadFailed', { name: f.name }))
      } finally {
        setPending((p) => p.filter((x) => x.key !== key))
      }
    }
  }

  function removeUpload(u: Upload) {
    change({ uploads: stateRef.current.uploads.filter((x) => x.id !== u.id) })
    // Si ya está en un borrador guardado, se borra al guardar o descartar.
    if (!stateRef.current.draftId) api.deleteUpload(u.id).catch(() => undefined)
  }

  async function close() {
    const touched = dirty.current || !!stateRef.current.draftId
    await save()
    onClose(touched)
  }

  async function discard() {
    const s = stateRef.current
    dirty.current = false
    await chain.current
    const id = stateRef.current.draftId
    if (id) await api.deleteDraft(id).catch(() => undefined)
    else for (const u of s.uploads) api.deleteUpload(u.id).catch(() => undefined)
    qc.invalidateQueries({ queryKey: ['drafts'] })
    onClose(false)
  }

  async function send(sendAt?: number) {
    const s = stateRef.current
    if (!s.to.length && !s.cc.length && !s.bcc.length) {
      setProblem(t('compose.noRecipients'))
      return
    }
    if (pending.length) {
      setProblem(t('compose.stillUploading'))
      return
    }
    if (!s.subject.trim() && !(await confirm({ text: t('compose.noSubjectConfirm'), ok: t('compose.send') }))) return
    // Queda guardado por si se cierra la pestaña antes de que salga.
    dirty.current = true
    await save()
    onSend({ state: stateRef.current, html: editor?.getHTML() ?? s.html, text: text(editor), sendAt, followUp })
  }

  function onKey(e: React.KeyboardEvent) {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault()
      send()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      close()
    }
  }

  const title = state.subject.trim() || t(state.replyTo ? 'compose.reply' : state.forwardOf ? 'compose.forward' : 'compose.new')
  const tool = (name: string, label: string, active: boolean, run: () => void) => (
    <button type="button" className={`grid size-9 place-items-center rounded-md hover:bg-surface-2 ${active ? 'bg-surface-2 text-accent' : 'text-muted'}`}
      aria-label={label} title={label} aria-pressed={active} onMouseDown={(e) => e.preventDefault()} onClick={run}>
      <Icon name={name} />
    </button>
  )

  function setLink() {
    if (!editor) return
    const prev = editor.getAttributes('link').href as string | undefined
    const url = window.prompt(t('compose.linkPrompt'), prev ?? 'https://')
    if (url === null) return
    if (!url.trim() || url === 'https://') editor.chain().focus().extendMarkRange('link').unsetLink().run()
    else editor.chain().focus().extendMarkRange('link').setLink({ href: url.trim() }).run()
  }

  return (
    <div role="dialog" aria-modal="false" aria-label={title} onKeyDown={onKey}
      onDragOver={(e) => { if (e.dataTransfer.types.includes('Files')) { e.preventDefault(); setDragging(true) } }}
      onDragLeave={(e) => { if (e.currentTarget === e.target) setDragging(false) }}
      onDrop={(e) => { if (e.dataTransfer.files.length) { e.preventDefault(); setDragging(false); addFiles(e.dataTransfer.files) } }}
      className="fixed inset-0 z-30 flex flex-col bg-surface pb-[env(safe-area-inset-bottom)] pt-[env(safe-area-inset-top)] md:inset-auto md:bottom-0 md:right-6 md:h-[min(44rem,calc(100vh-3rem))] md:w-[40rem] md:rounded-t-xl md:border md:border-b-0 md:border-line md:pb-0 md:pt-0 md:shadow-2xl">
      <header className="flex items-center gap-2 border-b border-line py-1.5 pl-4 pr-2">
        <h2 className="min-w-0 flex-1 truncate font-bold">{title}</h2>
        <span className="text-sm text-muted" role="status">
          {saved === 'saving' ? t('compose.saving') : saved === 'saved' ? t('compose.saved') : ''}
        </span>
        <button type="button" className="btn btn-ghost px-2" aria-label={t('compose.close')} title={t('compose.close')} onClick={close}>
          <Icon name="close" />
        </button>
      </header>

      {list.length > 1 && (
        <div className="flex items-center gap-1.5 border-b border-line px-4 py-1">
          <label htmlFor="compose-from" className="w-12 shrink-0 text-sm text-muted">{t('compose.from')}</label>
          <select id="compose-from" className="min-h-9 min-w-0 flex-1 bg-transparent py-1" value={state.identityId ?? ''}
            onChange={(e) => { const next = list.find((i) => i.id === e.target.value); if (next) chooseIdentity(next) }}>
            {list.map((i) => <option key={i.id} value={i.id}>{i.name ? `${i.name} <${i.email}>` : i.email}</option>)}
          </select>
        </div>
      )}
      <RecipientField label={t('compose.to')} value={state.to} onChange={(to) => change({ to })} autoFocus={!initial.to.length}>
        <span className="ml-auto flex gap-1 text-sm">
          {!showCc && <button type="button" className="rounded px-1.5 py-1 text-muted hover:text-fg" onClick={() => setShowCc(true)}>{t('compose.cc')}</button>}
          {!showBcc && <button type="button" className="rounded px-1.5 py-1 text-muted hover:text-fg" onClick={() => setShowBcc(true)}>{t('compose.bcc')}</button>}
        </span>
      </RecipientField>
      {showCc && <RecipientField label={t('compose.cc')} value={state.cc} onChange={(cc) => change({ cc })} />}
      {showBcc && <RecipientField label={t('compose.bcc')} value={state.bcc} onChange={(bcc) => change({ bcc })} />}
      <input aria-label={t('compose.subject')} placeholder={t('compose.subject')} value={state.subject}
        autoFocus={initial.to.length > 0 && !initial.subject.trim()}
        onChange={(e) => change({ subject: e.target.value })}
        className="min-h-11 border-b border-line bg-transparent px-4 font-semibold outline-none placeholder:font-normal placeholder:text-muted" />

      <div className="flex flex-wrap gap-0.5 border-b border-line px-2 py-1" role="toolbar" aria-label={t('compose.format')}>
        {tool('bold', t('compose.bold'), !!editor?.isActive('bold'), () => editor?.chain().focus().toggleBold().run())}
        {tool('italic', t('compose.italic'), !!editor?.isActive('italic'), () => editor?.chain().focus().toggleItalic().run())}
        {tool('list', t('compose.bullets'), !!editor?.isActive('bulletList'), () => editor?.chain().focus().toggleBulletList().run())}
        {tool('olist', t('compose.numbers'), !!editor?.isActive('orderedList'), () => editor?.chain().focus().toggleOrderedList().run())}
        {tool('quote', t('compose.quote'), !!editor?.isActive('blockquote'), () => editor?.chain().focus().toggleBlockquote().run())}
        {tool('link', t('compose.link'), !!editor?.isActive('link'), setLink)}
      </div>

      <div className={`relative min-h-0 flex-1 overflow-y-auto ${dragging ? 'outline-2 -outline-offset-4 outline-dashed outline-accent' : ''}`}
        onClick={(e) => { if (e.target === e.currentTarget) editor?.commands.focus('end') }}>
        <EditorContent editor={editor} />
      </div>

      {(state.uploads.length > 0 || pending.length > 0 || state.forwarded.length > 0) && (
        <ul className="flex max-h-32 flex-wrap gap-2 overflow-y-auto border-t border-line px-4 py-2" aria-label={t('mail.attachments')}>
          {state.forwarded.filter((a) => !a.inline).map((a) => (
            <li key={`f${a.index}`} className="flex max-w-full items-center gap-2 rounded-lg border border-line px-3 py-1.5 text-sm">
              <Icon name="forward" className="shrink-0 text-muted" />
              <span className="min-w-0 truncate">{a.name}</span>
              <small className="text-muted">{fileSize(a.size, i18n.language)}</small>
            </li>
          ))}
          {state.uploads.map((u) => (
            <li key={u.id} className="flex max-w-full items-center gap-2 rounded-lg border border-line py-1 pl-3 pr-1 text-sm">
              <Icon name="clip" className="shrink-0 text-muted" />
              <span className="min-w-0 truncate">{u.name}</span>
              <small className="text-muted">{fileSize(u.size, i18n.language)}</small>
              <button type="button" className="grid size-7 place-items-center rounded-md hover:bg-surface-2"
                aria-label={t('compose.removeFile', { name: u.name })} onClick={() => removeUpload(u)}>
                <Icon name="close" className="size-4" />
              </button>
            </li>
          ))}
          {pending.map((p) => (
            <li key={p.key} className="flex max-w-full items-center gap-2 rounded-lg border border-dashed border-line px-3 py-1.5 text-sm text-muted">
              <Icon name="sync" className="shrink-0 motion-safe:animate-spin" />
              <span className="min-w-0 truncate">{p.name}</span>
              <small>{t('compose.uploading')}</small>
            </li>
          ))}
        </ul>
      )}

      {problem && <div className="px-4 pb-2 pt-2"><Alert>{problem}</Alert></div>}

      <footer className="relative flex items-center gap-1 border-t border-line px-3 py-2">
        <button type="button" className="btn btn-primary px-5" onClick={() => send()}>
          <Icon name="sent" /> {t('compose.send')}
        </button>
        <ScheduleMenu onPick={(at) => send(at)} />
        <Popover label={followUp ? t('compose.followUpOn', { count: followUp }) : t('compose.followUp')} icon="bell"
          compact up fromParent active={followUp > 0}>
          {(close) => (
            <>
              <p className="px-3 pb-1 pt-2 text-sm text-muted">{t('compose.followUpHint')}</p>
              {[0, ...FOLLOW_UP_DAYS].map((d) => (
                <button key={d} type="button" role="menuitemradio" aria-checked={followUp === d}
                  className={`flex w-full items-center justify-between px-3 py-2 text-left hover:bg-surface-2 ${followUp === d ? 'font-semibold' : ''}`}
                  onClick={() => { setFollowUp(d); close() }}>
                  {d ? t('compose.followUpDays', { count: d }) : t('compose.followUpOff')}
                  {followUp === d && <Icon name="check" className="size-4 text-accent" />}
                </button>
              ))}
            </>
          )}
        </Popover>
        <button type="button" className="btn btn-ghost px-2.5" onClick={() => fileInput.current?.click()}
          aria-label={t('compose.attach')} title={t('compose.attach')}>
          <Icon name="clip" />
        </button>
        <input ref={fileInput} type="file" multiple hidden
          onChange={(e) => { if (e.target.files) addFiles(e.target.files); e.target.value = '' }} />
        <button type="button" className="btn btn-ghost ml-auto px-2.5 text-muted" onClick={discard}
          aria-label={t('compose.discard')} title={t('compose.discard')}>
          <Icon name="trash" />
        </button>
      </footer>
    </div>
  )
}

/** «Programar envío»: mañana a primera hora, el lunes… o la fecha que se elija. */
function ScheduleMenu({ onPick }: { onPick: (at: number) => void }) {
  const { t, i18n } = useTranslation()
  const [pick, setPick] = useState('')
  const fmt = (d: Date) => d.toLocaleString(i18n.language, { weekday: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
  return (
    <Popover label={t('compose.schedule')} icon="clock" compact up fromParent>
      {(close) => (
        <>
          {snoozeOptions().map((o) => (
            <button key={o.key} type="button" role="menuitem" className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left hover:bg-surface-2"
              onClick={() => { close(); onPick(Math.floor(o.at.getTime() / 1000)) }}>
              <span>{t(`flow.snoozeOptions.${o.key}`)}</span>
              <span className="text-sm text-muted">{fmt(o.at)}</span>
            </button>
          ))}
          <form className="flex gap-1 border-t border-line p-2"
            onSubmit={(e) => {
              e.preventDefault()
              const at = new Date(pick).getTime()
              if (!Number.isFinite(at) || at <= Date.now()) return
              close()
              onPick(Math.floor(at / 1000))
            }}>
            <input type="datetime-local" className="input min-h-9 min-w-0 flex-1 py-1 text-sm" aria-label={t('compose.scheduleAt')}
              value={pick} onChange={(e) => setPick(e.target.value)} />
            <button className="btn min-h-9 px-2.5 py-1 text-sm" disabled={!pick}>OK</button>
          </form>
        </>
      )}
    </Popover>
  )
}
