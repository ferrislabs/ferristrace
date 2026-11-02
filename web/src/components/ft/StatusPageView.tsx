import { Bell, CircleCheck, TriangleAlert, Wrench } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { StatusIndicator, UptimeBars } from './bits'
import { Logo } from './Logo'
import { useStatusGroups, type StatusConfig } from '@/data/status-page'
import { cn } from '@/lib/utils'

export function StatusPageView({ c, compact }: { c: StatusConfig; compact?: boolean }) {
  const groups = useStatusGroups()
  const incident = c.state === 'incident'
  return (
    <div className={cn(c.accent !== 'indigo' && `accent-${c.accent}`, 'min-h-full bg-sidebar text-foreground')}>
      <header className="border-b bg-background">
        <div className="mx-auto flex h-16 max-w-[880px] items-center gap-3 px-6">
          <span className="inline-flex size-8 items-center justify-center rounded-lg bg-primary text-base font-bold text-primary-foreground" aria-hidden>{c.name.slice(0, 1)}</span>
          <span className="text-[17px] font-bold">{c.name} <span className="font-normal text-muted-foreground">status</span></span>
          <span className="flex-1" />
          {!compact ? <a className="text-control text-muted-foreground" href="#">acme.dev</a> : null}
          {c.showSubscribe ? <button type="button" className="inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-md bg-primary px-2.5 text-[12.5px] font-semibold text-primary-foreground hover:bg-primary-hover"><Bell className="size-3.5" aria-hidden />Subscribe</button> : null}
        </div>
      </header>
      <main className="mx-auto flex max-w-[880px] flex-col gap-6 px-6 pb-16 pt-8">
        {c.state === 'maintenance' ? (
          <div className="flex items-start gap-2.5 rounded-md bg-accent-subtle px-4 py-3 text-control text-accent-text"><Wrench className="mt-0.5 size-4" aria-hidden /><div><span className="font-semibold">Scheduled maintenance · Thu Oct 22, 22:00 – 23:00 CEST</span><br />Payouts will be paused while we upgrade the payout ledger.</div></div>
        ) : null}
        <section aria-live="polite" className={cn('flex items-center gap-4 rounded-lg border bg-card px-6 py-5', incident ? 'border-warning-border' : 'border-success-border')}>
          {incident ? <TriangleAlert className="size-7 text-warning-text" aria-hidden /> : <CircleCheck className="size-7 text-success-text" aria-hidden />}
          <div className="flex flex-col gap-0.5"><h1 className="text-[24px] font-bold leading-8">{incident ? 'Degraded performance on Checkout' : 'All systems operational'}</h1><span className="text-control text-muted-foreground">Updated 14:41 CEST · refreshes every minute</span></div>
        </section>
        {incident ? (
          <article className="overflow-hidden rounded-lg border border-warning-border bg-card">
            <div className="flex flex-col gap-1.5 border-b border-warning-border bg-warning-subtle px-5 py-4">
              <div className="flex flex-wrap items-center gap-2"><Badge variant="warning"><TriangleAlert />Degraded performance</Badge><span className="text-control text-warning-text">Affects Checkout</span></div>
              <h2 className="text-[18px] font-bold leading-[26px]">Some card payments fail at checkout</h2>
            </div>
            <ol className="m-0 flex list-none flex-col px-5 pb-4 pt-1">
              {c.updates.map((u, i) => (
                <li key={i} className="grid grid-cols-[110px_minmax(0,1fr)] gap-x-4 border-b border-border-subtle py-3">
                  <div className="flex flex-col"><span className="text-control font-semibold">{u.status}</span><time className="font-mono text-caption text-muted-foreground">{u.time}</time></div>
                  <p className="m-0 text-[15px] leading-[23px]">{u.text}</p>
                </li>
              ))}
            </ol>
          </article>
        ) : null}
        <section className="overflow-hidden rounded-lg border bg-card" aria-label="Components">
          <div className="flex items-center gap-3 border-b px-5 py-3.5"><h2 className="flex-1 text-base font-bold">Components</h2><span className="text-caption text-muted-foreground">Last {c.days} days</span></div>
          {groups.map((g) => {
            const items = g.items.filter((i) => !c.hidden.includes(i.name))
            if (!items.length) return null
            return (
              <div key={g.name}>
                <div className="px-5 pt-2.5 text-[11px] font-semibold uppercase tracking-[.05em] text-muted-foreground">{g.name}</div>
                {items.map((i) => (
                  <div key={i.name} className="flex flex-col gap-2.5 border-b border-border-subtle px-5 pb-4 pt-3">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-semibold">{i.name}</span>
                      {c.showDesc ? <span className="text-caption text-muted-foreground">{i.desc}</span> : null}
                      <span className="flex-1" />
                      {i.live && incident ? <StatusIndicator status="degraded" /> : i.name === 'Payouts' && c.state === 'maintenance' ? <StatusIndicator status="maintenance" label="Maintenance tonight" /> : <StatusIndicator status="operational" />}
                    </div>
                    {c.showUptime ? <UptimeBars days={c.days} bad={i.bad} fresh={i.fresh} live={i.live} incident={incident} name={i.name} /> : null}
                  </div>
                ))}
              </div>
            )
          })}
        </section>
        {c.showPast ? (
          <section className="flex flex-col gap-3" aria-label="Past incidents">
            <h2 className="text-[18px] font-bold">Past incidents</h2>
            <div className="rounded-lg border bg-card">
              {[['Oct 20, 2026', incident ? 'Some card payments fail at checkout' : '', incident ? 'Ongoing · Degraded · Checkout' : ''], ['Oct 19, 2026', '', ''], ['Sep 17, 2026', 'Checkout unavailable for 12 minutes', 'Resolved in 12 min · Partial outage'], ['Aug 29, 2026', 'Slow API responses', 'Resolved in 41 min · Degraded · Public API']].map(([d, t, m]) => (
                <div key={d} className="grid grid-cols-[120px_minmax(0,1fr)] gap-x-4 border-b border-border-subtle px-5 py-3.5">
                  <time className="text-control font-semibold">{d}</time>
                  {t ? <div className="flex flex-col"><span className="font-semibold">{t}</span><span className="text-caption text-muted-foreground">{m}</span></div> : <span className="text-control text-muted-foreground">No incidents reported.</span>}
                </div>
              ))}
            </div>
          </section>
        ) : null}
      </main>
      {c.poweredBy ? <footer className="border-t bg-background"><div className="mx-auto flex max-w-[880px] items-center gap-2 px-6 py-5 text-caption text-muted-foreground"><span className="flex-1">Subscribe by email, RSS or webhook.</span><Logo className="size-3.5" />Powered by Ferristrace</div></footer> : null}
    </div>
  )
}
