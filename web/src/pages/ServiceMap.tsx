import { useId, useMemo, useState } from 'react'
import { TriangleAlert } from 'lucide-react'
import { Link } from '@/lib/router'
import { InputGroup } from '@/components/ui/input'
import { Table, Td, Th, Tr } from '@/components/ui/misc'
import { EmptyState, PageHeader, ServiceDot } from '@/components/ft/bits'
import { ERROR_THRESHOLD, useServiceDependencies } from '@/data/services'
import { svcIndex } from '@/lib/otel'
import { useWindow } from '@/lib/window'
import { placementSearch, usePlacement } from '@/data/placement'
import { PlacementPicker } from '@/components/ft/Placement'
import { because } from '@/auth/client'
import { useSettled } from '@/lib/settle'
import { cn } from '@/lib/utils'

const SIZE = 560
const CENTER = SIZE / 2
const RADIUS = CENTER - 64

function layout(names: string[]): Map<string, { x: number; y: number }> {
  const positions = new Map<string, { x: number; y: number }>()
  names.forEach((name, index) => {
    const angle = (index / Math.max(1, names.length)) * 2 * Math.PI - Math.PI / 2
    positions.set(name, { x: CENTER + RADIUS * Math.cos(angle), y: CENTER + RADIUS * Math.sin(angle) })
  })
  return positions
}

function nodeRadius(calls: number, max: number): number {
  if (max <= 0) return 12
  return 11 + (calls / max) * 15
}

