import { describe, expect, it } from 'vitest'
import { EMPTY_ACCOUNT, guessFromEmail, withSecurity } from './AccountForm'

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

describe('withSecurity', () => {
  it('ajusta el puerto habitual al cambiar el cifrado', () => {
    const a = withSecurity(EMPTY_ACCOUNT, 'imap', 'starttls')
    expect(a.imap_security).toBe('starttls')
    expect(a.imap_port).toBe(143)
    expect(withSecurity(EMPTY_ACCOUNT, 'smtp', 'starttls').smtp_port).toBe(587)
  })
  it('respeta un puerto escrito a mano', () => {
    const a = withSecurity({ ...EMPTY_ACCOUNT, imap_port: 1993 }, 'imap', 'starttls')
    expect(a.imap_port).toBe(1993)
  })
})
