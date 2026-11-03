
export const MEMBERS = [
  { email: 'ines@acme.dev', name: 'Inès Martin', role: 'admin', joined: '2026-03-02', you: true },
  { email: 'marc@acme.dev', name: 'Marc Dubois', role: 'member', joined: '2026-04-11' },
  { email: 'sofia@acme.dev', name: 'Sofia Rossi', role: 'member', joined: '2026-05-29' },
  { email: 'li.wei@acme.dev', name: 'Li Wei', role: 'member', joined: '2026-08-14' },
  { email: 'karim@acme.dev', name: 'Karim Benali', role: 'member', joined: '2026-09-01' },
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
