import { useMetricCatalogue, useMetricSeries, type MetricLine } from '@/data/metrics'
import { NOWHERE, type Placement } from '@/data/placement'

export const HOST_KEY = 'host.name'

const CPU = 'system.cpu.utilization'
const MEMORY = 'system.memory.utilization'
const DISK = 'system.filesystem.utilization'
const LOAD = 'system.cpu.load_average.1m'

export const HOST_METRICS: readonly string[] = [CPU, MEMORY, DISK, LOAD]

export type Host = {
  name: string
  cpu: number | null
  memory: number | null
  disk: number | null
  load: number | null
}

const state = (value: string) => ({ attr: 'state', op: 'eq', value })

const nameOf = (line: MetricLine) => line.labels[HOST_KEY] ?? ''

function latest(line: MetricLine): number | null {
  for (let i = line.points.length - 1; i >= 0; i -= 1) {
    const value = line.points[i][1]
    if (value !== null) return value
  }
  return null
}

const valueOf = (lines: MetricLine[], host: string): number | null => {
  const line = lines.find((one) => nameOf(one) === host)
  return line === undefined ? null : latest(line)
}

const percent = (ratio: number | null) => (ratio === null ? null : ratio * 100)

const busied = (line: MetricLine): MetricLine => ({
  labels: line.labels,
  points: line.points.map(([at, idle]) => [at, idle === null ? null : (1 - idle) * 100]),
})

export function useHosts(rangeMs: number, placement: Placement = NOWHERE) {
  const { metrics } = useMetricCatalogue('system.')
  const over = { rangeMs, fromMs: null, toMs: null, groupBy: [HOST_KEY] }
  const idle = useMetricSeries({ ...over, name: CPU, agg: 'avg', where: state('idle') }, placement)
  const memory = useMetricSeries({ ...over, name: MEMORY, agg: 'sum', where: state('used') }, placement)
  const disk = useMetricSeries({ ...over, name: DISK, agg: 'max', where: state('used') }, placement)
  const load = useMetricSeries({ ...over, name: LOAD, agg: 'avg' }, placement)

  const asked = [idle, memory, disk, load]
  const seen = asked.map((one) => one.result?.series ?? [])
  const names = [...new Set(seen.flat().map(nameOf).filter(Boolean))].sort()
  const [quiet, used, full, loaded] = seen

  const hosts: Host[] = names.map((name) => {
    const resting = valueOf(quiet, name)
    return {
      name,
      cpu: resting === null ? null : (1 - resting) * 100,
      memory: percent(valueOf(used, name)),
      disk: percent(valueOf(full, name)),
      load: valueOf(loaded, name),
    }
  })

  return {
    hosts,
    busy: quiet.map(busied),
    pending: asked.some((one) => one.pending),
    failed: asked.map((one) => one.failed).find(Boolean) ?? null,
    instrumented: metrics.some((one) => HOST_METRICS.includes(one.name)),
  }
}
