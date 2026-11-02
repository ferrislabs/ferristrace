import { BellOff, CheckCheck, CircleCheck, Plus, Siren, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { Link } from '@/lib/router'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/dialog'
import { Field, Input, NativeSelect } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Switch } from '@/components/ui/switch'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { EmptyState, PageHeader } from '@/components/ft/bits'
import { LineChart } from '@/components/ft/charts'
import { useAlertHistory, useAlertRules, useFiringSeries, usePreviewSeries, type Rule } from '@/data/alerts'
import { cn } from '@/lib/utils'

const SIG_CLS: Record<Rule['signal'], string> = { Spans: 'accent', Logs: 'neutral', Metrics: 'neutral', Exceptions: 'destructive', Uptime: 'success' }

export function Alerts() {
  const history = useAlertHistory()
  const loaded = useAlertRules()
  const [tab, setTab] = useState<'firing' | 'rules' | 'history'>('firing')
  const [rules, setRules] = useState(loaded)
  const [acked, setAcked] = useState<string[]>([])
  const [snoozed, setSnoozed] = useState<string[]>([])
  const [open, setOpen] = useState(false)
  const firing = rules.filter((r) => r.firing && r.on && !snoozed.includes(r.id))
  return (
    <>
      <PageHeader title="Alerts" meta="Rules on any signal: spans, logs, metrics, exceptions, uptime checks">
        <Button variant="primary" size="sm" onClick={() => setOpen(true)}><Plus />New alert</Button>
      </PageHeader>
      <div role="tablist" aria-label="Alerts" className="flex gap-1 border-b">
        {([['firing', 'Firing', firing.length], ['rules', 'Rules', rules.length], ['history', 'History', history.length]] as const).map(([v, l, n]) => (
          <button key={v} role="tab" type="button" aria-selected={tab === v} onClick={() => setTab(v)} className={cn('-mb-px inline-flex h-9 cursor-pointer items-center gap-1.5 border-b-2 border-transparent px-2.5 text-[13.5px] font-semibold text-muted-foreground hover:text-foreground', tab === v && 'border-primary text-foreground')}>
            {l}<span className={cn('rounded-sm px-1 font-mono text-[11px] font-medium leading-4', v === 'firing' && n ? 'bg-destructive-subtle text-destructive-text' : 'bg-muted')}>{n}</span>
          </button>
        ))}
      </div>

      {tab === 'firing' && (firing.length === 0 ? (
        <EmptyState icon={<CircleCheck />} title="Nothing is firing">All {rules.filter((r) => r.on).length} active rules are within their thresholds.</EmptyState>
      ) : (
        <div className="flex flex-col gap-3">
          {firing.map((r, k) => <FiringCard key={r.id} r={r} seed={k} acked={acked.includes(r.id)} onAck={() => { setAcked((a) => [...a, r.id]); toast.success('Acknowledged', { description: 'The team sees you are on it. Escalation paused.' }) }} onSnooze={() => { setSnoozed((s) => [...s, r.id]); toast('Snoozed for 1 h', { description: r.name }) }} />)}
        </div>
      ))}

      {tab === 'rules' && (
        <Card className="overflow-x-auto">
          <Table aria-label="Alert rules">
            <thead><tr><Th>Rule</Th><Th className="w-[110px]">Signal</Th><Th className="w-[100px]">Severity</Th><Th className="w-[240px]">Notify</Th><Th numeric className="w-[100px]">Now</Th><Th numeric className="w-[70px]">On</Th></tr></thead>
            <tbody>{rules.map((r) => (
              <Tr key={r.id}>
                <Td className="whitespace-normal py-2"><div className="flex flex-col gap-0.5"><span className="inline-flex items-center gap-2 text-control font-semibold">{r.name}{r.firing && r.on ? <Badge variant="destructive" size="xs">Firing</Badge> : null}</span><span className="text-caption text-muted-foreground"><span className="font-mono">{r.condition}</span> · {r.scope}</span></div></Td>
                <Td><Badge variant={SIG_CLS[r.signal] as 'accent'}>{r.signal}</Badge></Td>
                <Td><span className={cn('inline-flex items-center gap-1 text-control font-semibold', r.severity === 'critical' ? 'text-destructive-text' : 'text-warning-text')}>{r.severity === 'critical' ? <Siren className="size-3.5" aria-hidden /> : <TriangleAlert className="size-3.5" aria-hidden />}{r.severity}</span></Td>
                <Td className="whitespace-normal text-control">{r.notify}</Td>
                <Td numeric className={cn(r.firing && r.on && 'font-semibold text-destructive-text')}>{r.current}</Td>
                <Td numeric><Switch checked={r.on} onCheckedChange={(v) => setRules((x) => x.map((y) => (y.id === r.id ? { ...y, on: v } : y)))} aria-label={`Enable ${r.name}`} /></Td>
              </Tr>
            ))}</tbody>
          </Table>
        </Card>
      )}

      {tab === 'history' && (
        <Card className="overflow-x-auto">
          <Table aria-label="Alert history"><thead><tr><Th className="w-[150px]">Started</Th><Th>Rule</Th><Th className="w-[100px]">Severity</Th><Th className="w-[100px]">State</Th><Th numeric className="w-[110px]">Duration</Th><Th className="w-[150px]">Acknowledged by</Th></tr></thead>
            <tbody>{history.map((h) => (
              <Tr key={h[0] + h[1]}><Td className="font-mono">{h[0]}</Td><Td className="font-semibold">{h[1]}</Td><Td className={h[2] === 'critical' ? 'text-destructive-text' : 'text-warning-text'}>{h[2]}</Td><Td>{h[3] === 'Firing' ? <Badge variant="destructive">Firing</Badge> : <Badge variant="success">Resolved</Badge>}</Td><Td numeric>{h[4]}</Td><Td className="text-control text-muted-foreground">{h[5]}</Td></Tr>
            ))}</tbody></Table>
        </Card>
      )}
      <NewAlertDialog open={open} onOpenChange={setOpen} onCreate={(r) => { setRules((x) => [r, ...x]); setTab('rules'); toast.success('Alert created', { description: r.name }) }} />
    </>
  )
}

