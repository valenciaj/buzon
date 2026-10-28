// Utilidades para redactar: respuestas, reenvíos, firmas y el texto plano del correo.

import type { Addr, AttachmentInfo, Draft, DraftInput, Identity, Message, Upload } from './api'

/** Lo que tiene abierto la ventana de redactar. */
export interface ComposeState {
  draftId: string | null
  identityId: string | null
  to: Addr[]
  cc: Addr[]
  bcc: Addr[]
  subject: string
  html: string
  replyTo: string | null
  forwardOf: string | null
  uploads: Upload[]
  /** Adjuntos del original que van en el reenvío. */
  forwarded: AttachmentInfo[]
}

export function emptyCompose(identity?: Identity, to: Addr[] = []): ComposeState {
  return {
    draftId: null,
    identityId: identity?.id ?? null,
    to,
    cc: [],
    bcc: [],
    subject: '',
    html: `<p></p>${signatureHtml(identity?.signature ?? '')}`,
    replyTo: null,
    forwardOf: null,
    uploads: [],
    forwarded: [],
  }
}

export function fromDraft(d: Draft): ComposeState {
  return {
    draftId: d.id,
    identityId: d.identity_id,
    to: d.to,
    cc: d.cc,
    bcc: d.bcc,
    subject: d.subject,
    html: d.html || '<p></p>',
    replyTo: d.reply_to,
    forwardOf: d.forward_of,
    uploads: d.attachments,
    forwarded: [],
  }
}

export function toDraftInput(s: ComposeState, text: string): DraftInput {
  return {
    identity_id: s.identityId,
    to: s.to,
    cc: s.cc,
    bcc: s.bcc,
    subject: s.subject,
    html: s.html,
    text,
    reply_to: s.replyTo,
    forward_of: s.forwardOf,
    attachments: s.uploads.map((u) => u.id),
  }
}

export function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

/** Texto plano a párrafos HTML; los saltos de línea sueltos se conservan. */
export function textToHtml(text: string): string {
  return text
    .replace(/\r\n/g, '\n')
    .trim()
    .split(/\n{2,}/)
    .filter((p) => p.trim())
    .map((p) => `<p>${p.split('\n').map(escapeHtml).join('<br>')}</p>`)
    .join('')
}

/** La firma va tras «-- », como manda la costumbre del correo. */
export function signatureHtml(signature: string): string {
  const lines = signature.trim().split('\n')
  return signature.trim() ? `<p>-- <br>${lines.map(escapeHtml).join('<br>')}</p>` : ''
}

export function prefixSubject(prefix: 'Re' | 'Fwd', subject: string): string {
  const s = subject.trim()
  const already = prefix === 'Re' ? /^re\s*:/i : /^(fwd?|rv)\s*:/i
  return already.test(s) ? s : `${prefix}: ${s}`
}

const lower = (e: string) => e.trim().toLowerCase()

/** Desde qué identidad contestar: la que recibió el correo o, si no, la de la cuenta. */
export function pickIdentity(identities: Identity[], m: Message | null, account?: string): Identity | undefined {
  if (m) {
    const got = new Set([...m.to, ...m.cc].map((a) => lower(a.email)))
    const hit = identities.find((i) => got.has(lower(i.email)) && (!account || i.account_id === account))
      ?? identities.find((i) => got.has(lower(i.email)))
    if (hit) return hit
  }
  return identities.find((i) => i.is_default && (!account || i.account_id === account))
    ?? identities.find((i) => i.is_default)
    ?? identities[0]
}

function unique(list: Addr[], skip: Set<string>): Addr[] {
  const seen = new Set(skip)
  return list.filter((a) => {
    const e = lower(a.email)
    if (!e || seen.has(e)) return false
    seen.add(e)
    return true
  })
}

/** Destinatarios de una respuesta. `mine` son mis direcciones, que no se incluyen. */
export function replyRecipients(m: Message, all: boolean, mine: string[]): { to: Addr[]; cc: Addr[] } {
  const me = new Set(mine.map(lower))
  const fromMe = !!m.from && me.has(lower(m.from.email))
  // Si contesto a algo que envié yo, sigo escribiendo a los mismos.
  const base = fromMe ? m.to : m.from ? [m.from] : []
  // Una nota a mí mismo se contesta a mí mismo.
  const to = unique(base, me).length ? unique(base, me) : unique(base, new Set())
  if (!all) return { to, cc: [] }
  const taken = new Set([...me, ...to.map((a) => lower(a.email))])
  return { to, cc: unique(fromMe ? m.cc : [...m.to, ...m.cc], taken) }
}

export function quoteHtml(header: string, m: Message): string {
  return `<p>${escapeHtml(header)}</p><blockquote>${textToHtml(m.text ?? '')}</blockquote>`
}

