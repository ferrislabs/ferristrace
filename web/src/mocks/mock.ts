
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
