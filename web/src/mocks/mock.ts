import type { Sev } from '@/lib/otel'
import { makeSeries } from './series'

export const PROJECTS = [
  { name: 'storefront', team: 'Backend', keys: 2, created: '2026-03-02' },
  { name: 'payments-eu', team: 'Payments', keys: 3, created: '2026-03-02' },
  { name: 'data-pipeline', team: 'Data', keys: 1, created: '2026-07-19' },
]

export type Service = { name: string; project: string; rps: string; err: number | null; p50: string; p95: string; p99: string; seen: string; spark: number[] }
export const SERVICES: Service[] = [
  ['checkout-api', 'storefront', '41.7/s', 4.1, '96 ms', '1.28 s', '2.9 s', '2 s ago', 0.55, 0.3],
  ['payments', 'payments-eu', '22.8/s', 0.9, '140 ms', '640 ms', '1.4 s', '1 s ago', 0.45, 0.3],
  ['api-gateway', 'storefront', '182.4/s', 0.4, '18 ms', '212 ms', '1.1 s', 'now', 0.7, 0.2],
  ['inventory', 'storefront', '63.0/s', 0.1, '6.2 ms', '41 ms', '120 ms', 'now', 0.6, 0.25],
  ['auth', 'storefront', '30.2/s', 0, '3.1 ms', '12 ms', '28 ms', '1 s ago', 0.5, 0.2],
  ['notifier', 'data-pipeline', '4.6/s', 0, '210 ms', '880 ms', '1.6 s', '4 s ago', 0.3, 0.4],
  ['unknown_service:node', 'storefront', '1.2/s', 0, '22 ms', '95 ms', '140 ms', '9 s ago', 0.2, 0.3],
  ['webhook-sender', 'data-pipeline', '—', null, '—', '—', '—', '23 min ago', 0, 0],
].map(([name, project, rps, err, p50, p95, p99, seen, base, amp], i) => ({
  name: name as string, project: project as string, rps: rps as string, err: err as number | null,
  p50: p50 as string, p95: p95 as string, p99: p99 as string, seen: seen as string,
  spark: amp ? makeSeries(24, 7 + i, (_, r) => (base as number) + (r() - 0.5) * (amp as number)) : [],
}))