export function forwardHtml(title: string, lines: [string, string][], m: Message): string {
  const head = lines.filter(([, v]) => v).map(([k, v]) => `${escapeHtml(k)} ${escapeHtml(v)}`)
  return `<p>---------- ${escapeHtml(title)} ----------<br>${head.join('<br>')}</p>${textToHtml(m.text ?? '')}`
}

export function addrText(a: Addr): string {
  return a.name ? `${a.name} <${a.email}>` : a.email
}

/**
 * Lee lo que se escribe en un campo de destinatarios: «Ana <ana@x.es>», «ana@x.es» o
 * varias separadas por comas o punto y coma.
 */
export function parseAddresses(input: string): { ok: Addr[]; bad: string[] } {
  const ok: Addr[] = []
  const bad: string[] = []
  for (const part of input.split(/[,;\n]/)) {
    const p = part.trim()
    if (!p) continue
    const m = p.match(/^(.*?)\s*<([^<>\s]+)>$/)
    const email = (m ? m[2] : p).trim()
    const name = m ? m[1].replace(/^"|"$/g, '').trim() : ''
    if (isEmail(email)) ok.push({ name: name || null, email })
    else bad.push(p)
  }
  return { ok, bad }
}

export function isEmail(e: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(e)
}

// ---------- Documento del editor ----------

/** Forma mínima del JSON de TipTap/ProseMirror. */
export interface DocNode {
  type?: string
  text?: string
  attrs?: Record<string, unknown>
  marks?: { type: string; attrs?: Record<string, unknown> }[]
  content?: DocNode[]
}

function inline(nodes: DocNode[] = []): string {
  return nodes
    .map((n) => {
      if (n.type === 'hardBreak') return '\n'
      const text = n.text ?? ''
      const href = n.marks?.find((m) => m.type === 'link')?.attrs?.href as string | undefined
      return href && href !== text && `mailto:${text}` !== href ? `${text} <${href}>` : text
    })
    .join('')
}

function blocks(nodes: DocNode[] = []): string[] {
  const out: string[] = []
  for (const n of nodes) {
    switch (n.type) {
      case 'paragraph':
      case 'heading':
      case 'codeBlock':
        out.push(inline(n.content))
        break
      case 'blockquote':
        out.push(blocks(n.content).join('\n').split('\n').map((l) => (l ? `> ${l}` : '>')).join('\n'))
        break
      case 'bulletList':
      case 'orderedList':
        (n.content ?? []).forEach((li, i) => {
          const mark = n.type === 'bulletList' ? '- ' : `${Number(n.attrs?.start ?? 1) + i}. `
          const body = blocks(li.content).join('\n').split('\n')
          out.push(body.map((l, j) => (j ? ' '.repeat(mark.length) : mark) + l).join('\n'))
        })
        break
      case 'horizontalRule':
        out.push('---')
        break
      default:
        if (n.content) out.push(...blocks(n.content))
        else if (n.text) out.push(n.text)
    }
  }
  return out
}

/** Versión en texto plano del correo, para quien no lea HTML. */
export function textFromDoc(doc: DocNode): string {
  return blocks(doc.content).join('\n').replace(/\n{3,}/g, '\n\n').trim() + '\n'
}

function isSignature(n: DocNode): boolean {
  return n.type === 'paragraph' && (n.content?.[0]?.text ?? '').trim() === '--' && n.content?.[1]?.type === 'hardBreak'
}

const sameText = (a: string, b: string) => a.replace(/\s+/g, ' ').trim() === b.replace(/\s+/g, ' ').trim()

/**
 * Cambia la firma al cambiar de remitente. Solo toca la primera, y solo si sigue tal cual
 * (`previous`): si se escribió algo en ella, no se pierde.
 */
export function swapSignature(doc: DocNode, previous: string, signature: string): DocNode {
  const content = [...(doc.content ?? [])]
  const lines = signature.trim() ? signature.trim().split('\n') : []
  const node: DocNode = {
    type: 'paragraph',
    content: [{ type: 'text', text: '-- ' }, ...lines.flatMap((l) => [{ type: 'hardBreak' }, ...(l ? [{ type: 'text', text: l }] : [])])],
  }
  const at = content.findIndex(isSignature)
  if (at >= 0 && !sameText(inline(content[at].content), `-- \n${previous}`)) return doc
  if (at >= 0) {
    if (lines.length) content[at] = node
    else content.splice(at, 1)
  } else if (lines.length) {
    // Antes de la cita, si la hay; si no, al final.
    const quote = content.findIndex((n) => n.type === 'blockquote' || (n.content?.[0]?.text ?? '').startsWith('----------'))
    const before = quote > 0 && content[quote - 1].type === 'paragraph' && content[quote].type === 'blockquote' ? quote - 1 : quote
    content.splice(before >= 0 ? before : content.length, 0, node)
  }
  return { ...doc, content }
}

/** Los párrafos vacíos no ocupan sitio en otros programas: se rellenan con un salto. */
export function finalHtml(html: string): string {
  return html.replace(/<p><\/p>/g, '<p><br></p>')
}
