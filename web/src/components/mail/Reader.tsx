import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, type Addr, type Message, type Thread } from '../../lib/api'
import { emailDocument, fileSize, initials, linkify, longDate, shortDate } from '../../lib/mail'
import {
  addrText, emptyCompose, forwardHtml, pickIdentity, prefixSubject, quoteHtml, replyRecipients, signatureHtml,
} from '../../lib/compose'
import { useCompose } from '../compose/ComposerProvider'
import { FONTS } from '../../lib/look'
import { Alert } from '../ui'
import { Icon } from './Icons'

export function Reader({ id, font, onBack }: { id: string | null; font: string; onBack: () => void }) {
  const { t } = useTranslation()
  const qc = useQueryClient()
  const q = useQuery({ queryKey: ['thread', id], queryFn: () => api.thread(id!), enabled: !!id })
  const update = useMutation({
    mutationFn: (change: { seen?: boolean; flagged?: boolean }) => api.updateThread(id!, change),
    onMutate: (change) => {
      // Se ve al momento; el servidor IMAP se entera por detrás.
      qc.setQueryData<Thread>(['thread', id], (old) => old && {
        ...old,
        messages: old.messages.map((m, i) => ({
          ...m,
          seen: change.seen ?? m.seen,
          flagged: change.flagged === undefined ? m.flagged : change.flagged ? m.flagged || i === old.messages.length - 1 : false,
        })),
      })
    },
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ['threads'] })
      qc.invalidateQueries({ queryKey: ['mailboxes'] })
    },
  })

  // Abrir un hilo lo marca como leído, una sola vez.
  const marked = useRef<string | null>(null)
  useEffect(() => {
    if (!q.data || marked.current === q.data.id) return
    marked.current = q.data.id
    if (q.data.messages.some((m) => !m.seen)) update.mutate({ seen: true })
  }, [q.data]) // eslint-disable-line react-hooks/exhaustive-deps

  if (!id) {
    return (
      <div className="m-auto max-w-sm px-6 py-24 text-center text-muted">
        <b className="mb-1 block text-lg text-fg">{t('mail.pick')}</b>
        {t('mail.pickBody')}
      </div>
    )
  }
  if (q.isLoading) return <p className="p-6 text-muted">{t('common.loading')}</p>
  if (q.isError || !q.data) return <div className="p-6"><Alert>{t('mail.missing')}</Alert></div>

  const th = q.data
  const flagged = th.messages.some((m) => m.flagged)
  return (
    <div className="mx-auto flex w-full max-w-[48rem] flex-col gap-4 px-[var(--pad)] pb-12 sm:px-6">
      <div className="sticky top-0 z-[1] -mx-[var(--pad)] flex flex-wrap gap-1.5 border-b border-line bg-bg px-[var(--pad)] py-2.5 sm:-mx-6 sm:px-6">
        <button className="btn btn-ghost px-2.5 md:hidden" onClick={onBack}>
          <Icon name="back" /> {t('common.back')}
        </button>
        <button className="btn btn-ghost px-2.5" onClick={() => { update.mutate({ seen: false }); onBack() }}>
          <Icon name="unread" /> <span className="max-sm:sr-only">{t('mail.markUnread')}</span>
        </button>
        <button className="btn btn-ghost px-2.5" aria-pressed={flagged} onClick={() => update.mutate({ flagged: !flagged })}>
          <Icon name="star" filled={flagged} className={flagged ? 'text-warn' : ''} />
          <span className="max-sm:sr-only">{flagged ? t('mail.unflag') : t('mail.flag')}</span>
        </button>
      </div>
      <h2 className="text-2xl font-bold leading-tight sm:text-[1.75rem]">{th.subject || t('mail.noSubject')}</h2>
      <ol className="flex flex-col gap-3">
        {th.messages.map((m, i) => (
          <li key={m.id}>
            <MessageView message={m} thread={th} font={font} initiallyOpen={i === th.messages.length - 1 || !m.seen} />
          </li>
        ))}
      </ol>
    </div>
  )
}

function people(list: Addr[]): string {
  return list.map((a) => a.name || a.email).join(', ')
}

/** Prepara una respuesta o un reenvío de un mensaje. */
function useReply(m: Message, th: Thread) {
  const { t, i18n } = useTranslation()
  const compose = useCompose()
  const identities = useQuery({ queryKey: ['identities'], queryFn: api.identities })
  return (kind: 'reply' | 'all' | 'forward') => {
    const list = identities.data ?? []
    const identity = pickIdentity(list, m, th.account_id)
    const base = emptyCompose(identity)
    const sig = signatureHtml(identity?.signature ?? '')
    const date = longDate(m.date, i18n.language)
    if (kind === 'forward') {
      const lines: [string, string][] = [
        [t('compose.fwdFrom'), m.from ? addrText(m.from) : ''],
        [t('compose.fwdDate'), date],
        [t('compose.fwdSubject'), m.subject],
        [t('compose.fwdTo'), m.to.map(addrText).join(', ')],
      ]
      compose({
        ...base,
        subject: prefixSubject('Fwd', m.subject),
        html: `<p></p>${sig}${forwardHtml(t('compose.forwarded'), lines, m)}`,
        forwardOf: m.id,
        forwarded: m.attachments.filter((a) => !a.inline),
      })
      return
    }
    const { to, cc } = replyRecipients(m, kind === 'all', list.map((i) => i.email))
    const who = m.from?.name || m.from?.email || '?'
    compose({
      ...base,
      to,
      cc,
      subject: prefixSubject('Re', m.subject),
      html: `<p></p>${sig}${quoteHtml(t('compose.wrote', { date, name: who }), m)}`,
      replyTo: m.id,
    })
  }
}

