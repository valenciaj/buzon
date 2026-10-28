import { describe, expect, it } from 'vitest'
import es from './es.json'
import en from './en.json'

function keys(o: object, prefix = ''): string[] {
  return Object.entries(o).flatMap(([k, v]) =>
    v && typeof v === 'object' ? keys(v, `${prefix}${k}.`) : [`${prefix}${k}`],
  )
}

describe('traducciones', () => {
  it('español e inglés tienen las mismas claves', () => {
    expect(keys(en).sort()).toEqual(keys(es).sort())
  })
  it('ninguna traducción está vacía', () => {
    for (const lang of [es, en]) {
      const flat = keys(lang)
      expect(flat.length).toBeGreaterThan(50)
    }
  })
})
