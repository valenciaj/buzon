// Iconos de trazo, del mismo estilo que el prototipo.

const base = {
  width: 18, height: 18, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor',
  strokeWidth: 1.8, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const, 'aria-hidden': true,
}

const PATHS: Record<string, React.ReactNode> = {
  inbox: <><path d="M3 13h5l1.5 3h5L16 13h5" /><path d="M5 5h14l2 8v6H3v-6z" /></>,
  sent: <><path d="M21 3 10 14" /><path d="M21 3l-7 18-4-7-7-4z" /></>,
  drafts: <><path d="M4 20h4L19 9l-4-4L4 16z" /><path d="M13.5 6.5l4 4" /></>,
  archive: <><rect x="3" y="4" width="18" height="5" rx="1" /><path d="M5 9v10h14V9M10 13h4" /></>,
  junk: <><circle cx="12" cy="12" r="9" /><path d="M5.6 5.6l12.8 12.8" /></>,
  trash: <><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" /></>,
  folder: <path d="M3 6h6l2 2h10v11H3z" />,
  menu: <path d="M4 7h16M4 12h16M4 17h16" />,
  back: <path d="M15 5l-7 7 7 7" />,
  star: <path d="M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z" />,
  clip: <path d="M20 11l-8.5 8.5a5 5 0 0 1-7-7L13 4a3.5 3.5 0 0 1 5 5l-8.5 8.5a2 2 0 0 1-3-3L14 7" />,
  unread: <><rect x="3" y="5" width="18" height="14" rx="2" /><path d="M3.5 7l8.5 6 8.5-6" /></>,
  shield: <><path d="M12 3l8 3v6c0 4.5-3.4 8.3-8 9-4.6-.7-8-4.5-8-9V6z" /><path d="M9 12l2 2 4-4" /></>,
  sync: <><path d="M20 11a8 8 0 0 0-14.9-3M4 13a8 8 0 0 0 14.9 3" /><path d="M4 4v4h4M20 20v-4h-4" /></>,
  alert: <><path d="M12 3l10 18H2z" /><path d="M12 10v4M12 17.5v.5" /></>,
  download: <><path d="M12 4v11M7 10l5 5 5-5" /><path d="M5 20h14" /></>,
  compose: <><path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" /></>,
  reply: <><path d="M9 14 4 9l5-5" /><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" /></>,
  replyAll: <><path d="M7 14 2 9l5-5" /><path d="M12 14 7 9l5-5" /><path d="M7 9h8.5a5.5 5.5 0 0 1 0 11H13" /></>,
  forward: <><path d="M15 14l5-5-5-5" /><path d="M20 9H9.5a5.5 5.5 0 0 0 0 11H13" /></>,
  close: <path d="M6 6l12 12M18 6 6 18" />,
  bold: <path d="M7 5h6a3.5 3.5 0 0 1 0 7H7zM7 12h7a3.5 3.5 0 0 1 0 7H7z" />,
  italic: <path d="M10 5h8M6 19h8M14 5l-4 14" />,
  list: <><path d="M9 6h11M9 12h11M9 18h11" /><circle cx="4.5" cy="6" r=".8" /><circle cx="4.5" cy="12" r=".8" /><circle cx="4.5" cy="18" r=".8" /></>,
  olist: <><path d="M10 6h10M10 12h10M10 18h10" /><path d="M4 5l1.5-1v5M3.5 14.5a1.5 1.5 0 1 1 2.4 1.2L3.5 18h3" /></>,
  quote: <path d="M7 7h4v4c0 3-1.5 5-4 6M15 7h4v4c0 3-1.5 5-4 6" />,
  tag: <><path d="M3 12V4h8l10 10-8 8z" /><circle cx="7.5" cy="8.5" r="1.3" /></>,
  check: <path d="M5 12.5l4.5 4.5L19 7" />,
  search: <><circle cx="11" cy="11" r="6.5" /><path d="M16 16l5 5" /></>,
  contacts: <><circle cx="12" cy="8.5" r="3.5" /><path d="M5 20a7 7 0 0 1 14 0" /></>,
  keyboard: <><rect x="2.5" y="6" width="19" height="12" rx="2" /><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10" /></>,
  sliders: <><path d="M4 7h10M18 7h2M4 17h4M12 17h8" /><circle cx="16" cy="7" r="2" /><circle cx="10" cy="17" r="2" /></>,
  clock: <><circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" /></>,
  later: <><path d="M9 14 4 9l5-5" /><path d="M4 9h9a6 6 0 0 1 6 6v1" /><circle cx="18.5" cy="19" r=".6" /></>,
  pin: <><path d="M9 4h6l-1 6 3 3H7l3-3z" /><path d="M12 13v7" /></>,
  note: <><path d="M5 4h14v11l-5 5H5z" /><path d="M14 20v-5h5M8.5 9h7M8.5 12.5h4" /></>,
  screener: <><circle cx="10" cy="8.5" r="3.5" /><path d="M3.5 20a6.5 6.5 0 0 1 11.2-4.5" /><path d="M16 18l2 2 4-4" /></>,
  news: <><rect x="3.5" y="4.5" width="17" height="15" rx="2" /><path d="M7 9h10M7 12.5h10M7 16h6" /></>,
  receipt: <><path d="M6 3h12v18l-3-2-3 2-3-2-3 2z" /><path d="M9 8h6M9 12h6" /></>,
  link: <><path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" /><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" /></>,
}

export function Icon({ name, filled, className }: { name: keyof typeof PATHS | string; filled?: boolean; className?: string }) {
  return (
    <svg {...base} className={className} fill={filled ? 'currentColor' : 'none'}>
      {PATHS[name] ?? PATHS.folder}
    </svg>
  )
}
