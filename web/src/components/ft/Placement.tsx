import { Boxes } from 'lucide-react'
import { NativeSelect } from '@/components/ui/input'
import { chipsOf, isPlaced, usePlacements, type Place, type Placement } from '@/data/placement'
import { cn } from '@/lib/utils'
import { Filters } from './Filters'

function kept(values: string[], chosen: string): string[] {
  const unique = Array.from(new Set(values))
  return chosen && !unique.includes(chosen) ? [chosen, ...unique] : unique
}

export const placedCount = (placement: Placement) => Object.values(placement).filter((value) => value.trim() !== '').length

export function PlacementPicker({ placement, onChange, windowMs, collapsible }: { placement: Placement; onChange: (part: Partial<Placement>) => void; windowMs: number; collapsible?: boolean }) {
  const { seen } = usePlacements(windowMs)
  const inNamespace = (namespace: string) => !placement.namespace || namespace === placement.namespace
  const workloads = kept(seen.workloads.filter((w) => inNamespace(w.namespace)).map((w) => `${w.kind}:${w.name}`), placement.workload)
  const pods = kept(seen.pods.filter((p) => inNamespace(p.namespace) && (!placement.node || p.node === placement.node)).map((p) => p.name), placement.pod)
  const select = (place: Place, label: string, options: string[], width: string) => (
    <NativeSelect aria-label={label} value={placement[place]} onChange={(e) => onChange({ [place]: e.target.value })} className={cn(width, placement[place] && 'font-mono')}>
      <option value="">{label}: any</option>
      {options.map((option) => <option key={option} value={option}>{option}</option>)}
    </NativeSelect>
  )
  const nothing = seen.clusters.length + seen.namespaces.length + seen.workloads.length + seen.pods.length + seen.nodes.length === 0
  if (nothing && !isPlaced(placement)) return null
  const picker = (
    <div className="flex flex-wrap items-center gap-2 max-md:grid max-md:grid-cols-2" role="group" aria-label="Kubernetes placement">
      <Boxes className="size-3.5 text-muted-foreground max-md:hidden" aria-hidden />
      {select('cluster', 'Cluster', kept(seen.clusters, placement.cluster), 'w-[150px]')}
      {select('namespace', 'Namespace', kept(seen.namespaces, placement.namespace), 'w-[160px]')}
      {select('workload', 'Workload', workloads, 'w-[200px]')}
      {select('pod', 'Pod', pods, 'w-[200px]')}
      {select('node', 'Node', kept(seen.nodes, placement.node), 'w-[170px]')}
    </div>
  )
  return collapsible ? <Filters active={placedCount(placement)}>{picker}</Filters> : picker
}

export function PlacementChips({ attributes, onPick, className }: { attributes: Record<string, string>; onPick: (part: Partial<Placement>) => void; className?: string }) {
  const chips = chipsOf(attributes)
  if (chips.length === 0) return null
  return (
    <div className={cn('flex flex-wrap gap-1', className)} role="group" aria-label="Filter by placement">
      {chips.map((chip) => (
        <button
          key={chip.place}
          type="button"
          title={`Filter by ${chip.place} ${chip.value}`}
          onClick={(e) => { e.stopPropagation(); onPick({ [chip.place]: chip.value }) }}
          className="inline-flex h-6 cursor-pointer items-center rounded-sm border bg-muted px-2 font-mono text-caption hover:bg-row-hover focus-visible:outline-2 focus-visible:outline-ring"
        >
          {chip.label}
        </button>
      ))}
    </div>
  )
}
