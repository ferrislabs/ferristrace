/* eslint-disable */
export type Col = { label: string; cls: string; sort: 'none' | 'ascending' | 'descending' }
export type Cell = { v: string; cls: string; title: string; warn: boolean; bar?: number; barColor?: string }
export type SignalView = {
  title: string; subtitle: string; group: string[]
  kpis: [string, string, string, string, string][]
  chart: [string, string, string, string, string, string]
  a: (_i: number) => number; lo: number; hi: number; b: (_i: number) => number
  table: [string, string, string]; cols: Col[]; rows: Cell[][]; foot: string; empty: [string, string, string]
}
let seed = 5
const rnd = () => { seed = (seed * 9301 + 49297) % 233280; return seed / 233280 }
export const resetSeed = (s = 5) => { seed = s }
const C = (label: string, cls = '', sort: Col['sort'] = 'none'): Col => ({ label, cls, sort })
const X = (v: string, cls = '', o: { title?: string; warn?: boolean | number; bar?: number; barColor?: string } = {}): Cell => ({ v, cls, title: o.title || '', warn: !!o.warn, bar: o.bar, barColor: o.barColor })
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type R = any[]

export const BACKEND: Record<string, SignalView> = {
      requests: {
        title: 'Requests', subtitle: 'Incoming HTTP server spans, grouped by route', group: ['Route', 'Service'],
        kpis: [['Requests', '182/s', '+4 % vs previous hour', '', 'muted'], ['5xx', '1.9 %', '612 in range', 'warn-text', 'warn-text'], ['4xx', '3.2 %', 'mostly 401 and 404', '', 'muted'], ['p95', '212 ms', 'p99 1.1 s', '', 'muted']],
        chart: ['Requests per second', 'solid: all · dashed: 5xx ×20', '240', '120', '0', 'Requests steady near 180 per second; 5xx rise at 14:05'],
        a: (i: number) => 180 + Math.sin(i / 6) * 18 + (rnd() - 0.5) * 14, lo: 0, hi: 240, b: (i: number) => (i >= 33 ? 3.2 : 0.4) * 20 + (rnd() - 0.5) * 10,
        table: ['Routes', '42 routes · sorted by 5xx', 'Filter routes, e.g. /api/checkout'],
        cols: [C('Route'), C('Service', 'cw-130'), C('Requests', 'num cw-100'), C('2xx', 'num cw-80'), C('4xx', 'num cw-80'), C('5xx', 'num cw-90', 'descending'), C('p50', 'num cw-80'), C('p95', 'num cw-80'), C('Share of time', 'cw-140')],
        rows: [['POST /api/checkout', 'checkout-api', '18.2/s', '91.4 %', '0.6 %', '8.0 %', '210 ms', '1.9 s', 62, true], ['POST /api/payments/refund', 'api-gateway', '0.4/s', '94.1 %', '2.0 %', '3.9 %', '180 ms', '640 ms', 2, true], ['POST /api/login', 'auth', '6.8/s', '81.2 %', '18.1 %', '0.7 %', '24 ms', '96 ms', 3], ['GET /api/products', 'api-gateway', '72.0/s', '99.1 %', '0.8 %', '0.1 %', '18 ms', '61 ms', 14], ['GET /api/cart', 'api-gateway', '41.3/s', '99.6 %', '0.4 %', '0.0 %', '12 ms', '48 ms', 6], ['GET /api/orders/{id}', 'api-gateway', '9.1/s', '96.0 %', '4.0 %', '0.0 %', '31 ms', '88 ms', 4], ['GET /healthz', 'api-gateway', '2.0/s', '100 %', '0 %', '0 %', '0.8 ms', '1.2 ms', 0]].map((r: R) => [X(r[0], 'mono strong'), X(r[1], 'sm muted'), X(r[2], 'num mono'), X(r[3], 'num mono'), X(r[4], 'num mono'), X(r[5], 'num mono ' + (r[9] ? 'warn-text strong' : ''), { warn: r[9] }), X(r[6], 'num mono'), X(r[7], 'num mono'), X(r[8] + ' %', 'mono small', { bar: r[8] })]),
        foot: 'Routes come from http.route; without it, FerrisTrace groups by method and path with ids replaced. Click a route to see its traces.',
        empty: ['No HTTP requests yet', 'Requests appear once a service sends server spans with http.request.method and http.route. Most HTTP frameworks do it with their OpenTelemetry instrumentation.', 'npm i @opentelemetry/auto-instrumentations-node']
      },
      jobs: {
        title: 'Jobs', subtitle: 'Background work: consumer and scheduled spans', group: ['Job', 'Queue'],
        kpis: [['Processed', '38.2k', 'last hour', '', 'muted'], ['Failed', '214', '0.6 % · 41 after retries', 'warn-text', 'warn-text'], ['Wait time p95', '2.4 s', 'time spent queued', '', 'muted'], ['Scheduled', '12', 'next: expire_carts in 4 min', '', 'muted']],
        chart: ['Jobs processed per minute', 'solid: processed · dashed: failed ×50', '900', '450', '0', 'About 640 jobs per minute; failures rise slightly at 14:05'],
        a: (i: number) => 640 + Math.sin(i / 5) * 80 + (rnd() - 0.5) * 60, lo: 0, hi: 900, b: (i: number) => (i >= 33 ? 6 : 2) * 50 + (rnd() - 0.5) * 60,
        table: ['Jobs', '18 jobs', 'Filter jobs'],
        cols: [C('Job'), C('Queue', 'cw-120'), C('Processed', 'num cw-100'), C('Failed', 'num cw-90', 'descending'), C('Retries', 'num cw-80'), C('Wait p95', 'num cw-90'), C('Duration p95', 'num cw-110'), C('Last run', 'cw-110')],
        rows: [['payments.capture', 'payments', '4,812', '96', '211', '3.1 s', '1.4 s', '14:32:01', true], ['notifier.send_email', 'mail', '9,140', '64', '88', '1.2 s', '881 ms', '14:32:02', true], ['inventory.reserve', 'default', '12,330', '31', '40', '0.4 s', '154 ms', '14:32:00'], ['webhooks.deliver', 'webhooks', '6,021', '23', '310', '0.9 s', '420 ms', '14:09:12'], ['checkout.expire_carts', 'scheduled', '12', '0', '0', '—', '4.9 s', '14:30:00'], ['search.reindex', 'scheduled', '1', '0', '0', '—', '2 m 14 s', '13:00:00']].map((r: R) => [X(r[0], 'mono strong'), X(r[1], 'mono sm muted'), X(r[2], 'num mono'), X(r[3], 'num mono ' + (r[8] ? 'warn-text strong' : ''), { warn: r[8] }), X(r[4], 'num mono'), X(r[5], 'num mono'), X(r[6], 'num mono'), X(r[7], 'mono sm')]),
        foot: 'A job is a span with messaging.operation = process, or a span you mark with ferristrace.job = true. Scheduled jobs show their next run.',
        empty: ['No jobs yet', 'Jobs appear when your workers send consumer spans (messaging.operation = process) or spans tagged ferristrace.job.', 'span.setAttribute("ferristrace.job", true)']
      },
      cache: {
        title: 'Cache', subtitle: 'Cache reads and writes, from Redis and Memcached client spans', group: ['Key pattern', 'Store'],
        kpis: [['Hit rate', '93.4 %', '−1.2 pts vs previous hour', '', 'muted'], ['Operations', '2,410/s', '81 % reads', '', 'muted'], ['Misses', '134/s', 'most on cart:*', 'warn-text', 'warn-text'], ['p95', '1.8 ms', 'p99 6 ms', '', 'muted']],
        chart: ['Hit rate', 'solid: hit rate · dashed: 90 % target', '100 %', '90 %', '80 %', 'Hit rate between 92 and 95 percent, dipping at 14:05'],
        a: (i: number) => (i >= 33 ? 92 : 94.6) + (rnd() - 0.5) * 1.6, lo: 80, hi: 100, b: () => 90,
        table: ['Key patterns', 'keys with ids replaced by *', 'Filter keys'],
        cols: [C('Key pattern'), C('Store', 'cw-110'), C('Hits', 'num cw-100'), C('Misses', 'num cw-100'), C('Hit rate', 'cw-160', 'ascending'), C('Writes', 'num cw-90'), C('p95', 'num cw-80')],
        rows: [['cart:*', 'redis', '142/s', '61/s', 70, '58/s', '2.1 ms', true], ['psp:token:*', 'redis', '9.1/s', '3.2/s', 74, '3.2/s', '1.9 ms', true], ['session:*', 'redis', '610/s', '12/s', 98, '14/s', '1.1 ms'], ['product:*', 'redis', '1,120/s', '38/s', 97, '6/s', '1.4 ms'], ['rate:*', 'redis', '380/s', '0/s', 100, '380/s', '0.6 ms'], ['tpl:*', 'memcached', '96/s', '20/s', 83, '1/s', '0.9 ms']].map((r: R) => [X(r[0], 'mono strong'), X(r[1], 'mono sm muted'), X(r[2], 'num mono'), X(r[3], 'num mono'), X(r[4] + ' %', 'mono sm ' + (r[7] ? 'warn-text strong' : ''), { bar: r[4], barColor: r[7] ? 'bar-w' : 'bar-o', warn: r[7] }), X(r[5], 'num mono'), X(r[6], 'num mono')]),
        foot: 'Hits and misses come from db.system = redis or memcached spans; a GET that returns nothing counts as a miss. Amber under 80 %.',
        empty: ['No cache operations yet', 'Cache appears once your Redis or Memcached client is instrumented. Most client libraries have an OpenTelemetry instrumentation.', 'npm i @opentelemetry/instrumentation-ioredis']
      },
      emails: {
        title: 'Emails', subtitle: 'Outgoing mail, from mailer spans', group: ['Template', 'Provider'],
        kpis: [['Sent', '3,904', 'last hour', '', 'muted'], ['Failed', '64', '1.6 % · SMTP 421 from provider', 'warn-text', 'warn-text'], ['Send time p95', '880 ms', 'provider round trip', '', 'muted'], ['Queued now', '12', 'oldest 40 s', '', 'muted']],
        chart: ['Emails sent per minute', 'solid: sent · dashed: failed ×20', '100', '50', '0', 'About 65 emails per minute; failures appear after 14:10'],
        a: (i: number) => 65 + Math.sin(i / 7) * 10 + (rnd() - 0.5) * 8, lo: 0, hi: 100, b: (i: number) => (i >= 38 ? 2.5 : 0.2) * 20 + (rnd() - 0.5) * 4,
        table: ['Templates', '9 templates', 'Filter templates'],
        cols: [C('Template'), C('Provider', 'cw-120'), C('Sent', 'num cw-90'), C('Failed', 'num cw-90', 'descending'), C('Send p95', 'num cw-90'), C('Last sent', 'cw-110')],
        rows: [['order_confirmation', 'smtp · mailgun', '1,210', '41', '940 ms', '14:32:02', true], ['password_reset', 'smtp · mailgun', '88', '9', '820 ms', '14:31:40', true], ['shipping_update', 'smtp · mailgun', '1,402', '8', '860 ms', '14:32:01'], ['cart_reminder', 'smtp · mailgun', '960', '6', '790 ms', '14:30:00'], ['invoice', 'smtp · mailgun', '244', '0', '1.1 s', '14:12:44']].map((r: R) => [X(r[0], 'mono strong'), X(r[1], 'mono sm muted'), X(r[2], 'num mono'), X(r[3], 'num mono ' + (r[6] ? 'warn-text strong' : ''), { warn: r[6] }), X(r[4], 'num mono'), X(r[5], 'mono sm')]),
        foot: 'FerrisTrace records the template, recipient domain and result, never the address or the body. Recipients show as *@domain.',
        empty: ['No emails yet', 'Emails appear when your mailer sends a span named email.send with ferristrace.email.template. Addresses and bodies are not recorded.', 'span.setAttribute("ferristrace.email.template", "order_confirmation")']
      },
      queries: {
        title: 'Queries', subtitle: 'Database statements, from db client spans', group: ['Statement', 'Database'],
        kpis: [['Queries', '1,340/s', 'postgresql 92 %', '', 'muted'], ['Slow', '38/min', 'over 100 ms', 'warn-text', 'warn-text'], ['p95', '9.4 ms', 'p99 62 ms', '', 'muted'], ['Possible N+1', '3', 'same statement 20+ times in one trace', 'warn-text', 'warn-text']],
        chart: ['Queries per second', 'solid: all · dashed: over 100 ms ×20', '1.8k', '900', '0', 'About 1,340 queries per second, steady'],
        a: (i: number) => 1340 + Math.sin(i / 6) * 90 + (rnd() - 0.5) * 60, lo: 0, hi: 1800, b: (_i: number) => 0.63 * 20 * 20 + (rnd() - 0.5) * 60,
        table: ['Statements', 'normalized, values replaced by ?', 'Filter statements'],
        cols: [C('Statement'), C('Database', 'cw-110'), C('Calls', 'num cw-90'), C('Total time', 'cw-160', 'descending'), C('p50', 'num cw-80'), C('p95', 'num cw-80'), C('Rows avg', 'num cw-90')],
        rows: [['SELECT * FROM orders WHERE customer_id = ? ORDER BY created_at DESC', 'orders', '52.0/s', 38, '1.8 ms', '9 ms', '24'], ['SELECT price FROM products WHERE sku = ?', 'catalog', '410/s', 24, '0.4 ms', '2 ms', '1', true], ['UPDATE carts SET items = ?, updated_at = ? WHERE id = ?', 'orders', '58/s', 14, '1.1 ms', '6 ms', '1'], ['SELECT * FROM stock WHERE sku IN (?)', 'inventory', '63/s', 11, '0.9 ms', '5 ms', '12'], ['INSERT INTO events (type, payload) VALUES (?, ?)', 'events', '120/s', 7, '0.5 ms', '3 ms', '1'], ['DELETE FROM carts WHERE expires_at < ?', 'orders', '0.02/s', 6, '1.9 s', '2.4 s', '3,210']].map((r: R) => [X(r[0], 'mono sm truncate', { title: r[0] }), X(r[1], 'mono sm muted'), X(r[2], 'num mono'), X(r[3] + ' %', 'mono small', { bar: r[3] }), X(r[4], 'num mono'), X(r[5], 'num mono'), X(r[6], 'num mono ' + (r[7] ? 'warn-text' : ''), { warn: r[7] })]),
        foot: 'Statements come from db.query.text, normalized. The warning marks a likely N+1: the same SELECT called 20+ times inside one trace.',
        empty: ['No queries yet', 'Queries appear once your database client is instrumented (pg, mysql2, prisma…). Statements are normalized; values are never stored.', 'npm i @opentelemetry/instrumentation-pg']
      },
      system: {
        title: 'System metrics', subtitle: 'Hosts and containers, from host and process metrics', group: ['Host', 'Service'],
        kpis: [['Hosts', '6', '4 app · 2 workers', '', 'muted'], ['CPU', '41 %', 'average · peak 78 % on app-3', '', 'muted'], ['Memory', '63 %', '38 of 60 GB', '', 'muted'], ['Disk', '71 %', 'worker-1 at 88 %', 'warn-text', 'warn-text']],
        chart: ['CPU, all hosts', 'solid: average · dashed: busiest host', '100 %', '50 %', '0', 'Average CPU near 40 percent; busiest host between 60 and 80 percent'],
        a: (i: number) => 41 + Math.sin(i / 8) * 6 + (rnd() - 0.5) * 4, lo: 0, hi: 100, b: (i: number) => 68 + Math.sin(i / 5) * 8 + (rnd() - 0.5) * 6,
        table: ['Hosts', 'from system.* and process.* metrics', 'Filter hosts'],
        cols: [C('Host'), C('Services', 'cw-160'), C('CPU', 'cw-140', 'descending'), C('Memory', 'cw-140'), C('Disk', 'cw-140'), C('Load 1m', 'num cw-80'), C('Up', 'cw-90')],
        rows: [['app-3', 'checkout-api, api-gateway', 78, 71, 52, '3.1', '12 d'], ['app-1', 'api-gateway, auth', 46, 62, 49, '1.8', '12 d'], ['app-2', 'api-gateway, inventory', 39, 58, 50, '1.5', '12 d'], ['app-4', 'payments', 33, 66, 47, '1.2', '3 d'], ['worker-1', 'notifier, webhook-sender', 28, 55, 88, '0.9', '40 d'], ['worker-2', 'data jobs', 22, 49, 61, '0.7', '40 d']].map((r: R) => [X(r[0], 'mono strong'), X(r[1], 'sm muted'), X(r[2] + ' %', 'mono small', { bar: r[2], barColor: r[2] >= 75 ? 'bar-w' : 'bar-p' }), X(r[3] + ' %', 'mono small', { bar: r[3], barColor: r[3] >= 85 ? 'bar-w' : 'bar-p' }), X(r[4] + ' %', 'mono small ' + (r[4] >= 85 ? 'warn-text strong' : ''), { bar: r[4], barColor: r[4] >= 85 ? 'bar-w' : 'bar-p', warn: r[4] >= 85 }), X(r[5], 'num mono'), X(r[6], 'mono sm')]),
        foot: 'Needs the hostmetrics receiver of the OpenTelemetry Collector, or the host metrics package of your SDK. Amber from 75 % CPU and 85 % disk.',
        empty: ['No system metrics yet', 'Add the hostmetrics receiver to your OpenTelemetry Collector and point its exporter at FerrisTrace.', 'receivers:\n  hostmetrics:\n    scrapers: [cpu, memory, disk, load, network]']
      }
    }

