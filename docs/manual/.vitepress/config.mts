import { defineConfig } from 'vitepress'

// El manual se sirve en /manual desde la propia instalación de Buzón (web/dist/manual).
export default defineConfig({
  base: '/manual/',
  outDir: '../../web/dist/manual',
  title: 'Buzón',
  cleanUrls: false,
  lastUpdated: false,
  srcExclude: ['README.md'],
  head: [['link', { rel: 'icon', href: '/manual/icon.svg' }]],
  themeConfig: {
    logo: '/img/logo.svg',
    search: {
      provider: 'local',
      options: {
        locales: {
          root: {
            translations: {
              button: { buttonText: 'Buscar', buttonAriaLabel: 'Buscar' },
              modal: {
                displayDetails: 'Ver detalles',
                resetButtonTitle: 'Borrar búsqueda',
                backButtonTitle: 'Cerrar',
                noResultsText: 'Sin resultados para',
                footer: { selectText: 'abrir', navigateText: 'moverse', closeText: 'cerrar' },
              },
            },
          },
        },
      },
    },
    socialLinks: [{ icon: 'github', link: 'https://github.com/valenciaj/buzon' }],
  },
  locales: {
    root: {
      label: 'Español',
      lang: 'es',
      description: 'Manual de Buzón, el webmail autoalojado',
      themeConfig: {
        nav: [
          { text: 'PDF', link: '/manual/buzon.pdf', target: '_blank' },
        ],
        sidebar: [
          {
            text: 'Para todos',
            items: [
              { text: 'Primeros pasos', link: '/primeros-pasos' },
              { text: 'Leer el correo', link: '/leer' },
              { text: 'Escribir', link: '/escribir' },
              { text: 'Organizar', link: '/organizar' },
              { text: 'Modo Enfoque', link: '/enfoque' },
              { text: 'Hoy, programados y avisos', link: '/extras' },
              { text: 'Ajustes', link: '/ajustes' },
              { text: 'Atajos de teclado', link: '/atajos' },
            ],
          },
          {
            text: 'Avanzado',
            items: [
              { text: 'API y webhooks', link: '/api' },
              { text: 'Administración', link: '/administracion' },
            ],
          },
        ],
        outline: { label: 'En esta página' },
        docFooter: { prev: 'Anterior', next: 'Siguiente' },
        darkModeSwitchLabel: 'Tema',
        sidebarMenuLabel: 'Menú',
        returnToTopLabel: 'Volver arriba',
        langMenuLabel: 'Idioma',
      },
    },
    en: {
      label: 'English',
      lang: 'en',
      link: '/en/',
      description: 'Manual for Buzón, the self-hosted webmail',
      themeConfig: {
        nav: [
          { text: 'PDF', link: '/manual/buzon-en.pdf', target: '_blank' },
        ],
        sidebar: [
          {
            text: 'For everyone',
            items: [
              { text: 'Getting started', link: '/en/getting-started' },
              { text: 'Reading mail', link: '/en/reading' },
              { text: 'Writing', link: '/en/writing' },
              { text: 'Organizing', link: '/en/organizing' },
              { text: 'Focus mode', link: '/en/focus' },
              { text: 'Today, scheduled and reminders', link: '/en/extras' },
              { text: 'Settings', link: '/en/settings' },
              { text: 'Keyboard shortcuts', link: '/en/shortcuts' },
            ],
          },
          {
            text: 'Advanced',
            items: [
              { text: 'API and webhooks', link: '/en/api' },
              { text: 'Administration', link: '/en/administration' },
            ],
          },
        ],
      },
    },
  },
})