export type TraceRow = { id: string; root: string; svc: string; start: string; dur: string; spans: number; errors: number }
export const TRACES: TraceRow[] = [
  ['4bf92f3577b34da6a3ce929d0e0e4736', 'POST /api/checkout', 'api-gateway', '14:32:07.418', '1.284 s', 12, 3],
  ['9c1e44a0d2b7413f8e6a0b5c71d2e9f3', 'GET /api/products', 'api-gateway', '14:32:06.902', '42.7 ms', 5, 0],
  ['0af7651916cd43dd8448eb211c80319c', 'inventory.sync', 'inventory', '14:32:05.117', '2.310 s', 38, 0],
  ['e3b0c44298fc1c149afbf4c8996fb924', 'GET /api/cart', 'api-gateway', '14:32:04.660', '18.4 ms', 4, 0],
  ['7d865e959b2466918c9863afca942d0f', 'POST /api/login', 'api-gateway', '14:32:03.251', '312 ms', 7, 1],
  ['1f3870be274f6c49b3e31a0c6728957f', 'notifier.send_email', 'notifier', '14:32:02.998', '881 ms', 3, 0],
  ['c4ca4238a0b923820dcc509a6f75849b', 'GET /api/orders/{id}', 'api-gateway', '14:32:01.774', '61.0 ms', 6, 0],
  ['a87ff679a2f3e71d9181a67b7542122c', 'payments.webhook', 'payments', '14:32:01.002', '128 ms', 9, 0],
  ['e4da3b7fbbce2345d7772b0674a318d5', 'POST /api/checkout', 'api-gateway', '14:31:59.415', '1.102 s', 12, 0],
  ['1679091c5a880faf6fb5e6087eb1b2dc', 'GET /healthz', 'api-gateway', '14:31:58.000', '812 µs', 1, 0],
  ['8f14e45fceea167a5a36dedd4bea2543', 'checkout.expire_carts', 'checkout', '14:31:57.300', '4.870 s', 214, 0],
  ['c9f0f895fb98ab9159f51fd0297e236d', 'GET /api/products', 'api-gateway', '14:31:56.812', '39.9 ms', 5, 0],
  ['45c48cce2e2d7fbdea1afc51c7c6ad26', 'POST /api/checkout', 'api-gateway', '14:31:55.090', '2.004 s', 14, 2],
  ['d3d9446802a44259755d38e6d163e820', 'GET /api/cart', 'api-gateway', '14:31:54.471', '21.2 ms', 4, 0],
  ['6512bd43d9caa6e02c990b0a82652dca', 'auth.rotate_keys', 'auth', '14:31:52.000', '96.3 ms', 3, 0],
  ['c20ad4d76fe97759aa27a0c99bff6710', 'GET /api/products/{sku}', 'api-gateway', '14:31:51.663', '27.5 ms', 5, 0],
  ['c51ce410c124a10e0db5e4b97fc2af39', 'POST /api/payments/refund', 'api-gateway', '14:31:50.208', '640 ms', 10, 1],
  ['aab3238922bcc25a6f606eb525ffdc56', 'GET /api/orders', 'api-gateway', '14:31:49.930', '88.1 ms', 6, 0],
  ['9bf31c7ff062936a96d3c8bd1f8f2ff3', 'inventory.reserve', 'inventory', '14:31:48.512', '154 ms', 4, 0],
  ['c74d97b01eae257e44aa9d5bade97baf', 'GET /api/cart', 'api-gateway', '14:31:47.006', '19.8 ms', 4, 0],
].map(([id, root, svc, start, dur, spans, errors]) => ({ id: id as string, root: root as string, svc: svc as string, start: start as string, dur: dur as string, spans: spans as number, errors: errors as number }))

export type Span = { id: string; name: string; svc: string; depth: number; offset: number; width: number; dur: string; err: boolean; kind: string; attrs: [string, string][] }
const CHECKOUT_SPANS: [string, string, number, number, number, string, boolean, string][] = [
  ['POST /api/checkout', 'api-gateway', 0, 0, 100, '1.284 s', true, 'SERVER'],
  ['auth.verify_token', 'auth', 1, 0, 1.4, '18.0 ms', false, 'INTERNAL'],
  ['GET session:*', 'auth', 2, 0.1, 0.3, '3.1 ms', false, 'CLIENT'],
  ['checkout.create_order', 'checkout', 1, 2, 97, '1.245 s', false, 'INTERNAL'],
  ['inventory.reserve', 'inventory', 2, 2.2, 14, '180 ms', false, 'CLIENT'],
  ['SELECT stock', 'inventory', 3, 3, 6.7, '86.4 ms', false, 'CLIENT'],
  ['UPDATE stock', 'inventory', 3, 9.9, 6.5, '83.1 ms', false, 'CLIENT'],
  ['payments.charge', 'payments', 2, 17, 76, '975 ms', true, 'CLIENT'],
  ['SET idempotency:*', 'payments', 3, 17.2, 0.3, '2.9 ms', false, 'CLIENT'],
  ['POST psp.internal/charge', 'payments', 3, 18, 74.8, '960 ms', true, 'CLIENT'],
  ['notifier.enqueue', 'notifier', 2, 93.2, 3.5, '45.2 ms', false, 'PRODUCER'],
  ['INSERT order_events', 'checkout', 2, 97, 2, '26.0 ms', false, 'CLIENT'],
]
export function traceSpans(t: TraceRow): Span[] {
  if (t.root === 'POST /api/checkout') {
    return CHECKOUT_SPANS.map(([name, svc, depth, offset, width, dur, err, kind], i) => ({
      id: `${t.id.slice(0, 4)}${(i + 10).toString(16)}e1f0a9c${i}`, name, svc, depth, offset, width, dur,
      err: err && t.errors > 0, kind,
      attrs: [
        ['service.name', svc], ['span.kind', kind], ['duration', dur],
        ...(name.startsWith('POST') || name.startsWith('GET') ? [['http.request.method', name.split(' ')[0]], ['http.route', name.split(' ')[1]], ['http.response.status_code', err && t.errors ? '502' : '200']] as [string, string][] : []),
        ...(name.startsWith('SELECT') || name.startsWith('UPDATE') || name.startsWith('INSERT') ? [['db.system', 'postgresql'], ['db.query.text', `${name} …`]] as [string, string][] : []),
        ...(err && t.errors ? [['exception.type', 'PspTimeoutError'], ['exception.message', 'deadline exceeded after 960ms']] as [string, string][] : []),
        ['deployment.environment', 'production'], ['service.version', '2.14.1'],
      ],
    }))
  }
  const n = Math.min(t.spans, 6)
  return Array.from({ length: n }, (_, i) => ({
    id: `${t.id.slice(0, 6)}${i}a4c2`, name: i === 0 ? t.root : ['db.query', 'cache.get', 'render', 'serialize', 'http.client'][i % 5], svc: t.svc,
    depth: i === 0 ? 0 : 1, offset: i === 0 ? 0 : i * 14, width: i === 0 ? 100 : 12, dur: i === 0 ? t.dur : '—', err: i === 1 && t.errors > 0,
    kind: i === 0 ? 'SERVER' : 'INTERNAL', attrs: [['service.name', t.svc], ['deployment.environment', 'production']],
  }))
}