function MessageView({ message: m, thread, font, initiallyOpen }: { message: Message; thread: Thread; font: string; initiallyOpen: boolean }) {
  const { t, i18n } = useTranslation()
  const reply = useReply(m, thread)
  const many = m.to.length + m.cc.length > 1
  const [open, setOpen] = useState(initiallyOpen)
  const [images, setImages] = useState(false)
  const name = m.from?.name || m.from?.email || '?'
  const files = m.attachments.filter((a) => !a.inline)
  const preview = (m.text ?? '').replace(/\s+/g, ' ').trim().slice(0, 140)

  return (
    <article className="card overflow-hidden">
      <button className="flex w-full items-center gap-3 px-4 py-3 text-left" aria-expanded={open} onClick={() => setOpen(!open)}>
        <span className="grid size-10 shrink-0 place-items-center rounded-full bg-surface-2 text-sm font-bold" aria-hidden="true">
          {initials(name)}
        </span>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate font-semibold">{name}</span>
          <span className="truncate text-sm text-muted">
            {open ? <>{m.from?.email}{m.to.length > 0 && <> · {t('mail.to')} {people(m.to)}</>}</> : preview}
          </span>
        </span>
        {m.answered && <span className="shrink-0 self-start pt-0.5 text-muted" title={t('compose.answered')}>
          <Icon name="reply" className="size-4" /><span className="sr-only">{t('compose.answered')}</span>
        </span>}
        <time className="shrink-0 self-start text-sm text-muted tabular-nums" dateTime={new Date(m.date * 1000).toISOString()}>
          {open ? longDate(m.date, i18n.language) : shortDate(m.date, i18n.language)}
        </time>
      </button>
      {open && (
        <div className="flex flex-col gap-3 border-t border-line">
          {m.cc.length > 0 && <p className="px-4 pt-3 text-sm text-muted">{t('mail.cc')} {people(m.cc)}</p>}
          {m.html && m.remote_images > 0 && !images && (
            <div className="mx-4 mt-3 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-lg border border-[color-mix(in_srgb,var(--good)_40%,var(--line))] bg-[color-mix(in_srgb,var(--good)_8%,var(--surface))] px-3 py-2 text-sm">
              <Icon name="shield" className="shrink-0 text-good" />
              <span className="min-w-0 flex-1">{t('mail.imagesBlocked', { count: m.remote_images })}</span>
              <button className="btn min-h-8 px-3 py-1 text-sm" onClick={() => setImages(true)}>{t('mail.showImages')}</button>
            </div>
          )}
          {m.html ? (
            <EmailFrame html={m.html} images={images} font={font} title={m.subject} />
          ) : (
            <div className="whitespace-pre-wrap px-4 pb-4 pt-1 [overflow-wrap:anywhere]">
              {linkify(m.text ?? '').map((p, i) => p.href
                ? <a key={i} href={p.href} target="_blank" rel="noopener noreferrer" className="text-accent underline">{p.text}</a>
                : <span key={i}>{p.text}</span>)}
            </div>
          )}
          {files.length > 0 && (
            <ul className="flex flex-wrap gap-2 px-4 pb-4" aria-label={t('mail.attachments')}>
              {files.map((a) => (
                <li key={a.index}>
                  <a href={a.url} download={a.name} className="flex max-w-full items-center gap-2 rounded-lg border border-line bg-bg px-3 py-2 text-sm hover:border-muted">
                    <Icon name="clip" className="shrink-0 text-muted" />
                    <span className="min-w-0 truncate">{a.name}</span>
                    <small className="shrink-0 text-muted">{fileSize(a.size, i18n.language)}</small>
                    <Icon name="download" className="shrink-0 text-muted" />
                  </a>
                </li>
              ))}
            </ul>
          )}
          <div className="flex flex-wrap gap-1.5 border-t border-line px-3 py-2">
            <button className="btn btn-ghost px-3" onClick={() => reply('reply')}>
              <Icon name="reply" /> {t('compose.replyAction')}
            </button>
            {many && (
              <button className="btn btn-ghost px-3" onClick={() => reply('all')}>
                <Icon name="replyAll" /> {t('compose.replyAll')}
              </button>
            )}
            <button className="btn btn-ghost px-3" onClick={() => reply('forward')}>
              <Icon name="forward" /> {t('compose.forwardAction')}
            </button>
          </div>
        </div>
      )}
    </article>
  )
}

/**
 * Muestra el HTML de un correo aislado: iframe con sandbox (sin scripts) y CSP que
 * bloquea lo remoto. Se ajusta a la altura del contenido.
 */
function EmailFrame({ html, images, font, title }: { html: string; images: boolean; font: string; title: string }) {
  const ref = useRef<HTMLIFrameElement>(null)
  const [height, setHeight] = useState(200)
  const stack = FONTS[font as keyof typeof FONTS]?.stack ?? FONTS.atkinson.stack

  useEffect(() => {
    const frame = ref.current
    if (!frame) return
    let ro: ResizeObserver | undefined
    const fit = () => {
      const doc = frame.contentDocument
      if (!doc?.documentElement) return
      setHeight(Math.max(60, doc.documentElement.scrollHeight))
      if (!ro && typeof ResizeObserver !== 'undefined') {
        ro = new ResizeObserver(() => setHeight(Math.max(60, doc.documentElement.scrollHeight)))
        ro.observe(doc.body)
      }
    }
    frame.addEventListener('load', fit)
    return () => {
      frame.removeEventListener('load', fit)
      ro?.disconnect()
    }
  }, [html, images])

  return (
    <div className="mx-4 mb-4 overflow-hidden rounded-lg border border-line bg-white">
      <iframe ref={ref} title={title} className="block w-full" style={{ height }}
        sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
        srcDoc={emailDocument(html, { images, font: stack.replace(/"/g, "'") })} />
    </div>
  )
}
