import { AlertTriangle, Check, CircleHelp, Copy, KeyRound, Waypoints } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { because } from '@/auth/client'
import { useOrganisations } from '@/auth/session'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Field, Input } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Alert } from '@/components/ui/misc'
import { PageHeader } from '@/components/ft/bits'
import { keyNameProblem, useIssueKey, type IssuedKey } from '@/data/keys'
import { SNIPPETS, otlpBase, otlpEndpoint, useFirstSignals, type Arrival, type Signal } from '@/data/onboarding'
import { useCatalogue } from '@/data/projects'
import { Link } from '@/lib/router'
import { useScope } from '@/lib/scope'
import { cn } from '@/lib/utils'

const copy = (text: string, what: string) => {
  void navigator.clipboard?.writeText(text)
  toast.success(`${what} copied`)
}

function Copyable({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-control font-semibold">{label}</span>
      <div className="flex items-center gap-2">
        <code className="min-w-0 flex-1 truncate rounded-md border bg-muted px-3 py-2 font-mono text-[12.5px]">{value}</code>
        <Button size="icon" aria-label={`Copy ${label}`} onClick={() => copy(value, label)} className="max-md:size-11">
          <Copy />
        </Button>
      </div>
    </div>
  )
}

function Step({ number, title, done, children }: { number: number; title: string; done: boolean; children: React.ReactNode }) {
  return (
    <li className="grid grid-cols-[28px_minmax(0,1fr)] gap-x-3 gap-y-3 border-b px-4 py-5 last:border-b-0 md:px-5">
      <span aria-hidden className={cn('flex size-7 items-center justify-center rounded-full border font-mono text-caption font-semibold', done ? 'border-success-border bg-success-subtle text-success-text' : 'bg-background text-muted-foreground')}>
        {done ? <Check className="size-3.5" /> : number}
      </span>
      <h2 className="self-center text-heading">{title}{done ? <span className="sr-only"> (done)</span> : null}</h2>
      <div className="col-start-2 flex min-w-0 flex-col gap-3">{children}</div>
    </li>
  )
}

function KeyStep({ issued, onIssued }: { issued: IssuedKey | null; onIssued: (key: IssuedKey) => void }) {
  const { active } = useOrganisations()
  const { projects } = useCatalogue()
  const { project: chosen } = useScope()
  const issue = useIssueKey()
  const [name, setName] = useState('')
  const project = (projects ?? []).find((p) => p.id === chosen) ?? projects?.[0]
  const mayIssue = active?.role === 'owner' || active?.role === 'admin'
  const problem = name === '' ? null : keyNameProblem(name)

  if (issued) {
    return (
      <>
        <Alert variant="warning" icon={<AlertTriangle />}>Copy this key now. It is shown once; after this page only its prefix is kept.</Alert>
        <div className="flex gap-2">
          <Input readOnly mono value={issued.secret} className="bg-muted text-[12px]" aria-label="Your ingest key" />
          <Button onClick={() => copy(issued.secret, 'Key')} className="max-md:h-11"><Copy />Copy</Button>
        </div>
        <span className="text-caption text-muted-foreground">It is already filled into the snippet below.</span>
      </>
    )
  }
  if (!project) return <span className="text-control text-muted-foreground">This organisation holds no project yet. <Link to="/settings/projects">Make one</Link>, then come back.</span>
  if (!mayIssue) {
    return <span className="text-control text-muted-foreground">Only an owner or an administrator can create a key. Ask one for an ingest key for <span className="font-semibold text-foreground">{project.name}</span>, then paste it in place of <span className="font-mono">&lt;key&gt;</span> below.</span>
  }
  return (
    <form
      className="flex flex-wrap items-end gap-2"
      onSubmit={(e) => {
        e.preventDefault()
        if (keyNameProblem(name) !== null || issue.isPending) return
        issue.mutate({ name: name.trim(), scopes: ['ingest'], project: project.id, ttl: 'never' }, { onSuccess: onIssued, onError: (error) => toast.error(because(error, 'The key was not created')) })
      }}
    >
      <Field label="Key name" htmlFor="ob-key" hint={<>Where the key will live, so you know what breaks when you revoke it. It sends to <span className="font-semibold text-foreground">{project.name}</span>.</>} className="min-w-[240px] flex-1">
        <Input id="ob-key" value={name} placeholder="checkout-api, production" aria-invalid={problem ? true : undefined} onChange={(e) => setName(e.target.value)} className="max-md:h-11" />
      </Field>
      <Button type="submit" variant="primary" loading={issue.isPending} disabled={keyNameProblem(name) !== null} className="mb-[22px] max-md:mb-0 max-md:h-11 max-md:w-full"><KeyRound />Create ingest key</Button>
      {problem ? <span role="alert" className="basis-full text-caption text-destructive-text">{problem}</span> : null}
    </form>
  )
}

