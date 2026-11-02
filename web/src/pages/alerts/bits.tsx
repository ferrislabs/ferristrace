import { Badge } from '@/components/ui/badge'
import type { AlertContext as Context, AlertStatus } from '@/data/alerts'

const VARIANT: Record<AlertStatus, 'destructive' | 'warning' | 'neutral' | 'success'> = {
  firing: 'destructive',
  pending: 'warning',
  no_data: 'neutral',
  ok: 'success',
}

export const StateBadge = ({ status }: { status: AlertStatus }) => <Badge variant={VARIANT[status]}>{status.replace('_', ' ')}</Badge>


export function AlertContext({ context }: { context: Context }) {
  const labels = Object.entries(context.labels)
  if (!context.pods.length && !context.nodes.length && !context.versions.length && !labels.length) return null
  return (
    <span className="flex flex-col gap-0.5 font-mono text-caption text-muted-foreground" aria-label="What fired">
      {context.pods.map((pod) => (
        <span key={pod.name}>{pod.name}{pod.node ? ` (${pod.node})` : ''} · {pod.count.toLocaleString('en-US')}</span>
      ))}
      {context.others > 0 ? <span>and {context.others.toLocaleString('en-US')} more {context.others === 1 ? 'pod' : 'pods'}</span> : null}
      {context.nodes.length ? <span>nodes {context.nodes.join(', ')}</span> : null}
      {context.versions.length ? <span>version {context.versions.join(', ')}</span> : null}
      {labels.length ? <span>{labels.map(([key, value]) => `${key}=${value}`).join(', ')}</span> : null}
    </span>
  )
}
