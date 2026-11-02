import { Check, ExternalLink, Eye, EyeOff, GripVertical, Plus, TriangleAlert, Upload, Wrench, Zap } from 'lucide-react'
import { useState } from 'react'
import { Link } from '@/lib/router'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input, Textarea } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Switch } from '@/components/ui/switch'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { StatusIndicator } from '@/components/ft/bits'
import { StatusPageView } from '@/components/ft/StatusPageView'
import { useStatusConfig, useStatusGroups, type Accent, type StatusConfig } from '@/data/status-page'
import { cn } from '@/lib/utils'

type Tab = 'overview' | 'components' | 'incidents' | 'appearance' | 'domain'
const ACCENTS: { v: Accent; hex: string; label: string }[] = [
  { v: 'indigo', hex: '#4f46e5', label: 'Indigo' }, { v: 'teal', hex: '#0f766e', label: 'Teal' }, { v: 'orange', hex: '#c2410c', label: 'Orange' }, { v: 'rose', hex: '#be123c', label: 'Rose' }, { v: 'slate', hex: '#334155', label: 'Slate' },
]
const SOURCES: Record<string, string> = {
  Website: 'storefront · web-frontend · error rate ≥ 2 %', Checkout: 'storefront · checkout-api · error rate ≥ 2 %', 'Card payments': 'payments-eu · psp-gateway · error rate ≥ 1 %',
  Payouts: 'Set by hand · maintenance scheduled', 'Public API': 'storefront · public-api · error rate ≥ 5 %', Webhooks: 'data-pipeline · webhook-sender · no spans for 10 min',
}