const ARRIVAL: Record<Arrival, { label: string; variant: 'neutral' | 'success' | 'outline' }> = {
  unknown: { label: 'Could not check', variant: 'outline' },
  waiting: { label: 'Waiting', variant: 'neutral' },
  arrived: { label: 'Arrived', variant: 'success' },
}

const SIGNALS: { kind: Signal; label: string }[] = [
  { kind: 'traces', label: 'Traces' },
  { kind: 'logs', label: 'Logs' },
  { kind: 'metrics', label: 'Metrics' },
]

export function Onboarding() {
  const signals = useFirstSignals()
  const [issued, setIssued] = useState<IssuedKey | null>(null)
  const [language, setLanguage] = useState(SNIPPETS[0].language)
  const snippet = SNIPPETS.find((s) => s.language === language) ?? SNIPPETS[0]
  const code = snippet.code
    .replaceAll('<endpoint>', otlpEndpoint())
    .replaceAll('<base>', otlpBase())
    .replaceAll('<key>', issued?.secret ?? '<key>')
  const arrived = SIGNALS.some(({ kind }) => signals[kind] === 'arrived')
  const waiting = SIGNALS.some(({ kind }) => signals[kind] === 'waiting')

  return (
    <>
      <PageHeader title="Send your first signal" meta="Create a key, point your service at it, and watch it arrive." />
      <Card className="max-w-[880px]">
        <ol className="m-0 list-none p-0">
          <Step number={1} title="Create an ingest key" done={issued !== null}>
            <KeyStep issued={issued} onIssued={setIssued} />
          </Step>
          <Step number={2} title="Point your service at FerrisTrace" done={arrived}>
            <Copyable label="OTLP endpoint, HTTP" value={otlpBase()} />
            <div className="flex flex-col overflow-hidden rounded-md border">
              <div className="flex items-center gap-2 border-b bg-muted/50 px-3 py-2">
                <div className="scrollbar-none min-w-0 flex-1 overflow-x-auto">
                  <Segmented label="Language" value={language} onChange={setLanguage} options={SNIPPETS.map((s) => ({ value: s.language, label: s.language }))} />
                </div>
                <Button size="sm" aria-label={`Copy the ${snippet.language} snippet`} className="shrink-0 max-md:size-11 max-md:p-0" onClick={() => copy(code, `${snippet.language} snippet`)}><Copy /><span className="max-md:hidden">Copy</span></Button>
              </div>
              <pre className="m-0 overflow-x-auto px-3 py-3 font-mono text-[12.5px] leading-5">{code}</pre>
            </div>
            <span className="text-caption text-muted-foreground">Logs and metrics take the same endpoint and key. Restart the service once the variables are set.</span>
          </Step>
          <Step number={3} title="Watch it arrive" done={arrived}>
            <ul role="status" aria-label="First signals" className="m-0 flex list-none flex-col rounded-md border p-0">
              {SIGNALS.map(({ kind, label }) => {
                const state = signals[kind]
                return (
                  <li key={kind} className="flex min-h-11 items-center gap-2.5 border-b border-border-subtle px-3 last:border-b-0">
                    {state === 'arrived' ? <Check className="size-4 text-success-text" aria-hidden /> : state === 'waiting' ? <span aria-hidden className="mx-1 size-2 animate-pulse-soft rounded-full bg-faint-foreground" /> : <CircleHelp className="size-4 text-muted-foreground" aria-hidden />}
                    <span className="flex-1 text-control font-semibold">{label}</span>
                    <Badge variant={ARRIVAL[state].variant}>{ARRIVAL[state].label}</Badge>
                  </li>
                )
              })}
            </ul>
            <span className="text-caption text-muted-foreground">
              {waiting ? 'Checking every 5 seconds over the last 24 hours. Nothing to do here but wait.' : arrived ? 'FerrisTrace is receiving your telemetry.' : 'The server did not answer. Check that it is running, then reload this page.'}
            </span>
            <div className="flex flex-wrap gap-2">
              {signals.trace ? (
                <Button asChild variant="primary" className="max-md:h-11"><Link to={`/traces/${signals.trace}`}><Waypoints />Open your first trace</Link></Button>
              ) : arrived ? (
                <Button asChild variant="primary" className="max-md:h-11"><Link to="/">Open the overview</Link></Button>
              ) : null}
              <Button asChild variant="ghost" className="max-md:h-11"><Link to="/">{arrived ? 'Back to the overview' : 'Skip for now'}</Link></Button>
            </div>
          </Step>
        </ol>
      </Card>
    </>
  )
}