export type LogRow = { time: string; sev: Sev; svc: string; body: string; trace: string; attrs: [string, string][] }
export const LOGS: LogRow[] = ([
  ['14:32:08.702', 'error', 'api-gateway', 'upstream checkout returned 502 for POST /api/checkout after 1284ms', '4bf92f3577b34da6a3ce929d0e0e4736'],
  ['14:32:08.655', 'info', 'notifier', 'enqueued order.failed order_id=ord_01JB7X3K9M', '4bf92f3577b34da6a3ce929d0e0e4736'],
  ['14:32:08.606', 'error', 'payments', 'charge failed: psp deadline exceeded after 960ms', '4bf92f3577b34da6a3ce929d0e0e4736'],
  ['14:32:08.512', 'warn', 'payments', 'psp latency 900ms over budget, cancelling at 960ms', '4bf92f3577b34da6a3ce929d0e0e4736'],
  ['14:32:07.851', 'warn', 'payments', 'retrying psp charge attempt=2 reason=timeout', '4bf92f3577b34da6a3ce929d0e0e4736'],
  ['14:32:07.640', 'debug', 'payments', 'charge request amount_cents=4999 currency=EUR', '4bf92f3577b34da6a3ce929d0e0e4736'],
  ['14:32:07.421', 'info', 'api-gateway', 'POST /api/checkout from 203.0.113.24', '4bf92f3577b34da6a3ce929d0e0e4736'],
  ['14:32:06.903', 'info', 'api-gateway', 'GET /api/products 200 in 42ms', '9c1e44a0d2b7413f8e6a0b5c71d2e9f3'],
  ['14:32:06.120', 'trace', 'inventory', 'sync batch 14/38 rows=500', '0af7651916cd43dd8448eb211c80319c'],
  ['14:32:05.118', 'info', 'inventory', 'inventory sync started source=erp', '0af7651916cd43dd8448eb211c80319c'],
  ['14:32:04.001', 'fatal', 'notifier', 'smtp pool exhausted, worker exiting with code 1', ''],
  ['14:32:03.560', 'warn', 'auth', 'login failed: invalid credentials', '7d865e959b2466918c9863afca942d0f'],
  ['14:32:02.999', 'debug', 'notifier', 'rendering template order_confirmation locale=fr', '1f3870be274f6c49b3e31a0c6728957f'],
  ['14:32:01.775', 'info', 'api-gateway', 'GET /api/orders/{id} 200 in 61ms', 'c4ca4238a0b923820dcc509a6f75849b'],
  ['14:32:00.000', 'info', 'auth', 'jwks refreshed keys=3', ''],
  ['14:31:59.416', 'info', 'api-gateway', 'POST /api/checkout 200 in 1102ms', 'e4da3b7fbbce2345d7772b0674a318d5'],
  ['14:31:58.640', 'error', 'payments', 'authorize failed: 502 Bad Gateway', '45c48cce2e2d7fbdea1afc51c7c6ad26'],
  ['14:31:57.112', 'info', 'checkout', 'expired 3,210 carts older than 7d', '8f14e45fceea167a5a36dedd4bea2543'],
  ['14:31:55.090', 'error', 'checkout-api', 'PspTimeoutError: upstream psp-gateway timed out after 3000 ms', '45c48cce2e2d7fbdea1afc51c7c6ad26'],
  ['14:31:49.374', 'error', 'inventory', 'reservation conflict on sku 8841-B', '9bf31c7ff062936a96d3c8bd1f8f2ff3'],
] as [string, Sev, string, string, string][]).map(([time, sev, svc, body, trace]) => ({
  time, sev, svc, body, trace,
  attrs: [['service.name', svc], ['severity_text', sev.toUpperCase()], ['deployment.environment', 'production'], ['host.name', svc === 'payments' ? 'app-4' : 'app-3'], ...(trace ? [['trace_id', trace]] as [string, string][] : [])],
}))