export const FRONTEND: Record<string, SignalView> = {
      pages: {
        title: 'Pages', subtitle: 'Page views and Core Web Vitals, from your users\' browsers', group: ['Route', 'Device'],
        kpis: [['Page views', '48.2k', 'last hour · 9.1k sessions', '', 'muted'], ['LCP p75', '2.9 s', 'Needs improvement · good under 2.5 s', 'warn-text', 'warn-text'], ['INP p75', '180 ms', 'Good · under 200 ms', 'ok-text', 'muted'], ['CLS p75', '0.04', 'Good · under 0.1', 'ok-text', 'muted']],
        chart: ['LCP p75', 'solid: LCP p75 · dashed: 2.5 s good threshold', '4 s', '2 s', '0', 'LCP p75 around 2.6 s, rising to 3.1 s after 14:05'],
        a: (i: number) => (i >= 33 ? 3.0 : 2.6) + (rnd() - 0.5) * 0.3, lo: 0, hi: 4, b: () => 2.5,
        table: ['Routes', '31 routes · sorted by LCP', 'Filter routes, e.g. /checkout'],
        cols: [C('Route'), C('Views', 'num cw-90'), C('LCP p75', 'num cw-100', 'descending'), C('INP p75', 'num cw-100'), C('CLS p75', 'num cw-90'), C('Good experience', 'cw-160'), C('JS errors', 'num cw-90')],
        rows: [['/checkout', '3,120', '4.1 s', '310 ms', '0.12', 48, '214', 1], ['/products/{sku}', '18,400', '2.8 s', '160 ms', '0.03', 71, '12', 1], ['/cart', '6,210', '2.2 s', '140 ms', '0.02', 86, '8'], ['/', '12,900', '1.9 s', '120 ms', '0.01', 91, '3'], ['/account/orders', '2,040', '1.7 s', '110 ms', '0.00', 94, '1'], ['/search', '5,530', '2.4 s', '230 ms', '0.05', 79, '6']].map((r: R) => [X(r[0], 'mono strong'), X(r[1], 'num mono'), X(r[2], 'num mono ' + (r[7] ? 'warn-text strong' : ''), { warn: r[7] }), X(r[3], 'num mono ' + (parseInt(r[3]) > 200 ? 'warn-text' : '')), X(r[4], 'num mono ' + (parseFloat(r[4]) > 0.1 ? 'warn-text' : '')), X(r[5] + ' %', 'mono small', { bar: r[5], barColor: r[5] < 75 ? 'bar-w' : 'bar-o' }), X(r[6], 'num mono')]),
        foot: 'p75 is what 3 users out of 4 get or better, as Google measures it. Good experience = share of views with all three vitals in the good range.',
        empty: ['No page views yet', 'Add the OpenTelemetry web SDK to your front end with an ingest key, then reload a page.', 'npm i @opentelemetry/sdk-trace-web @opentelemetry/auto-instrumentations-web']
      },
      errors: {
        title: 'Errors', subtitle: 'JavaScript errors and unhandled promise rejections', group: ['Message', 'Release'],
        kpis: [['Errors', '412', 'last hour · 61 sessions', 'warn-text', 'warn-text'], ['Sessions with errors', '0.7 %', '+0.5 pts vs previous hour', 'warn-text', 'warn-text'], ['New issues', '2', 'first seen after release 2.14.1', 'warn-text', 'warn-text'], ['Source maps', '2.14.1', 'uploaded · stacks are readable', 'ok-text', 'muted']],
        chart: ['Errors per minute', 'solid: errors · dashed: sessions with errors', '20', '10', '0', 'Around 2 errors per minute until 14:05, then about 11'],
        a: (i: number) => (i >= 33 ? 11 : 2) + (rnd() - 0.5) * 3, lo: 0, hi: 20, b: (i: number) => (i >= 33 ? 5 : 1) + (rnd() - 0.5) * 1.5,
        table: ['Issues', 'grouped by message and top frame', 'Filter messages'],
        cols: [C('Error'), C('Where', 'cw-180'), C('Count', 'num cw-80', 'descending'), C('Sessions', 'num cw-90'), C('Browsers', 'cw-130'), C('First seen', 'cw-110'), C('Status', 'cw-90')],
        rows: [['TypeError: Cannot read properties of undefined (reading \'token\')', 'checkout.tsx:142', '214', '38', 'all', '14:06 today', 'New', 1], ['Failed to fetch: POST /api/checkout', 'api.ts:58', '96', '27', 'all', '14:05 today', 'New', 1], ['ResizeObserver loop completed with undelivered notifications', 'vendor', '61', '40', 'Chrome', 'Sep 12', 'Ignored'], ['ChunkLoadError: Loading chunk 412 failed', 'router.ts:31', '22', '19', 'Safari 17', 'Oct 18', 'Open'], ['SecurityError: The operation is insecure', 'storage.ts:9', '19', '7', 'Firefox', 'Oct 2', 'Open']].map((r: R) => [X(r[0], 'mono sm ' + (r[7] ? 'err-text strong' : ''), { title: r[0] }), X(r[1], 'mono sm muted'), X(r[2], 'num mono'), X(r[3], 'num mono'), X(r[4], 'sm'), X(r[5], 'mono sm'), X(r[6], 'sm ' + (r[6] === 'New' ? 'warn-text strong' : 'muted'), { warn: r[6] === 'New' })]),
        foot: 'Errors are log records with exception.* attributes from the browser. Upload source maps at build time to see your own file and line.',
        empty: ['No front-end errors yet', 'Errors appear once the web SDK is set up. Upload source maps from your build so stacks point to your code.', 'npx ferristrace sourcemaps upload ./dist --release 2.14.1']
      },
      network: {
        title: 'Network', subtitle: 'fetch and XHR calls made by the browser, joined to their backend traces', group: ['Endpoint', 'Origin'],
        kpis: [['Calls', '96/s', 'from 9.1k sessions', '', 'muted'], ['Failed', '1.4 %', 'status 0 or 5xx', 'warn-text', 'warn-text'], ['p75 in the browser', '320 ms', 'server part 190 ms', '', 'muted'], ['Linked to a trace', '97 %', 'traceparent header sent', 'ok-text', 'muted']],
        chart: ['Duration p75 seen by users', 'solid: browser · dashed: server', '1.6 s', '800 ms', '0', 'Browser p75 near 300 ms, rising to 900 ms after 14:05; server part follows'],
        a: (i: number) => (i >= 33 ? 0.9 : 0.3) + (rnd() - 0.5) * 0.12, lo: 0, hi: 1.6, b: (i: number) => (i >= 33 ? 0.7 : 0.19) + (rnd() - 0.5) * 0.08,
        table: ['Endpoints', 'called from the browser', 'Filter endpoints'],
        cols: [C('Endpoint'), C('Calls', 'num cw-90'), C('Failed', 'num cw-90', 'descending'), C('Browser p75', 'num cw-110'), C('Server p75', 'num cw-110'), C('Network share', 'cw-160'), C('Trace', 'cw-90')],
        rows: [['POST /api/checkout', '3.1/s', '7.8 %', '2.1 s', '1.6 s', 24, 'Open', 1], ['GET /api/products', '38/s', '0.2 %', '140 ms', '61 ms', 56, 'Open'], ['GET /api/cart', '22/s', '0.1 %', '96 ms', '48 ms', 50, 'Open'], ['POST /api/login', '1.4/s', '0.9 %', '180 ms', '96 ms', 47, 'Open'], ['GET cdn.acme.dev/img/*', '30/s', '0.0 %', '210 ms', '—', 100, '—'], ['POST /collect (analytics)', '4.2/s', '2.3 %', '60 ms', '—', 100, '—', 1]].map((r: R) => [X(r[0], 'mono strong'), X(r[1], 'num mono'), X(r[2], 'num mono ' + (r[7] ? 'warn-text strong' : ''), { warn: r[7] }), X(r[3], 'num mono'), X(r[4], 'num mono'), X(r[5] + ' %', 'mono small', { bar: r[5] }), X(r[6], 'sm ' + (r[6] === 'Open' ? 'accent-text' : 'muted'))]),
        foot: 'Network share = time spent outside your servers (DNS, TLS, transfer). Calls to your API send traceparent, so each one opens its full backend trace.',
        empty: ['No browser calls yet', 'Enable fetch and XHR instrumentation and list your API origins so the browser sends traceparent to them.', "propagateTraceHeaderCorsUrls: [/api\\.acme\\.dev/]"]
      },
      sessions: {
        title: 'Sessions', subtitle: 'One visit, page by page, with its errors and slow calls', group: ['Session', 'User'],
        kpis: [['Sessions', '9,140', 'last hour', '', 'muted'], ['With errors', '0.7 %', '61 sessions', 'warn-text', 'warn-text'], ['Median length', '4 m 12 s', '5 pages', '', 'muted'], ['Rage clicks', '23', '3+ clicks in 1 s on one element', 'warn-text', 'warn-text']],
        chart: ['Active sessions', 'solid: active · dashed: with errors ×20', '1.2k', '600', '0', 'About 900 active sessions; sessions with errors rise after 14:05'],
        a: (i: number) => 900 + Math.sin(i / 8) * 90 + (rnd() - 0.5) * 50, lo: 0, hi: 1200, b: (i: number) => (i >= 33 ? 12 : 3) * 20 + (rnd() - 0.5) * 40,
        table: ['Sessions', 'newest first · users are pseudonymous ids', 'Filter by user id, page or error'],
        cols: [C('Session'), C('User', 'cw-120'), C('Started', 'cw-100', 'descending'), C('Pages', 'num cw-70'), C('Length', 'num cw-90'), C('Errors', 'num cw-80'), C('Device', 'cw-160'), C('Ended on', 'cw-120')],
        rows: [['ses_8f41…2c9a', 'usr_31d…', '14:31', '6', '3 m 40 s', '3', 'Safari 17 · iPhone', '/checkout', 1], ['ses_77b0…e112', 'usr_0a9…', '14:30', '4', '2 m 02 s', '1', 'Chrome 130 · Windows', '/checkout', 1], ['ses_19ca…7f03', 'usr_c42…', '14:29', '11', '9 m 15 s', '0', 'Chrome 130 · Android', '/account/orders'], ['ses_c0de…41aa', 'usr_9e1…', '14:28', '3', '1 m 10 s', '0', 'Firefox 131 · macOS', '/products/{sku}'], ['ses_5a2f…903b', 'usr_7b7…', '14:27', '7', '5 m 51 s', '0', 'Edge 130 · Windows', '/cart']].map((r: R) => [X(r[0], 'mono strong'), X(r[1], 'mono sm muted'), X(r[2], 'mono sm'), X(r[3], 'num mono'), X(r[4], 'num mono'), X(r[5], 'num mono ' + (r[8] ? 'warn-text strong' : ''), { warn: r[8] }), X(r[6], 'sm'), X(r[7], 'mono sm muted')]),
        foot: 'A session ends after 30 min without activity. Users are the id you set with setUser(); without it they stay anonymous. No screen recording.',
        empty: ['No sessions yet', 'Sessions start as soon as the web SDK sends its first page view. Set a pseudonymous user id to group visits by user.', "ferristrace.setUser({ id: 'usr_31d…' })"]
      },
      resources: {
        title: 'Resources', subtitle: 'Scripts, styles, images and fonts loaded by your pages', group: ['File', 'Type'],
        kpis: [['Transferred per view', '1.8 MB', 'median · 42 requests', '', 'muted'], ['JavaScript', '612 KB', 'compressed · 1.9 MB parsed', 'warn-text', 'warn-text'], ['From cache', '68 %', 'of requests on repeat views', '', 'muted'], ['Render-blocking', '3', 'scripts and styles in <head>', 'warn-text', 'warn-text']],
        chart: ['Page weight, median', 'solid: total · dashed: JavaScript', '3 MB', '1.5 MB', '0', 'Total page weight steady near 1.8 MB; JavaScript near 0.6 MB'],
        a: (_i: number) => 1.8 + (rnd() - 0.5) * 0.1, lo: 0, hi: 3, b: (_i: number) => 0.61 + (rnd() - 0.5) * 0.04,
        table: ['Largest files', 'by bytes transferred per view', 'Filter files'],
        cols: [C('File'), C('Type', 'cw-90'), C('Size', 'num cw-90', 'descending'), C('Load p75', 'num cw-100'), C('Cached', 'cw-160'), C('Blocking', 'cw-90')],
        rows: [['/assets/vendor-3f9a.js', 'script', '402 KB', '640 ms', 71, 'Yes', 1], ['/img/hero-1920.jpg', 'image', '388 KB', '510 ms', 64, 'No'], ['/assets/app-91c2.js', 'script', '186 KB', '310 ms', 58, 'Yes', 1], ['/fonts/SourceSans3.woff2', 'font', '96 KB', '120 ms', 92, 'No'], ['/assets/app-91c2.css', 'style', '48 KB', '90 ms', 88, 'Yes'], ['js.stripe.com/v3', 'script', '44 KB', '220 ms', 40, 'No']].map((r: R) => [X(r[0], 'mono strong'), X(r[1], 'sm muted'), X(r[2], 'num mono ' + (r[6] ? 'warn-text strong' : ''), { warn: r[6] }), X(r[3], 'num mono'), X(r[4] + ' %', 'mono small', { bar: r[4], barColor: r[4] < 50 ? 'bar-w' : 'bar-p' }), X(r[5], 'sm ' + (r[5] === 'Yes' ? 'warn-text' : 'muted'))]),
        foot: 'From the browser Resource Timing API. Third-party files show their size only if the server sends Timing-Allow-Origin.',
        empty: ['No resource timings yet', 'Turn on the document-load instrumentation of the web SDK to collect Resource Timing entries.', 'new DocumentLoadInstrumentation()']
      },
      devices: {
        title: 'Browsers & devices', subtitle: 'Who your users are, and where the problems are', group: ['Browser', 'Country'],
        kpis: [['Browsers', '14', 'Chrome 58 % · Safari 29 %', '', 'muted'], ['Mobile', '61 %', 'of sessions', '', 'muted'], ['Worst LCP', 'Safari 17 iOS', '3.8 s p75', 'warn-text', 'warn-text'], ['Countries', '23', 'France 64 %', '', 'muted']],
        chart: ['Sessions by device', 'solid: mobile · dashed: desktop', '800', '400', '0', 'Mobile sessions around 550, desktop around 350'],
        a: (i: number) => 550 + Math.sin(i / 8) * 60 + (rnd() - 0.5) * 30, lo: 0, hi: 800, b: (i: number) => 350 + Math.cos(i / 9) * 40 + (rnd() - 0.5) * 20,
        table: ['Browsers', 'by share of sessions', 'Filter browsers or OS'],
        cols: [C('Browser'), C('OS', 'cw-120'), C('Share', 'cw-160', 'descending'), C('LCP p75', 'num cw-100'), C('Errors / 1k views', 'num cw-140'), C('Device', 'cw-100')],
        rows: [['Chrome 130', 'Android', 31, '2.7 s', '4.2', 'Mobile'], ['Safari 17', 'iOS', 26, '3.8 s', '11.8', 'Mobile', 1], ['Chrome 130', 'Windows', 21, '2.1 s', '2.0', 'Desktop'], ['Safari 17', 'macOS', 9, '2.3 s', '3.1', 'Desktop'], ['Firefox 131', 'Windows', 6, '2.4 s', '6.4', 'Desktop'], ['Samsung Internet 26', 'Android', 4, '3.1 s', '5.0', 'Mobile']].map((r: R) => [X(r[0], 'strong'), X(r[1], 'sm muted'), X(r[2] + ' %', 'mono small', { bar: r[2] * 3 }), X(r[3], 'num mono ' + (r[6] ? 'warn-text strong' : ''), { warn: r[6] }), X(r[4], 'num mono ' + (r[6] ? 'warn-text' : '')), X(r[5], 'sm')]),
        foot: 'Parsed from the user agent and screen size on the first page view. Country comes from the IP, which is then dropped, never stored.',
        empty: ['No devices yet', 'This page fills in from the same page views as Pages: nothing else to install.', 'Waiting for the first page view…']
      }
    }
