import { describe, expect, it } from 'vitest'
import { EMPTY_ACCOUNT, guessFromEmail } from './AccountForm'

describe('guessFromEmail', () => {
  it('rellena servidores y usuario desde la dirección', () => {
    const a = guessFromEmail(EMPTY_ACCOUNT, 'yo@ejemplo.es')
    expect(a.imap_host).toBe('imap.ejemplo.es')
    expect(a.smtp_host).toBe('smtp.ejemplo.es')
    expect(a.login).toBe('yo@ejemplo.es')
  })
  it('no pisa lo que la persona escribió a mano', () => {
    const a = guessFromEmail({ ...EMPTY_ACCOUNT, email: 'yo@a.es', imap_host: 'mail.a.es', login: 'yo' }, 'yo@b.es')
    expect(a.imap_host).toBe('mail.a.es')
    expect(a.login).toBe('yo')
    expect(a.smtp_host).toBe('smtp.b.es')
  })
})
