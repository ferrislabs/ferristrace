import { Card } from '@/components/ui/card'

export function Meter({ label, used, of, share, note }: { label: string; used: string; of?: string; share: number | null; note: string }) {
  return (
    <Card className="flex flex-col gap-2 p-3.5">
      <div className="flex items-baseline gap-2">
        <span className="flex-1 text-control font-semibold">{label}</span>
        <span className="font-mono text-control">{used}</span>
        {of ? <span className="text-caption text-muted-foreground">{of}</span> : null}
      </div>
      {share === null ? null : (
        <div className="relative h-2 rounded bg-muted" role="img" aria-label={`${share.toFixed(0)} percent`}>
          <div className="absolute inset-y-0 left-0 rounded bg-series" style={{ width: `${Math.min(100, share)}%` }} />
        </div>
      )}
      <span className="text-caption text-muted-foreground">{note}</span>
    </Card>
  )
}