export const METRICS = [
  { name: 'http.server.active_requests', kind: 'Sum', unit: '{request}', desc: 'Number of active HTTP server requests.' },
  { name: 'http.server.request.duration', kind: 'Histogram', unit: 's', desc: 'Duration of HTTP server requests.' },
  { name: 'http.server.request.body.size', kind: 'Histogram', unit: 'By', desc: 'Size of HTTP request bodies.' },
  { name: 'process.cpu.utilization', kind: 'Gauge', unit: '1', desc: 'CPU used by the process.' },
  { name: 'db.client.connection.count', kind: 'Sum', unit: '{connection}', desc: 'Open database connections.' },
  { name: 'messaging.process.duration', kind: 'Histogram', unit: 's', desc: 'Duration of message processing.' },
]
export const METRIC_SERIES = [
  { label: '{http.request.method="POST", http.route="/api/checkout"}', f: (i: number) => (i < 44 ? 6 + Math.sin(i / 5) * 1.5 : 6 + (i - 44) * 1.0) },
  { label: '{http.request.method="GET", http.route="/api/products"}', f: (i: number) => 10 + Math.sin(i / 7) * 2.5 },
  { label: '{http.request.method="GET", http.route="/api/cart"}', f: (i: number) => 4 + Math.cos(i / 6) * 1.2 },
  { label: '{http.request.method="GET", http.route="/api/orders/{id}"}', f: (i: number) => 2 + Math.sin(i / 4) * 0.8 },
]

export type Notif = { id: string; kind: 'warn' | 'info' | 'muted'; title: string; meta: string; time: string; unread: boolean; to: string }
export const NOTIFICATIONS: Notif[] = [
  { id: 'n1', kind: 'warn', title: 'checkout-api error rate above 2 %', meta: '4.1 % for 27 min · most errors from calls to payments', time: '14:32 · alert', unread: true, to: '/services/checkout-api' },
  { id: 'n2', kind: 'info', title: 'Incident posted on your status page', meta: '"Some card payments fail at checkout" · Karim · Identified', time: '14:22', unread: true, to: '/status-page' },
  { id: 'n3', kind: 'warn', title: 'webhook-sender stopped sending spans', meta: 'data-pipeline · no data for 23 min', time: '14:09', unread: true, to: '/services' },
  { id: 'n4', kind: 'muted', title: 'Ingest key expires in 7 days', meta: 'storefront · ft_ik_…a91f · issue a new one before Oct 31', time: '09:00', unread: false, to: '/settings/project/api-keys' },
  { id: 'n5', kind: 'muted', title: 'Usage at 50 % of included', meta: 'Cloud · projected 92 % by the end of the month', time: 'Yesterday', unread: false, to: '/settings/usage' },
  { id: 'n6', kind: 'muted', title: 'nadia@acme.dev accepted the invitation', meta: 'member · storefront access', time: 'Oct 22', unread: false, to: '/settings/members' },
]