function FiringCard({ r, seed, acked, onAck, onSnooze }: { r: Rule; seed: number; acked: boolean; onAck: () => void; onSnooze: () => void }) {
  const vals = useFiringSeries(r, seed)
  return (
    <Card className={cn('flex flex-wrap gap-4 p-4', r.severity === 'critical' ? 'border-destructive-border' : 'border-warning-border')}>
      <div className="flex min-w-0 flex-[1_1_360px] flex-col gap-2">
        <div className="flex flex-wrap items-center gap-2">
          {r.severity === 'critical' ? <Siren className="size-4 text-destructive-text" aria-hidden /> : <TriangleAlert className="size-4 text-warning-text" aria-hidden />}
          <h2 className="text-heading">{r.name}</h2>
          <Badge variant={r.severity === 'critical' ? 'destructive' : 'warning'}>{r.severity}</Badge>
          {acked ? <Badge variant="outline"><CheckCheck />Acknowledged by you</Badge> : null}
        </div>
        <span className="text-control text-muted-foreground"><span className="font-mono">{r.condition}</span> · {r.scope} · project {r.project}</span>
        <div className="flex flex-wrap gap-x-5 gap-y-1 text-control">
          <span>Now <span className="font-mono font-semibold text-destructive-text">{r.current}</span></span><span>Threshold <span className="font-mono">{r.threshold}</span></span><span>Since <span className="font-mono">{r.since}</span></span>
        </div>
        <span className="text-caption text-muted-foreground">Notified: {r.notify}</span>
        <div className="mt-1 flex flex-wrap gap-2">
          <Button variant="primary" size="sm" onClick={onAck} disabled={acked}><CheckCheck />Acknowledge</Button>
          <Button size="sm" onClick={onSnooze}><BellOff />Snooze 1 h</Button>
          <Button asChild variant="ghost" size="sm"><Link to={r.id === 'r3' ? '/backend/system' : '/services/checkout-api'}>Investigate</Link></Button>
          {r.id === 'r1' ? <Button asChild variant="ghost" size="sm"><Link to="/releases/2.14.1">Suspect release 2.14.1</Link></Button> : null}
        </div>
      </div>
      <div className="min-w-0 flex-[1_1_380px]">
        <LineChart title="Last hour" legend="dashed: threshold" values={vals} threshold={r.id === 'r3' ? 85 : 50} hi={100} yLabels={['', '', '']} aria={`${r.name}, above threshold`} height={90} markers={r.id === 'r1' || r.id === 'r2' ? [{ at: 32, label: '2.14.1', to: '/releases/2.14.1' }] : []} />
      </div>
    </Card>
  )
}