export function ServiceMap() {
  const { ms: windowMs, label } = useWindow()
  const [typedEnv, setTypedEnv] = useState('')
  const settledEnv = useSettled(typedEnv)
  const { placement, place } = usePlacement()
  const { edges, pending, failed } = useServiceDependencies(settledEnv, windowMs, placement)
  const kept = { ...(settledEnv.trim() ? { env: settledEnv.trim() } : {}), ...placementSearch(placement) }
  const markerId = useId()

  const names = useMemo(() => {
    const seen = new Set<string>()
    edges.forEach((edge) => { seen.add(edge.caller); seen.add(edge.callee) })
    return [...seen].sort()
  }, [edges])
  const positions = useMemo(() => layout(names), [names])
  const callsOf = useMemo(() => {
    const totals = new Map<string, number>()
    edges.forEach((edge) => {
      totals.set(edge.caller, (totals.get(edge.caller) ?? 0) + edge.calls)
      totals.set(edge.callee, (totals.get(edge.callee) ?? 0) + edge.calls)
    })
    return totals
  }, [edges])
  const maxCalls = Math.max(1, ...callsOf.values())
  const rows = useMemo(() => [...edges].sort((a, b) => b.calls - a.calls), [edges])

  return (
    <>
      <PageHeader title="Service map" meta={<>Built from each span whose parent lives in another service · last {label}</>} />
      <div className="flex flex-wrap items-center gap-2" role="search">
        <InputGroup className="w-[220px] max-w-full"><input aria-label="Environment" placeholder="Environment, e.g. production" value={typedEnv} onChange={(e) => setTypedEnv(e.target.value)} /></InputGroup>
      </div>
      <PlacementPicker collapsible placement={placement} onChange={place} windowMs={windowMs} />
      {failed ? <EmptyState title="The dependency map could not be read">{because(failed, 'The span store did not answer')}</EmptyState> : null}
      {!failed && !pending && edges.length === 0 ? (
        <EmptyState title={settledEnv.trim() ? `No cross-service calls in ${settledEnv.trim()} in the last ${label}` : `No cross-service calls in the last ${label}`}>
          An edge appears once a server or consumer span's parent was recorded under a different service.name.
        </EmptyState>
      ) : null}
      {edges.length > 0 ? (
        <div className="flex flex-col gap-4 lg:flex-row">
          <figure className="m-0 flex min-w-0 flex-1 flex-col gap-2 rounded-lg border bg-card p-3">
            <svg viewBox={`0 0 ${SIZE} ${SIZE}`} width="100%" height={SIZE} role="img" aria-label={`Service dependency map, ${edges.length} call paths, last ${label}`} className="block overflow-visible">
              <defs>
                <marker id={`${markerId}-arrow`} viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                  <path d="M0,0 L10,5 L0,10 z" fill="var(--muted-foreground)" />
                </marker>
              </defs>
              {rows.map((edge) => {
                const from = positions.get(edge.caller)
                const to = positions.get(edge.callee)
                if (!from || !to) return null
                const dx = to.x - from.x
                const dy = to.y - from.y
                const length = Math.hypot(dx, dy) || 1
                const toR = nodeRadius(callsOf.get(edge.callee) ?? 0, maxCalls) + 8
                const fromR = nodeRadius(callsOf.get(edge.caller) ?? 0, maxCalls) + 2
                const x1 = from.x + (dx / length) * fromR
                const y1 = from.y + (dy / length) * fromR
                const x2 = to.x - (dx / length) * toR
                const y2 = to.y - (dy / length) * toR
                const bad = (edge.err ?? 0) >= ERROR_THRESHOLD
                const width = Math.min(8, 1.5 + Math.log2(edge.calls + 1))
                return (
                  <line
                    key={`${edge.caller}->${edge.callee}`}
                    x1={x1} y1={y1} x2={x2} y2={y2}
                    stroke={bad ? 'var(--destructive-text)' : 'var(--border)'}
                    strokeWidth={width}
                    strokeLinecap="round"
                    markerEnd={`url(#${markerId}-arrow)`}
                  >
                    <title>{`${edge.caller} → ${edge.callee}: ${edge.calls} calls${bad ? `, ${(edge.err ?? 0).toFixed(1)} % errors` : ''}`}</title>
                  </line>
                )
              })}
              {names.map((name) => {
                const at = positions.get(name)
                if (!at) return null
                const r = nodeRadius(callsOf.get(name) ?? 0, maxCalls)
                return (
                  <g key={name}>
                    <circle cx={at.x} cy={at.y} r={r} fill={`var(--svc-${svcIndex(name)})`} stroke="var(--card)" strokeWidth="2" />
                    <text x={at.x} y={at.y + r + 14} textAnchor="middle" className="fill-foreground text-[11px] font-mono font-semibold">{name}</text>
                  </g>
                )
              })}
            </svg>
          </figure>
          <div className="min-w-0 overflow-x-auto rounded-lg border lg:w-[420px]">
            <Table aria-label="Dependencies">
              <thead><tr><Th>Caller</Th><Th>Callee</Th><Th numeric className="w-[90px]">Calls</Th><Th numeric className="w-[100px]">Error rate</Th></tr></thead>
              <tbody>
                {rows.map((edge) => {
                  const bad = (edge.err ?? 0) >= ERROR_THRESHOLD
                  return (
                    <Tr key={`${edge.caller}->${edge.callee}`}>
                      <Td><Link to={`/services/${edge.caller}`} search={kept} className="inline-flex items-center gap-2 text-foreground no-underline"><ServiceDot name={edge.caller} /><span className="font-mono">{edge.caller}</span></Link></Td>
                      <Td><Link to={`/services/${edge.callee}`} search={kept} className="inline-flex items-center gap-2 text-foreground no-underline"><ServiceDot name={edge.callee} /><span className="font-mono">{edge.callee}</span></Link></Td>
                      <Td numeric>{edge.calls}</Td>
                      <Td numeric className={cn(bad && 'font-semibold text-warning-text')}>
                        <span className="inline-flex items-center gap-1">{bad ? <TriangleAlert className="size-3.5" aria-hidden /> : null}{edge.err === null ? '—' : `${edge.err.toFixed(1)} %`}</span>
                      </Td>
                    </Tr>
                  )
                })}
              </tbody>
            </Table>
          </div>
        </div>
      ) : null}
    </>
  )
}
