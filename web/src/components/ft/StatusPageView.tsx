import { Bell, CircleCheck, CircleX, FileText, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { StatusIndicator, type Status } from './bits'
import { Logo } from './Logo'
import { OVERALL_WORDS, type ComponentStatus, type PublicIncident, type PublicStatus } from '@/data/status-page'
import { cn } from '@/lib/utils'

const INDICATOR: Record<ComponentStatus, Status> = { operational: 'operational', degraded: 'degraded', down: 'outage' }

const BANNER: Record<ComponentStatus, { border: string; text: string; Icon: typeof CircleCheck }> = {
  operational: { border: 'border-success-border', text: 'text-success-text', Icon: CircleCheck },
  degraded: { border: 'border-warning-border', text: 'text-warning-text', Icon: TriangleAlert },
  down: { border: 'border-destructive-border', text: 'text-destructive-text', Icon: CircleX },
}

const when = (ms: number) =>
  new Date(ms).toLocaleString('en-US', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })

const capital = (word: string) => word.slice(0, 1).toUpperCase() + word.slice(1)

export function StatusPageView({ view, compact, subscribe }: { view: PublicStatus; compact?: boolean; subscribe?: (email: string) => Promise<void> }) {
  const { page } = view
  const banner = BANNER[view.overall]
  const open = view.incidents.filter((incident) => incident.status !== 'resolved')
  const past = view.incidents.filter((incident) => incident.status === 'resolved')
  return (
    <div className={cn(page.theme.accent !== 'indigo' && `accent-${page.theme.accent}`, 'min-h-full bg-sidebar text-foreground')}>
      <header className="border-b bg-background">
        <div className="mx-auto flex h-16 max-w-[880px] items-center gap-3 px-6">
          <span className="inline-flex size-8 items-center justify-center rounded-lg bg-primary text-base font-bold text-primary-foreground" aria-hidden>{page.name.slice(0, 1)}</span>
          <span className="text-[17px] font-bold">{page.name} <span className="font-normal text-muted-foreground">status</span></span>
        </div>
      </header>
      <main className="mx-auto flex max-w-[880px] flex-col gap-6 px-6 pb-16 pt-8">
        {page.intro ? <p className="m-0 whitespace-pre-wrap text-control text-muted-foreground">{page.intro}</p> : null}
        <section aria-live="polite" className={cn('flex items-center gap-4 rounded-lg border bg-card px-6 py-5', banner.border)}>
          <banner.Icon className={cn('size-7', banner.text)} aria-hidden />
          <div className="flex flex-col gap-0.5"><h1 className="text-[24px] font-bold leading-8">{OVERALL_WORDS[view.overall]}</h1><span className="text-control text-muted-foreground">Refreshes every minute</span></div>
        </section>
        {open.map((incident, index) => <IncidentCard key={`${incident.opened_ms}-${index}`} incident={incident} />)}
        <section className="overflow-hidden rounded-lg border bg-card" aria-label="Components">
          <div className="flex items-center gap-3 border-b px-5 py-3.5"><h2 className="flex-1 text-base font-bold">Components</h2></div>
          {view.components.length === 0 ? <div className="px-5 py-4 text-control text-muted-foreground">No components are shown yet.</div> : null}
          {view.components.map((component) => (
            <div key={component.name} className="flex items-center gap-2 border-b border-border-subtle px-5 py-3 last:border-b-0">
              <span className="flex-1 font-semibold">{component.name}</span>
              <StatusIndicator status={INDICATOR[component.status]} />
            </div>
          ))}
        </section>
        {past.length ? (
          <section className="flex flex-col gap-3" aria-label="Past incidents">
            <h2 className="text-[18px] font-bold">Resolved in the last seven days</h2>
            {past.map((incident, index) => <IncidentCard key={`${incident.opened_ms}-${index}`} incident={incident} />)}
          </section>
        ) : null}
        {!compact && subscribe ? <SubscribeForm subscribe={subscribe} /> : null}
      </main>
      <footer className="border-t bg-background"><div className="mx-auto flex max-w-[880px] items-center gap-2 px-6 py-5 text-caption text-muted-foreground"><span className="flex-1" /><Logo className="size-4" />Powered by FerrisTrace</div></footer>
    </div>
  )
}

function IncidentCard({ incident }: { incident: PublicIncident }) {
  const resolved = incident.status === 'resolved'
  return (
    <article className={cn('overflow-hidden rounded-lg border bg-card', !resolved && 'border-warning-border')}>
      <div className={cn('flex flex-col gap-1.5 border-b px-5 py-4', !resolved && 'border-warning-border bg-warning-subtle')}>
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant={resolved ? 'success' : 'warning'}>{capital(incident.status)}</Badge>
          <span className="text-control text-muted-foreground">Since {when(incident.opened_ms)}{incident.resolved_ms !== null ? ` · resolved ${when(incident.resolved_ms)}` : ''}</span>
          {incident.postmortem_url ? <a className="ml-auto inline-flex items-center gap-1 text-control" href={incident.postmortem_url}><FileText className="size-3.5" aria-hidden />Postmortem</a> : null}
        </div>
        <h2 className="text-[18px] font-bold leading-[26px]">{incident.title}</h2>
      </div>
      <ol className="m-0 flex list-none flex-col px-5 pb-4 pt-1">
        {incident.updates.map((update, index) => (
          <li key={`${update.at_ms}-${index}`} className="grid grid-cols-[130px_minmax(0,1fr)] gap-x-4 border-b border-border-subtle py-3 last:border-b-0">
            <div className="flex flex-col"><span className="text-control font-semibold">{update.kind === 'status' ? capital(update.text) : 'Update'}</span><time className="font-mono text-caption text-muted-foreground">{when(update.at_ms)}</time></div>
            <p className="m-0 whitespace-pre-wrap text-[15px] leading-[23px]">{update.kind === 'status' ? `The incident is ${update.text}.` : update.text}</p>
          </li>
        ))}
      </ol>
    </article>
  )
}

function SubscribeForm({ subscribe }: { subscribe: (email: string) => Promise<void> }) {
  const [email, setEmail] = useState('')
  const [state, setState] = useState<'idle' | 'sending' | 'sent' | 'failed'>('idle')
  return (
    <section className="flex flex-col gap-2 rounded-lg border bg-card px-5 py-4" aria-label="Subscribe">
      <h2 className="flex items-center gap-1.5 text-base font-bold"><Bell className="size-4" aria-hidden />Get incident updates by email</h2>
      {state === 'sent' ? (
        <p className="m-0 text-control text-muted-foreground">If this address can subscribe, a confirmation link is on its way. Open it within seven days.</p>
      ) : (
        <form className="flex flex-wrap gap-2" onSubmit={(event) => {
          event.preventDefault()
          setState('sending')
          subscribe(email.trim()).then(() => setState('sent'), () => setState('failed'))
        }}>
          <Input type="email" required aria-label="Email address" placeholder="you@example.com" value={email} onChange={(event) => setEmail(event.target.value)} className="max-w-[320px]" />
          <Button type="submit" variant="primary" size="sm" className="h-8" disabled={state === 'sending' || !email.trim()}>Subscribe</Button>
          {state === 'failed' ? <span className="w-full text-control text-destructive-text">That address was not accepted.</span> : null}
        </form>
      )}
    </section>
  )
}
