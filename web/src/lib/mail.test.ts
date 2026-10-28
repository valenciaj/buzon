import { describe, expect, it } from 'vitest'
import type { Mailbox } from './api'
import { boxKey, customFolders, dayGroup, emailDocument, fileSize, initials, linkify, parseBox, shortDate, unreadByRole } from './mail'

const mb = (p: Partial<Mailbox>): Mailbox => ({
  id: 'x', account_id: 'a', name: 'INBOX', delimiter: '/', role: null, total: 0, unread: 0, ...p,
})

describe('carpetas', () => {
  it('lee y escribe la carpeta de la URL', () => {
    expect(parseBox(null)).toEqual({ role: 'inbox' })
    expect(parseBox('sent')).toEqual({ role: 'sent' })
    expect(parseBox('m:abc')).toEqual({ mailbox: 'abc' })
    expect(parseBox('nada')).toEqual({ role: 'inbox' })
    expect(boxKey({ mailbox: 'abc' })).toBe('m:abc')
  })
  it('suma no leídos por tipo y lista las carpetas propias', () => {
    const list = [
      mb({ id: '1', role: 'inbox', unread: 2 }),
      mb({ id: '2', role: 'inbox', unread: 3, account_id: 'b' }),
      mb({ id: '3', name: 'Viajes' }),
      mb({ id: '4', name: 'Facturas' }),
    ]
    expect(unreadByRole(list).inbox).toBe(5)
    expect(unreadByRole(list, 'b').inbox).toBe(3)
    expect(customFolders(list).map((m) => m.name)).toEqual(['Facturas', 'Viajes'])
  })
})

describe('textos', () => {
  it('iniciales', () => {
    expect(initials('María López García')).toBe('MG')
    expect(initials('ana')).toBe('A')
    expect(initials('')).toBe('?')
  })
  it('fechas cortas y grupos por día', () => {
    const now = new Date(2026, 9, 28, 12, 0)
    const ts = (d: Date) => d.getTime() / 1000
    expect(shortDate(ts(new Date(2026, 9, 28, 9, 5)), 'es', now)).toBe('09:05')
    expect(shortDate(ts(new Date(2026, 9, 27, 9, 5)), 'es', now)).toBe('ayer')
    expect(shortDate(ts(new Date(2026, 2, 3)), 'es', now)).toBe('3 mar')
    expect(dayGroup(ts(new Date(2026, 9, 28, 1)), now)).toBe('today')
    expect(dayGroup(ts(new Date(2026, 9, 24)), now)).toBe('week')
    expect(dayGroup(ts(new Date(2026, 5, 1)), now)).toBe('older')
  })
  it('tamaños', () => {
    expect(fileSize(900, 'es')).toBe('900 B')
    expect(fileSize(1536, 'es')).toBe('1,5 KB')
  })
  it('enlaces en texto plano', () => {
    expect(linkify('Mira https://ejemplo.es/a. O escribe a yo@ejemplo.es')).toEqual([
      { text: 'Mira ' },
      { text: 'https://ejemplo.es/a', href: 'https://ejemplo.es/a' },
      { text: '. O escribe a ' },
      { text: 'yo@ejemplo.es', href: 'mailto:yo@ejemplo.es' },
    ])
  })
})

describe('lector', () => {
  it('bloquea lo remoto salvo que se pidan las imágenes', () => {
    const blocked = emailDocument('<p>hola</p>', { images: false, font: 'serif' })
    expect(blocked).toContain("img-src 'self' data:;")
    expect(blocked).toContain("default-src 'none'")
    expect(emailDocument('', { images: true, font: 'serif' })).toContain('https: http:')
  })
})
