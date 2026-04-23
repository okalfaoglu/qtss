# qtss-web

React shell for the QTSS v2 console. Talks to `qtss-api` over the v2 OAuth
flow and renders the Dashboard / Risk / Strategy panels backed by the
`qtss-gui-api` DTOs.

This app is intentionally separate from the legacy `web/` project; the
v1 console keeps running unchanged while v2 is built out.

## Dev

```bash
npm install
npm run dev          # http://localhost:5174 (proxies /api and /oauth to :8080)
```

Override the API target with `VITE_API_URL=http://host:port npm run dev`.

## Build

```bash
npm run build        # tsc -b && vite build → dist/
```
