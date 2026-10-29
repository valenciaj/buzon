# Manual de Buzón

Fuente del manual online (`/manual`) y en PDF (`/manual/buzon.pdf`, `/manual/buzon-en.pdf`). Español en la raíz, inglés en `en/`.

```sh
npm install
npm run dev          # vista previa con recarga
npm run build        # compila en web/dist/manual (después de compilar la web)
npm run pdf          # genera los PDF (necesita: npx playwright install chromium)
./demo/run.sh        # regenera las capturas de public/img/{es,en}
```

Al añadir un capítulo: créalo en los dos idiomas, añádelo a la barra lateral (`.vitepress/config.mts`) y a las páginas de impresión (`imprimir.md`, `en/print.md`).
