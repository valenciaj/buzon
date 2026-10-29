// Capturas del manual sobre una instancia de demostración recién creada (ver demo/run.sh).
// Uso: node scripts/screenshots.mjs es|en
import { mkdir, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright'

const lang = process.argv[2] ?? 'es'
const B = process.env.BUZON_URL ?? 'http://127.0.0.1:8089'
const demo = fileURLToPath(new URL('../demo/', import.meta.url))
const out = fileURLToPath(new URL(`../public/img/${lang}/`, import.meta.url))
await mkdir(out, { recursive: true })
const es = lang === 'es'
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function api(method, path, body, cookie) {
  const res = await fetch(B + '/api/v1' + path, {
    method,
    headers: { 'content-type': 'application/json', ...(cookie ? { cookie } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${await res.text()}`)
  return { body: res.status === 204 ? null : await res.json(), cookie: res.headers.get('set-cookie')?.split(';')[0] }
}
async function until(what, fn, ms = 30000) {
  const t = Date.now()
  while (Date.now() - t < ms) {
    if (await fn()) return
    await sleep(300)
  }
  throw new Error('No llega: ' + what)
}

// 1. Asistente por la API, con la cuenta de demostración.
const setup = await api('POST', '/setup', {
  admin: { username: 'javier', password: 'una-contraseña-larga' },
  server: { language: lang, timezone: 'Europe/Madrid' },
  preferences: { theme: 'light' },
  account: {
    name: es ? 'Personal' : 'Personal', email: 'yo@ejemplo.es',
    imap_host: '127.0.0.1', imap_port: 3143, imap_security: 'none',
    smtp_host: '127.0.0.1', smtp_port: 3025, smtp_security: 'none',
    login: 'yo', password: 'clave',
  },
})
const cookie = setup.cookie
await until('primera sincronización', async () => (await api('GET', '/accounts', null, cookie)).body[0]?.state === 'idle')
// 2. Remitentes nuevos para «Por aprobar».
await writeFile(join(demo, 'deliver'), '')
await until('remitentes nuevos', async () => (await api('GET', '/flow', null, cookie)).body.screener >= 2)

const browser = await chromium.launch()
const [name, value] = cookie.split('=')
const context = async (viewport) => {
  const ctx = await browser.newContext({ viewport, deviceScaleFactor: 2, locale: es ? 'es-ES' : 'en-US' })
  await ctx.addCookies([{ name, value, url: B }])
  return ctx
}
const shot = async (page, file) => {
  await sleep(400)
  await page.screenshot({ path: join(out, file + '.png') })
  console.log(`${lang}/${file}.png`)
}
const t = (a, b) => (es ? a : b)

const ctx = await context({ width: 1280, height: 800 })
const page = await ctx.newPage()

// Inicio en Enfoque, con una conversación abierta.
await page.goto(B + '/')
await page.getByText(t('Cena del viernes', 'Dinner on Friday')).first().click()
await page.getByText(t('reservo mesa para cuatro', 'book a table for four')).first().waitFor()
await shot(page, 'inicio')

// Redactar.
await page.getByRole('button', { name: t('Redactar', 'Compose') }).first().click()
const dlg = page.getByRole('dialog')
await dlg.getByLabel(t('Para', 'To'), { exact: true }).fill('ana@ejemplo.es')
await page.keyboard.press('Enter')
await dlg.getByLabel(t('Asunto', 'Subject')).fill(t('Fotos de la cena', 'Photos from dinner'))
await dlg.locator('.ProseMirror').click()
await page.keyboard.type(t('¡Qué buena noche! Te paso las fotos.', 'What a great night! Here are the photos.'))
await shot(page, 'redactar')
await dlg.getByRole('button', { name: t('Descartar borrador', 'Discard draft') }).click()

// Por aprobar.
await page.goto(B + '/?b=v:screener')
await page.getByText(t('Propuesta de colaboración', 'Collaboration proposal')).first().waitFor()
await shot(page, 'por-aprobar')

// Hoy.
await page.goto(B + '/?b=p:today')
await page.getByRole('heading', { name: t('Hoy', 'Today') }).first().waitFor()
await shot(page, 'hoy')

// Paleta de órdenes.
await page.goto(B + '/')
await page.getByText(t('Cena del viernes', 'Dinner on Friday')).first().waitFor()
await page.keyboard.press('Control+k')
await page.keyboard.type(t('ir a', 'go to'))
await shot(page, 'paleta')
await page.keyboard.press('Escape')

// Ajustes.
await page.goto(B + '/settings/look')
await page.getByRole('heading', { name: t('Ajustes', 'Settings') }).waitFor()
await shot(page, 'apariencia')
await page.goto(B + '/settings/accounts')
await page.getByRole('heading', { name: t('Conectar sin contraseña', 'Connect without a password') }).waitFor()
await shot(page, 'cuentas')

// Móvil, en oscuro.
const mctx = await browser.newContext({
  viewport: { width: 390, height: 780 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true,
  locale: es ? 'es-ES' : 'en-US', colorScheme: 'dark',
})
await mctx.addCookies([{ name, value, url: B }])
await api('PUT', '/me/preferences', { theme: 'dark' }, cookie)
const m = await mctx.newPage()
await m.goto(B + '/')
await m.getByText(t('Cena del viernes', 'Dinner on Friday')).first().waitFor()
await shot(m, 'movil')
await api('PUT', '/me/preferences', { theme: 'light' }, cookie)

await browser.close()
