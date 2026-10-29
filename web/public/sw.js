// Service worker de Buzón: permite instalar la app y abrirla sin conexión
// (muestra la última versión de la interfaz; el correo siempre se pide al servidor).
const CACHE = 'buzon-v1'

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(['/', '/icon.svg'])).then(() => self.skipWaiting()))
})

self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches.keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  )
})

self.addEventListener('fetch', (e) => {
  const url = new URL(e.request.url)
  if (e.request.method !== 'GET' || url.origin !== self.location.origin || url.pathname.startsWith('/api/')) return
  // Ficheros con hash en el nombre: nunca cambian.
  if (url.pathname.startsWith('/assets/')) {
    e.respondWith(caches.match(e.request).then((hit) => hit || fetch(e.request).then((res) => {
      if (res.ok) { const copy = res.clone(); caches.open(CACHE).then((c) => c.put(e.request, copy)) }
      return res
    })))
    return
  }
  // Páginas: primero la red; sin conexión, la última copia.
  if (e.request.mode === 'navigate') {
    e.respondWith(fetch(e.request).then((res) => {
      if (res.ok) { const copy = res.clone(); caches.open(CACHE).then((c) => c.put('/', copy)) }
      return res
    }).catch(() => caches.match('/')))
  }
})

// Al tocar una notificación, abre (o enfoca) Buzón en ese hilo.
self.addEventListener('notificationclick', (e) => {
  e.notification.close()
  const target = e.notification.data && e.notification.data.url ? e.notification.data.url : '/'
  e.waitUntil(self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((list) => {
    for (const c of list) {
      if ('focus' in c) { c.navigate(target); return c.focus() }
    }
    return self.clients.openWindow(target)
  }))
})
