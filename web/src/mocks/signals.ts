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
