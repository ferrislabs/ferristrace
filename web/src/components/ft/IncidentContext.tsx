import { Bug, GitCommitVertical, Server, Waypoints } from 'lucide-react'
import type { ReactNode } from 'react'
import { Link } from '@/lib/router'
import { Card, CardHeader, CardTitle } from '@/components/ui/card'
import { useIncidentContext } from '@/data/incidents'
import { releasePath } from '@/data/releases'
import { dur, stamp } from '@/lib/format'

const ROW = 'flex items-center gap-2 border-b border-border-subtle px-4 py-2 text-control text-foreground no-underline hover:bg-muted'

function Group({ title, left, children }: { title: string; left: number; children: ReactNode }) {
  return (
    <div>
      <div className="px-4 pb-1 pt-3 text-caption font-semibold uppercase text-muted-foreground">{title}</div>
      {children}
      {left > 0 ? <div className="px-4 py-1.5 text-caption text-muted-foreground">and {left} more</div> : null}
    </div>
  )
}

export function IncidentContext({ id }: { id: string }) {
  const { context } = useIncidentContext(id)
  if (!context) return null
  const { deploys, issues, resources, traces, left_out: left } = context
  if (deploys.length + issues.length + resources.length + traces.length === 0) return null
  return (
    <Card className="overflow-hidden">
      <CardHeader>
        <CardTitle className="flex-1">Context</CardTitle>
        <span className="text-caption text-muted-foreground">{context.services.join(', ')} · since {stamp(context.from_ms)}</span>
      </CardHeader>
      {deploys.length > 0 ? (
        <Group title="Deployed" left={left.deploys}>
          {deploys.map((one) => (
            <Link key={`${one.service}/${one.version}/${one.at_ms}`} to={releasePath(one.service, one.version)} className={ROW}>
              <GitCommitVertical className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
              <span className="font-mono">{one.service} {one.version}</span>
              <span className="text-caption text-muted-foreground">{one.environment} · {stamp(one.at_ms)}</span>
            </Link>
          ))}
        </Group>
      ) : null}
      {issues.length > 0 ? (
        <Group title="New errors" left={left.issues}>
          {issues.map((one) => (
            <Link key={one.print} to={`/issues/${one.print}`} className={ROW}>
              <Bug className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
              <span className="truncate font-mono">{one.title}</span>
              <span className="truncate text-caption text-muted-foreground">{one.service} · {one.occurrences} times</span>
            </Link>
          ))}
        </Group>
      ) : null}
      {resources.length > 0 ? (
        <Group title="Runs on" left={left.resources}>
          {resources.map((one) => (
            <Link key={`${one.kind}/${one.id}`} to={one.kind === 'cluster' ? '/infra/kubernetes' : '/infra'} className={ROW}>
              <Server className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
              <span className="truncate font-mono">{one.name}</span>
              <span className="text-caption text-muted-foreground">{one.kind}</span>
              {one.silent ? <span className="text-caption font-semibold text-warning-text">silent</span> : null}
            </Link>
          ))}
        </Group>
      ) : null}
      {traces.length > 0 ? (
        <Group title="Slowest failing traces" left={left.traces}>
          {traces.map((one) => (
            <Link key={one.trace_id} to={`/traces/${one.trace_id}`} className={ROW}>
              <Waypoints className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
              <span className="truncate font-mono">{one.service} {one.operation}</span>
              <span className="text-caption text-muted-foreground">{dur(one.duration_ms)}</span>
            </Link>
          ))}
        </Group>
      ) : null}
    </Card>
  )
}
