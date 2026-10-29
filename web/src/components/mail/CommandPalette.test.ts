import { describe, expect, it } from 'vitest'
import { filterCommands, normalize, type Command } from './CommandPalette'

const cmd = (id: string, label: string, keywords?: string): Command =>
  ({ id, label, keywords, group: 'Ir a', icon: 'inbox', run: () => undefined })

describe('paleta de órdenes', () => {
  const list = [cmd('trash', 'Papelera'), cmd('archive', 'Archivo'), cmd('theme', 'Tema: oscuro', 'theme dark'), cmd('drafts', 'Borradores')]

  it('ignora mayúsculas y tildes', () => {
    expect(normalize('Ápice ÑANDÚ')).toBe('apice nandu')
    expect(filterCommands(list, 'papeléra').map((c) => c.id)).toEqual(['trash'])
  })

  it('busca en las palabras clave y pone primero lo que empieza igual', () => {
    expect(filterCommands(list, 'dark').map((c) => c.id)).toEqual(['theme'])
    expect(filterCommands(list, 'ar').map((c) => c.id)).toEqual(['archive', 'theme'])
    expect(filterCommands(list, 'bo ra').map((c) => c.id)).toEqual(['drafts'])
    expect(filterCommands(list, '')).toHaveLength(4)
  })
})