export function StatusPageEditor() {
  const loaded = useStatusConfig()
  const groups = useStatusGroups()
  const [tab, setTab] = useState<Tab>('overview')
  const [published, setPublished] = useState<StatusConfig>(loaded)
  const [draft, setDraft] = useState<StatusConfig>(loaded)
  const [postOpen, setPostOpen] = useState(false)
  const [publishOpen, setPublishOpen] = useState(false)
  const changes = diff(published, draft)
  const set = (p: Partial<StatusConfig>) => setDraft((d) => ({ ...d, ...p }))
  const tabs: { v: Tab; label: string; cnt?: string }[] = [
    { v: 'overview', label: 'Overview' }, { v: 'components', label: 'Components', cnt: '6' }, { v: 'incidents', label: 'Incidents', cnt: draft.state === 'incident' ? '1' : undefined }, { v: 'appearance', label: 'Appearance' }, { v: 'domain', label: 'Domain and access' },
  ]
  return (
    <>
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="text-title">Status page</h1>
        <Badge variant="success"><Check />Published</Badge>
        <Link to="/status/acme" target="_blank" className="inline-flex items-center gap-1 font-mono text-control">status.acme.dev<ExternalLink className="size-3" aria-hidden /></Link>
        <span className="flex-1" />
        {changes.length ? (
          <>
            <span className="inline-flex items-center gap-1.5 text-control text-warning-text"><span className="size-1.5 rounded-full bg-warning-text" />{changes.length} unpublished change{changes.length > 1 ? 's' : ''}</span>
            <Button variant="ghost" size="sm" onClick={() => setDraft(published)}>Discard</Button>
            <Button variant="primary" size="sm" onClick={() => setPublishOpen(true)}>Review and publish</Button>
          </>
        ) : <span className="text-control text-muted-foreground">Everything is published</span>}
      </div>
      <div role="tablist" aria-label="Status page" className="flex gap-1 overflow-x-auto border-b">
        {tabs.map((t) => (
          <button key={t.v} role="tab" type="button" aria-selected={tab === t.v} onClick={() => setTab(t.v)} className={cn('-mb-px inline-flex h-9 cursor-pointer items-center gap-1.5 whitespace-nowrap border-b-2 border-transparent px-2.5 text-[13.5px] font-semibold text-muted-foreground hover:text-foreground', tab === t.v && 'border-primary text-foreground')}>
            {t.label}{t.cnt ? <span className="rounded-sm bg-muted px-1 font-mono text-[11px] font-medium leading-4 text-muted-foreground">{t.cnt}</span> : null}
          </button>
        ))}
      </div>

      {tab === 'overview' && (
        <div className="flex flex-wrap items-start gap-6">
          <div className="flex min-w-0 flex-[1_1_480px] flex-col gap-4">
            {draft.state === 'incident' ? (
              <>
                <section className="flex items-start gap-3.5 rounded-lg border border-warning-border bg-card px-5 py-4">
                  <TriangleAlert className="mt-0.5 size-5 text-warning-text" aria-hidden />
                  <div className="flex min-w-0 flex-1 flex-col gap-1">
                    <h2 className="text-base font-bold">Your users see: Degraded performance on Checkout</h2>
                    <span className="text-control text-muted-foreground">Set automatically at 14:05 because <span className="font-mono">storefront · checkout-api</span> span error rate reached 4.1 % (threshold 2 %).</span>
                    <div className="mt-1.5 flex gap-2"><Button asChild size="sm"><Link to="/services/checkout-api">Open service</Link></Button></div>
                  </div>
                </section>
                <Card className="overflow-hidden">
                  <CardHeader className="flex-wrap"><Badge variant="warning">Degraded</Badge><CardTitle>Some card payments fail at checkout</CardTitle><span className="flex-1" /><span className="text-caption text-muted-foreground">Started 14:05</span><Button variant="primary" size="sm" onClick={() => setPostOpen(true)}>Post update</Button></CardHeader>
                  <ol className="m-0 list-none px-5">
                    {draft.updates.map((u, i) => (
                      <li key={i} className="grid grid-cols-[110px_minmax(0,1fr)] gap-x-4 border-b border-border-subtle py-3 last:border-b-0">
                        <div className="flex flex-col"><span className="text-control font-semibold">{u.status}</span><time className="font-mono text-caption text-muted-foreground">{u.time}</time></div>
                        <p className="m-0 text-control">{u.text}</p>
                      </li>
                    ))}
                  </ol>
                  <div className="border-t border-border-subtle bg-sidebar px-5 py-2.5 text-caption text-muted-foreground">312 subscribers were notified of each update. Internal links (traces, logs) stay private.</div>
                </Card>
              </>
            ) : (
              <section className="flex items-center gap-3 rounded-lg border border-success-border bg-card px-5 py-4"><StatusIndicator status="operational" label="Your users see: All systems operational" /></section>
            )}
            <Card className="flex items-center gap-3 px-5 py-3.5">
              <Wrench className="size-4 text-muted-foreground" aria-hidden />
              <div className="flex flex-1 flex-col"><span className="text-control font-semibold">Payout ledger upgrade</span><span className="text-caption text-muted-foreground">Scheduled · Thu Oct 22, 22:00 – 23:00 CEST · Payouts</span></div>
              <Button variant="ghost" size="sm" onClick={() => setTab('incidents')}>Edit</Button>
            </Card>
          </div>
          <aside className="flex w-[340px] max-w-full flex-col gap-4" aria-label="Page summary">
            <Card className="grid grid-cols-2 gap-4 p-4">
              {[['Uptime, 90 days', '99.94 %'], ['Subscribers', '312'], ['Views, 7 days', '18,402'], ['Incidents, 90 days', '4']].map(([l, v]) => <div key={l} className="flex flex-col"><span className="text-caption text-muted-foreground">{l}</span><span className="font-mono text-[20px] font-semibold">{v}</span></div>)}
            </Card>
            <Card className="flex flex-col gap-2.5 p-4">
              <h2 className="text-sub">Components now</h2>
              {groups.flatMap((g) => g.items).filter((i) => !draft.hidden.includes(i.name)).map((i) => (
                <div key={i.name} className="flex items-center gap-2 text-control"><span className="flex-1 truncate">{i.name}</span>{i.live && draft.state === 'incident' ? <StatusIndicator status="degraded" /> : i.name === 'Webhooks' ? <StatusIndicator status="nodata" /> : <StatusIndicator status="operational" />}</div>
              ))}
            </Card>
          </aside>
        </div>
      )}

      {tab === 'components' && (
        <div className="flex flex-wrap items-start gap-6">
          <div className="flex min-w-0 flex-[1_1_480px] flex-col gap-3">
            <div className="flex items-center gap-2"><p className="m-0 flex-1 text-control text-muted-foreground">Automatic components follow a rule on your signals; manual ones keep the status you set.</p><Button size="sm"><Plus />Add component</Button></div>
            {groups.map((g) => (
              <Card key={g.name} className="overflow-hidden">
                <div className="flex items-center gap-2 border-b bg-sidebar px-3 py-2"><GripVertical className="size-3.5 text-faint-foreground" aria-hidden /><span className="flex-1 text-control font-semibold">{g.name}</span></div>
                {g.items.map((i) => {
                  const hidden = draft.hidden.includes(i.name)
                  return (
                    <div key={i.name} className={cn('flex items-center gap-2.5 border-b border-border-subtle px-3 py-2.5 last:border-b-0', hidden && 'opacity-60')}>
                      <GripVertical className="size-3.5 text-faint-foreground" aria-hidden />
                      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                        <span className="inline-flex items-center gap-1.5 text-control font-semibold">{i.name}{hidden ? <Badge variant="outline" size="xs">Hidden</Badge> : null}</span>
                        <span className="inline-flex items-center gap-1 truncate font-mono text-[11.5px] text-muted-foreground">{i.name === 'Payouts' ? null : <Zap className="size-3" aria-hidden />}{SOURCES[i.name]}</span>
                      </div>
                      <span className="w-[110px]">{i.live && draft.state === 'incident' ? <StatusIndicator status="degraded" /> : i.name === 'Webhooks' ? <StatusIndicator status="nodata" /> : <StatusIndicator status="operational" />}</span>
                      <Button variant="ghost" size="icon-sm" aria-label={`${hidden ? 'Show' : 'Hide'} ${i.name}`} onClick={() => set({ hidden: hidden ? draft.hidden.filter((h) => h !== i.name) : [...draft.hidden, i.name] })}>{hidden ? <EyeOff /> : <Eye />}</Button>
                    </div>
                  )
                })}
              </Card>
            ))}
          </div>
          <Preview c={draft} />
        </div>
      )}

      {tab === 'incidents' && (
        <div className="flex max-w-[1100px] flex-col gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <Segmented label="Incident state shown" value={draft.state} onChange={(v) => set({ state: v })} options={[{ value: 'incident', label: 'Incident open' }, { value: 'operational', label: 'All operational' }, { value: 'maintenance', label: 'Maintenance' }]} />
            <span className="flex-1" />
            <Button size="sm"><Wrench />Schedule maintenance</Button>
            <Button variant="primary" size="sm" onClick={() => setPostOpen(true)}><Plus />{draft.state === 'incident' ? 'Post update' : 'Declare incident'}</Button>
          </div>
          <Card className="overflow-x-auto">
            <Table>
              <thead><tr><Th>Incident</Th><Th className="w-[130px]">Impact</Th><Th className="w-[200px]">Components</Th><Th className="w-[140px]">Started</Th><Th numeric className="w-[90px]">Duration</Th><Th className="w-[120px]">State</Th></tr></thead>
              <tbody>
                {[['Some card payments fail at checkout', 'Degraded', 'warning', 'Checkout', 'Oct 20, 14:05', '36 min', draft.state === 'incident' ? 'Identified' : 'Resolved'], ['Payout ledger upgrade', 'Maintenance', 'outline', 'Payouts', 'Oct 22, 22:00', '1 h', 'Scheduled'], ['Checkout unavailable for 12 minutes', 'Outage', 'destructive', 'Checkout, Card payments', 'Sep 17, 09:12', '12 min', 'Resolved'], ['Slow API responses', 'Degraded', 'warning', 'Public API', 'Aug 29, 16:40', '41 min', 'Resolved'], ['Delayed webhook delivery', 'Degraded', 'warning', 'Webhooks', 'Aug 3, 02:15', '2 h 10 min', 'Resolved']].map((r) => (
                  <Tr key={r[0]}><Td className="font-semibold">{r[0]}</Td><Td><Badge variant={r[2] as 'warning'}>{r[1]}</Badge></Td><Td className="text-control text-muted-foreground">{r[3]}</Td><Td className="font-mono text-control">{r[4]}</Td><Td numeric>{r[5]}</Td><Td className="text-control">{r[6]}</Td></Tr>
                ))}
              </tbody>
            </Table>
          </Card>
        </div>
      )}

      {tab === 'appearance' && (
        <div className="flex flex-wrap items-start gap-6">
          <form className="flex min-w-0 flex-[1_1_420px] flex-col gap-5" aria-label="Appearance" onSubmit={(e) => e.preventDefault()}>
            <Field label="Page name" htmlFor="sp-name" hint="Shown in the header and the browser tab."><Input id="sp-name" value={draft.name} onChange={(e) => set({ name: e.target.value || 'Acme' })} /></Field>
            <Field label="Logo">
              <div className="flex items-center gap-3 rounded-md border border-dashed border-input bg-sidebar p-3">
                <span className={cn(draft.accent !== 'indigo' && `accent-${draft.accent}`, 'inline-flex size-8 items-center justify-center rounded-lg bg-primary font-bold text-primary-foreground')} aria-hidden>{draft.name.slice(0, 1)}</span>
                <div className="flex flex-1 flex-col"><span className="text-control">No logo yet. We show your initial on the accent colour.</span><span className="text-caption text-muted-foreground">SVG or PNG, at least 128 px tall.</span></div>
                <Button size="sm" type="button"><Upload />Upload</Button>
              </div>
            </Field>
            <Field label="Accent colour" hint="Status colours (green, amber, red) never change.">
              <div role="radiogroup" aria-label="Accent colour" className="flex items-center gap-2.5">
                {ACCENTS.map((a) => (
                  <button key={a.v} type="button" role="radio" aria-checked={draft.accent === a.v} aria-label={a.label} onClick={() => set({ accent: a.v })} className={cn('size-7 cursor-pointer rounded-full border-2 border-background ring-1 ring-border', draft.accent === a.v && 'ring-2 ring-ring')} style={{ background: a.hex }} />
                ))}
              </div>
            </Field>
            <Field label="History shown"><div><Segmented label="History" value={String(draft.days)} onChange={(v) => set({ days: Number(v) as 30 | 60 | 90 })} options={[{ value: '30', label: '30 days' }, { value: '60', label: '60 days' }, { value: '90', label: '90 days' }]} /></div></Field>
            <fieldset className="m-0 flex flex-col gap-0.5 border-0 p-0">
              <legend className="mb-1 text-control font-semibold">Sections</legend>
              <Switch checked={draft.showUptime} onCheckedChange={(v) => set({ showUptime: v })} label="Uptime bars per component" />
              <Switch checked={draft.showDesc} onCheckedChange={(v) => set({ showDesc: v })} label="Component descriptions" />
              <Switch checked={draft.showPast} onCheckedChange={(v) => set({ showPast: v })} label="Past incidents" />
              <Switch checked={draft.showSubscribe} onCheckedChange={(v) => set({ showSubscribe: v })} label="Subscribe button" />
              <Switch checked={draft.poweredBy} onCheckedChange={(v) => set({ poweredBy: v })} label="“Powered by FerrisTrace” in the footer" />
            </fieldset>
          </form>
          <Preview c={draft} />
        </div>
      )}

      {tab === 'domain' && (
        <div className="flex max-w-[820px] flex-col gap-7">
          <section className="flex flex-col gap-2.5"><h2 className="text-heading">Address</h2>
            <Card><dl className="m-0 grid grid-cols-[200px_minmax(0,1fr)] text-control">
              <dt className="border-b border-border-subtle px-4 py-3 text-muted-foreground">Default address</dt><dd className="m-0 border-b border-border-subtle px-4 py-3 font-mono">acme-prod.status.ferristrace.example.com</dd>
              <dt className="px-4 py-3 text-muted-foreground">Self-hosted</dt><dd className="m-0 px-4 py-3">Served by the same container at <span className="font-mono">https://&lt;your-host&gt;/status/acme-prod</span>.</dd>
            </dl></Card>
          </section>
          <section className="flex flex-col gap-2.5"><div className="flex items-center gap-2.5"><h2 className="text-heading">Custom domain</h2><Badge variant="success"><Check />Verified · TLS active</Badge></div>
            <Card className="flex flex-col gap-3.5 p-4">
              <Field label="Domain" htmlFor="sp-dom"><div className="flex max-w-[460px] gap-2"><Input id="sp-dom" mono defaultValue="status.acme.dev" /><Button size="sm" className="h-8" onClick={() => toast.success('DNS record found', { description: 'CNAME status → acme-prod.status.ferristrace.example.com' })}>Check DNS</Button></div></Field>
              <span className="text-caption text-muted-foreground">CNAME <span className="font-mono">status</span> → <span className="font-mono">acme-prod.status.ferristrace.example.com</span> · found Oct 2, 09:41</span>
            </Card>
          </section>
          <section className="flex flex-col gap-2.5"><h2 className="text-heading">Who can see it</h2>
            <VisibilityPicker />
          </section>
        </div>
      )}

      <PostUpdateDialog open={postOpen} onOpenChange={setPostOpen} incident={draft.state === 'incident'} onPost={(u, resolve) => {
        const next = { ...draft, updates: [u, ...draft.updates], state: resolve ? ('operational' as const) : ('incident' as const) }
        setDraft(next); setPublished((p) => ({ ...p, updates: next.updates, state: next.state }))
        toast.success(resolve ? 'Incident resolved' : 'Update posted', { description: '312 subscribers notified' })
      }} />
      <Dialog open={publishOpen} onOpenChange={setPublishOpen}>
        <DialogContent title={`Publish ${changes.length} change${changes.length > 1 ? 's' : ''}`} description="Visitors of status.acme.dev see them within a minute.">
          <ul className="m-0 list-none rounded-lg border p-0">
            {changes.map((c) => <li key={c} className="border-b border-border-subtle px-3 py-2.5 text-control last:border-b-0">{c}</li>)}
          </ul>
          <span className="text-caption text-muted-foreground">Incidents are not part of a publish: they go live when posted.</span>
          <DialogFooter><Button onClick={() => setPublishOpen(false)}>Keep editing</Button><Button variant="primary" onClick={() => { setPublished(draft); setPublishOpen(false); toast.success('Status page published') }}>Publish</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

function diff(a: StatusConfig, b: StatusConfig) {
  const out: string[] = []
  if (a.name !== b.name) out.push(`Page name “${a.name}” → “${b.name}”`)
  if (a.accent !== b.accent) out.push(`Accent colour ${a.accent} → ${b.accent}`)
  if (a.days !== b.days) out.push(`History ${a.days} → ${b.days} days`)
  ;(['showUptime', 'showDesc', 'showPast', 'showSubscribe', 'poweredBy'] as const).forEach((k) => { if (a[k] !== b[k]) out.push(`${b[k] ? 'Show' : 'Hide'} ${{ showUptime: 'uptime bars', showDesc: 'descriptions', showPast: 'past incidents', showSubscribe: 'subscribe button', poweredBy: '“Powered by FerrisTrace”' }[k]}`) })
  b.hidden.filter((h) => !a.hidden.includes(h)).forEach((h) => out.push(`Hide component “${h}”`))
  a.hidden.filter((h) => !b.hidden.includes(h)).forEach((h) => out.push(`Show component “${h}”`))
  return out
}

function Preview({ c }: { c: StatusConfig }) {
  const [dark, setDark] = useState(false)
  return (
    <div className="flex w-[560px] max-w-full shrink-0 flex-col gap-2">
      <div className="flex items-center gap-2"><span className="flex-1 text-control font-semibold">Preview · draft</span><Segmented size="sm" label="Preview theme" value={dark ? 'dark' : 'light'} onChange={(v) => setDark(v === 'dark')} options={[{ value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }]} /></div>
      <div className="overflow-hidden rounded-lg border shadow-sm">
        <div className="flex h-8 items-center gap-2 border-b bg-sidebar px-2.5"><span className="flex gap-1">{[0, 1, 2].map((i) => <span key={i} className="size-2 rounded-full bg-border" />)}</span><span className="flex h-5 flex-1 items-center rounded bg-muted px-2 font-mono text-[11px] text-muted-foreground">status.acme.dev</span></div>
        <div className={cn('h-[640px] overflow-y-auto', dark ? 'dark' : 'light')}>
          <div className="origin-top-left scale-[0.62]" style={{ width: '161%' }}><StatusPageView c={c} compact /></div>
        </div>
      </div>
    </div>
  )
}

function VisibilityPicker() {
  const [v, setV] = useState('public')
  const opts = [['public', 'Public', 'Anyone with the address.'], ['password', 'Password', 'One shared password, for B2B customers.'], ['members', 'Members only', 'Signed-in workspace members.']]
  return (
    <div role="radiogroup" aria-label="Visibility" className="grid grid-cols-[repeat(auto-fit,minmax(200px,1fr))] gap-2">
      {opts.map(([k, t, d]) => (
        <button key={k} type="button" role="radio" aria-checked={v === k} onClick={() => setV(k)} className={cn('flex cursor-pointer items-start gap-2.5 rounded-md border bg-background p-3 text-left', v === k && 'border-ring ring-1 ring-ring')}>
          <span className={cn('relative mt-0.5 size-4 shrink-0 rounded-full border border-input', v === k && 'border-primary after:absolute after:inset-[3px] after:rounded-full after:bg-primary')} />
          <span className="flex flex-col gap-0.5"><span className="text-control font-semibold">{t}</span><span className="text-caption text-muted-foreground">{d}</span></span>
        </button>
      ))}
    </div>
  )
}

function PostUpdateDialog({ open, onOpenChange, incident, onPost }: { open: boolean; onOpenChange: (o: boolean) => void; incident: boolean; onPost: (u: { status: string; time: string; text: string }, resolve: boolean) => void }) {
  const [state, setState] = useState(incident ? 'Monitoring' : 'Investigating')
  const [text, setText] = useState('')
  const [notify, setNotify] = useState(true)
  const submit = (e: React.FormEvent) => {
    e.preventDefault()
    const now = new Date()
    onPost({ status: state, time: `${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}`, text: text || 'We are looking into it.' }, state === 'Resolved')
    setText(''); onOpenChange(false)
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent size="md" title={incident ? 'Post update' : 'Declare incident'} description={incident ? 'Some card payments fail at checkout · started 14:05' : 'Shown on status.acme.dev as soon as you post it.'}>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <Field label="State"><div><Segmented label="State" value={state} onChange={setState} options={['Investigating', 'Identified', 'Monitoring', 'Resolved'].map((s) => ({ value: s, label: s }))} /></div></Field>
          <Field label="Message" htmlFor="pu-msg" hint="Write it for your users: what they notice, not the cause. Do not paste trace ids."><Textarea id="pu-msg" value={text} onChange={(e) => setText(e.target.value)} placeholder={state === 'Resolved' ? 'The payment provider has recovered. Failed payments were retried and went through.' : 'What users notice, and what you are doing.'} /></Field>
          {state === 'Resolved' ? <div className="flex items-start gap-2 rounded-md bg-accent-subtle px-3 py-2.5 text-control text-accent-text"><Zap className="mt-0.5 size-4" aria-hidden />Checkout goes back to its automatic status. Its error rate is 0.3 % now, so it will show Operational.</div> : null}
          <Switch checked={notify} onCheckedChange={setNotify} label="Notify 312 subscribers" />
          <DialogFooter><Button type="button" onClick={() => onOpenChange(false)}>Cancel</Button><Button type="submit" variant="primary">{state === 'Resolved' ? 'Post and resolve' : 'Post'}</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
