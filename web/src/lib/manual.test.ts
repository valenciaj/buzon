import { describe, expect, it } from 'vitest'
import { manualUrl } from './manual'

describe('manualUrl', () => {
  it('apunta al manual en el idioma de la interfaz', () => {
    expect(manualUrl('es')).toBe('/manual/')
    expect(manualUrl('en')).toBe('/manual/en/')
    expect(manualUrl('es', 'shortcuts')).toBe('/manual/atajos.html')
    expect(manualUrl('en-GB', 'settings', 'email-accounts')).toBe('/manual/en/settings.html#email-accounts')
  })
})