export const MEMBERS = [
  { email: 'ines@acme.dev', name: 'Inès Martin', role: 'admin', joined: '2026-03-02', you: true },
  { email: 'marc@acme.dev', name: 'Marc Dubois', role: 'member', joined: '2026-04-11' },
  { email: 'sofia@acme.dev', name: 'Sofia Rossi', role: 'member', joined: '2026-05-29' },
  { email: 'li.wei@acme.dev', name: 'Li Wei', role: 'member', joined: '2026-08-14' },
  { email: 'karim@acme.dev', name: 'Karim Benali', role: 'member', joined: '2026-09-01' },
]
export const INGEST_KEYS = [
  { name: 'k8s-prod collector', prefix: 'ft_ik_7Hq2', created: '2026-03-02', used: 'today 14:32:08', revoked: false },
  { name: 'browser-rum', prefix: 'ft_ik_Qm4x', created: '2026-05-11', used: 'today 12:28:40', revoked: false },
  { name: 'old-staging', prefix: 'ft_ik_P0aa', created: '2026-03-02', used: 'never', revoked: true },
]
export const QUERY_KEYS = [
  { name: 'grafana-export', prefix: 'ft_at_Zp81', by: 'ines@acme.dev', created: '2026-04-02', used: 'today 14:00:12' },
  { name: 'ci-smoke-tests', prefix: 'ft_at_Lr3c', by: 'marc@acme.dev', created: '2026-08-20', used: 'yesterday 22:10:03' },
]
export const ALERT_RULES = [
  { name: 'New exception type', cond: 'An exception.type never seen in this project before', notify: 'Backend · Slack #alerts-backend', last: '14:05 today', on: true, fired: true },
  { name: 'Error rate above 2 %', cond: 'Any service, over 5 min, at least 200 spans', notify: 'Backend · Slack, email', last: '14:05 today', on: true, fired: true },
  { name: 'Exception spike', cond: '10× the usual count of one type in 10 min', notify: 'Backend · email', last: 'Oct 17', on: true, fired: false },
  { name: 'Service stops sending', cond: 'No spans for 10 min from a service seen in the last day', notify: 'Data · Slack #data-oncall', last: 'never', on: false, fired: false },
]

export const STATUS_GROUPS = [
  { name: 'Storefront', items: [{ name: 'Website', desc: 'acme.dev and the product pages', bad: { 17: 'deg' } }, { name: 'Checkout', desc: 'Cart, payment form and order confirmation', bad: { 0: 'deg', 33: 'out' }, live: true }] },
  { name: 'Payments', items: [{ name: 'Card payments', desc: 'Authorisation and capture in the EU', bad: { 33: 'deg' } }, { name: 'Payouts', desc: 'Daily transfers to merchants', bad: {} }] },
  { name: 'Developers', items: [{ name: 'Public API', desc: 'api.acme.dev', bad: { 52: 'deg' } }, { name: 'Webhooks', desc: 'Event delivery to your endpoints', bad: {}, fresh: 70 }] },
] as { name: string; items: { name: string; desc: string; bad: Record<number, 'deg' | 'out'>; live?: boolean; fresh?: number }[] }[]
export const INCIDENT_UPDATES = [
  { status: 'Identified', time: '14:22', by: 'Inès', text: 'A payment provider is timing out for about 4 % of card payments. We are retrying failed payments automatically; no card is charged twice.' },
  { status: 'Investigating', time: '14:05', by: 'Karim', text: 'Some customers see an error when paying at checkout. Browsing and carts are not affected.' },
]