function NewAlertDialog({ open, onOpenChange, onCreate }: { open: boolean; onOpenChange: (o: boolean) => void; onCreate: (r: Rule) => void }) {
  const [signal, setSignal] = useState<Rule['signal']>('Spans')
  const [agg, setAgg] = useState('p95 duration')
  const [svc, setSvc] = useState('payments')
  const [op, setOp] = useState('>')
  const [thr, setThr] = useState('800')
  const [win, setWin] = useState('5')
  const [sev, setSev] = useState<'critical' | 'warning'>('warning')
  const preview = usePreviewSeries()
  const fires = preview.filter((v) => v > Number(thr || 0)).length
  const AGG: Record<string, string[]> = { Spans: ['p95 duration', 'error rate', 'count'], Logs: ['count', 'count of error'], Metrics: ['avg', 'max', 'sum'], Exceptions: ['new type', 'count'], Uptime: ['failed checks'] }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent size="lg" title="New alert" description="Fires once when the condition holds for the window, then again only after it resolves.">
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); onCreate({ id: `r${Date.now()}`, name: `${svc} ${agg} ${op} ${thr}`, signal, condition: `${agg} ${op} ${thr}${agg.includes('duration') ? ' ms' : ''} for ${win} min`, scope: `service = ${svc}`, threshold: thr, current: '—', severity: sev, notify: 'Backend · Slack #alerts-backend', on: true, firing: false, project: 'storefront' }); onOpenChange(false) }}>
          <Field label="Signal"><div><Segmented label="Signal" value={signal} onChange={(v) => { setSignal(v); setAgg(AGG[v][0]) }} options={(['Spans', 'Logs', 'Metrics', 'Exceptions', 'Uptime'] as const).map((v) => ({ value: v, label: v }))} /></div></Field>
          <div className="flex flex-wrap items-center gap-2 text-control">
            <NativeSelect aria-label="Aggregation" value={agg} onChange={(e) => setAgg(e.target.value)} className="w-[150px]">{AGG[signal].map((a) => <option key={a}>{a}</option>)}</NativeSelect>
            of<NativeSelect aria-label="Service" value={svc} onChange={(e) => setSvc(e.target.value)} className="w-[160px] font-mono"><option>payments</option><option>checkout-api</option><option>api-gateway</option></NativeSelect>
            <NativeSelect aria-label="Operator" value={op} onChange={(e) => setOp(e.target.value)} className="w-16"><option>&gt;</option><option>&lt;</option></NativeSelect>
            <Input aria-label="Threshold" mono value={thr} onChange={(e) => setThr(e.target.value)} className="w-24" /><span className="text-muted-foreground">ms for</span>
            <Input aria-label="Window" mono value={win} onChange={(e) => setWin(e.target.value)} className="w-16" /><span className="text-muted-foreground">min</span>
          </div>
          <LineChart title="Preview, last hour" legend={fires ? `would have fired ${Math.min(3, Math.ceil(fires / 15))} times` : 'would not have fired'} values={preview} threshold={Number(thr) || 0} hi={1000} yLabels={['1 s', '500 ms', '0']} aria="Preview of the alert condition" height={90} />
          <div className="flex flex-wrap gap-4">
            <Field label="Severity"><div><Segmented label="Severity" value={sev} onChange={setSev} options={[{ value: 'warning', label: 'Warning' }, { value: 'critical', label: 'Critical' }]} /></div></Field>
            <Field label="Notify" className="flex-1"><NativeSelect><option>Backend team · Slack #alerts-backend</option><option>Backend team · PagerDuty (critical)</option><option>Payments team · Slack #alerts-payments</option></NativeSelect></Field>
          </div>
          <DialogFooter><Button type="button" onClick={() => onOpenChange(false)}>Cancel</Button><Button variant="primary" type="submit">Create alert</Button></DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
