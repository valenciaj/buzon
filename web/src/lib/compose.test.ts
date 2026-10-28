import { describe, expect, it } from 'vitest'
import type { Identity, Message } from './api'
import {
  parseAddresses, pickIdentity, prefixSubject, replyRecipients, signatureHtml, swapSignature, textFromDoc, textToHtml,
} from './compose'

const msg = (over: Partial<Message> = {}): Message => ({
  id: 'm', mailbox_id: 'b', subject: 'Cena', from: { name: 'Ana', email: 'ana@x.es' },
  to: [{ name: null, email: 'yo@x.es' }, { name: 'Luis', email: 'luis@x.es' }], cc: [{ name: null, email: 'eva@x.es' }],
  date: 0, seen: true, flagged: false, answered: false, text: 'Hola\n\n¿Vienes?', html: null, remote_images: 0, attachments: [],
  ...over,
})

const ident = (id: string, email: string, account: string, is_default = false): Identity =>
  ({ id, email, account_id: account, name: '', signature: '', is_default })

describe('responder', () => {
  it('elige destinatarios', () => {
    expect(replyRecipients(msg(), false, ['YO@x.es'])).toEqual({ to: [{ name: 'Ana', email: 'ana@x.es' }], cc: [] })
    const all = replyRecipients(msg(), true, ['yo@x.es'])
    expect(all.to.map((a) => a.email)).toEqual(['ana@x.es'])
    expect(all.cc.map((a) => a.email)).toEqual(['luis@x.es', 'eva@x.es'])
    // Respondo a algo mío: vuelve a los mismos destinatarios.
    const mine = replyRecipients(msg({ from: { name: null, email: 'yo@x.es' } }), true, ['yo@x.es'])
    expect(mine.to.map((a) => a.email)).toEqual(['luis@x.es'])
    expect(mine.cc.map((a) => a.email)).toEqual(['eva@x.es'])
  })

  it('elige identidad y asunto', () => {
    const ids = [ident('a', 'yo@otra.es', 'A', true), ident('b', 'yo@x.es', 'B'), ident('c', 'c@x.es', 'B', true)]
    expect(pickIdentity(ids, msg())?.id).toBe('b')
    expect(pickIdentity(ids, msg({ to: [], cc: [] }), 'B')?.id).toBe('c')
    expect(pickIdentity(ids, null)?.id).toBe('a')
    expect(prefixSubject('Re', 'Cena')).toBe('Re: Cena')
    expect(prefixSubject('Re', 'RE: Cena')).toBe('RE: Cena')
    expect(prefixSubject('Fwd', 'Fw: Cena')).toBe('Fw: Cena')
  })
})

describe('direcciones', () => {
  it('lee varias', () => {
    const r = parseAddresses('Ana García <ana@x.es>, luis@x.es; nada')
    expect(r.ok).toEqual([{ name: 'Ana García', email: 'ana@x.es' }, { name: null, email: 'luis@x.es' }])
    expect(r.bad).toEqual(['nada'])
  })
})

describe('texto y firma', () => {
  it('escapa el texto citado', () => {
    expect(textToHtml('a <b>\nc\n\n\nd')).toBe('<p>a &lt;b&gt;<br>c</p><p>d</p>')
    expect(signatureHtml('')).toBe('')
    expect(signatureHtml('Javi\nVoiper')).toBe('<p>-- <br>Javi<br>Voiper</p>')
  })

  const doc = {
    type: 'doc',
    content: [
      { type: 'paragraph', content: [{ type: 'text', text: 'Mira ' }, { type: 'text', text: 'esto', marks: [{ type: 'link', attrs: { href: 'https://x.es' } }] }] },
      { type: 'bulletList', content: [{ type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'uno' }] }] }] },
      { type: 'paragraph' },
      { type: 'paragraph', content: [{ type: 'text', text: '-- ' }, { type: 'hardBreak' }, { type: 'text', text: 'Javi' }] },
      { type: 'blockquote', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Hola' }] }, { type: 'paragraph', content: [{ type: 'text', text: 'adiós' }] }] },
    ],
  }

  it('saca el texto plano', () => {
    expect(textFromDoc(doc)).toBe('Mira esto <https://x.es>\n- uno\n\n-- \nJavi\n> Hola\n> adiós\n')
  })

  it('cambia o quita la firma', () => {
    const swapped = swapSignature(doc, 'Javi', 'Javier\nVoiper')
    expect(textFromDoc(swapped)).toContain('-- \nJavier\nVoiper\n> Hola')
    // Si se ha escrito en la firma, se deja como está.
    expect(swapSignature(doc, 'Otra', 'Javier')).toBe(doc)
    const none = swapSignature(doc, 'Javi', '')
    expect(textFromDoc(none)).not.toContain('--')
    const added = swapSignature(none, 'Javi', 'J')
    expect(textFromDoc(added)).toContain('-- \nJ\n\n> Hola')
  })
})
