// Genera los PDF del manual a partir de la web ya compilada (npm run build):
// web/dist/manual/buzon.pdf (español) y buzon-en.pdf (inglés).
// Necesita Chromium de Playwright: npx playwright install chromium
import { createServer } from 'node:http'
import { readFile } from 'node:fs/promises'
import { extname, join, normalize } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright'

const root = fileURLToPath(new URL('../../../web/dist/manual/', import.meta.url))
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.woff': 'font/woff', '.json': 'application/json' }

// Sirve el manual en /manual/ igual que Buzón.
const server = createServer(async (req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname)
  const rel = normalize(path.replace(/^\/manual\//, '')).replace(/^(\.\.[/\\])+/, '')
  try {
    const body = await readFile(join(root, rel.endsWith('/') || rel === '.' ? join(rel, 'index.html') : rel))
    res.writeHead(200, { 'content-type': types[extname(rel)] ?? 'application/octet-stream' })
    res.end(body)
  } catch {
    res.writeHead(404).end()
  }
})
await new Promise((r) => server.listen(0, '127.0.0.1', r))
const base = `http://127.0.0.1:${server.address().port}/manual/`

const browser = await chromium.launch()
try {
  for (const [page, out, footer] of [
    ['imprimir.html', 'buzon.pdf', 'Buzón · Manual'],
    ['en/print.html', 'buzon-en.pdf', 'Buzón · Manual'],
  ]) {
    const p = await browser.newPage()
    await p.emulateMedia({ media: 'print', colorScheme: 'light' })
    await p.goto(base + page, { waitUntil: 'networkidle' })
    await p.evaluate(() => document.fonts.ready)
    await p.pdf({
      path: join(root, out),
      format: 'A4',
      printBackground: true,
      margin: { top: '18mm', bottom: '18mm', left: '16mm', right: '16mm' },
      displayHeaderFooter: true,
      headerTemplate: '<span></span>',
      footerTemplate: `<div style="width:100%;font-size:8pt;color:#586374;padding:0 16mm;display:flex;justify-content:space-between"><span>${footer}</span><span class="pageNumber"></span></div>`,
    })
    console.log(`PDF: ${join(root, out)}`)
    await p.close()
  }
} finally {
  await browser.close()
  server.close()
}
