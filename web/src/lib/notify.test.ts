import { describe, expect, it } from 'vitest'
import { shouldNotify, type NewMail } from './notify'

const mail = (status: NewMail['status'], bucket: NewMail['bucket']): NewMail =>
  ({ thread: 't', account: 'a', from: null, subject: 'Hola', snippet: '', status, bucket })

describe('notificaciones', () => {
  it('en Enfoque solo avisa de Principal', () => {
    expect(shouldNotify(mail('approved', 'principal'), 'focus')).toBe(true)
    expect(shouldNotify(mail('approved', 'news'), 'focus')).toBe(false)
    expect(shouldNotify(mail('pending', 'principal'), 'focus')).toBe(false)
  })

  it('en Clásico avisa de todo menos de lo rechazado', () => {
    expect(shouldNotify(mail('pending', 'news'), 'classic')).toBe(true)
    expect(shouldNotify(mail('blocked', 'principal'), 'classic')).toBe(false)
  })
})
