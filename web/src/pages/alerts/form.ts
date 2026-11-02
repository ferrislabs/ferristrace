import {
  PENDING_MAX_S,
  WINDOW_MAX_S,
  WINDOW_MIN_S,
  type LogSeverity,
  type NoData,
  type Op,
  type Rule,
  type RuleDefinition,
  type RuleSeverity,
  type RuleSignal,
  type SignalKind,
  type TraceMeasure,
} from '@/data/alerts'
import { AGGREGATIONS, type Aggregation } from '@/data/metrics'

export type Form = {
  name: string
  project: string
  kind: SignalKind
  service: string
  operation: string
  environment: string
  traceMeasure: TraceMeasure
  logServices: string
  logSeverities: LogSeverity[]
  logText: string
  logAttrs: string
  metricName: string
  aggregation: Aggregation
  groupBy: string
  filter: string
  op: Op
  threshold: string
  windowMin: string
  pendingMin: string
  severity: RuleSeverity
  noData: NoData
  channels: string[]
  page: string
  enabled: boolean
}

export const BLANK: Form = {
  name: '',
  project: '',
  kind: 'traces',
  service: '',
  operation: '',
  environment: '',
  traceMeasure: 'error_rate',
  logServices: '',
  logSeverities: [],
  logText: '',
  logAttrs: '',
  metricName: '',
  aggregation: 'avg',
  groupBy: '',
  filter: '',
  op: '>',
  threshold: '',
  windowMin: '5',
  pendingMin: '0',
  severity: 'warning',
  noData: 'no_data',
  channels: [],
  page: '',
  enabled: true,
}

const list = (text: string) => text.split(/[,\n]/).map((part) => part.trim()).filter((part) => part.length > 0)

const minutes = (seconds: number) => String(seconds / 60)

export function formOf(rule: Rule): Form {
  const base: Form = {
    ...BLANK,
    name: rule.name,
    project: rule.project ?? '',
    kind: rule.signal.kind,
    op: rule.condition.op,
    threshold: String(rule.condition.threshold),
    windowMin: minutes(rule.window_s),
    pendingMin: minutes(rule.pending_s),
    severity: rule.severity,
    noData: rule.no_data,
    channels: rule.channels,
    page: rule.page?.schedule ?? '',
    enabled: rule.enabled,
  }
  const signal = rule.signal
  if (signal.kind === 'traces') {
    return { ...base, service: signal.service ?? '', operation: signal.operation ?? '', environment: signal.environment ?? '', traceMeasure: signal.measure }
  }
  if (signal.kind === 'logs') {
    return { ...base, logServices: signal.services.join(', '), logSeverities: signal.severities, logText: signal.q ?? '', logAttrs: signal.attrs.join(', ') }
  }
  return {
    ...base,
    metricName: signal.name,
    aggregation: signal.aggregation,
    groupBy: signal.group_by.join(', '),
    filter: Object.entries(signal.filter).map(([k, v]) => `${k}=${v}`).join(', '),
  }
}

export const aggregationsFor = (kind: keyof typeof AGGREGATIONS | undefined): Aggregation[] =>
  kind ? AGGREGATIONS[kind] : [...new Set(Object.values(AGGREGATIONS).flat())]

function pairs(text: string): Record<string, string> | string {
  const out: Record<string, string> = {}
  for (const part of list(text)) {
    const at = part.indexOf('=')
    if (at < 1) return `"${part}" is not key=value`
    out[part.slice(0, at).trim()] = part.slice(at + 1).trim()
  }
  return out
}

function signalOf(form: Form): RuleSignal | string {
  if (form.kind === 'traces') {
    return {
      kind: 'traces',
      service: form.service.trim() || null,
      operation: form.operation.trim() || null,
      environment: form.environment.trim() || null,
      measure: form.traceMeasure,
    }
  }
  if (form.kind === 'logs') {
    const attrs = list(form.logAttrs)
    const bad = attrs.find((attr) => attr.indexOf('=') < 1)
    if (bad) return `"${bad}" is not key=value`
    return {
      kind: 'logs',
      services: list(form.logServices),
      severities: form.logSeverities,
      q: form.logText.trim() || null,
      attrs,
      measure: 'count',
    }
  }
  if (!form.metricName.trim()) return 'Choose a metric'
  const filter = pairs(form.filter)
  if (typeof filter === 'string') return filter
  return { kind: 'metrics', name: form.metricName.trim(), aggregation: form.aggregation, group_by: list(form.groupBy), filter }
}

export function definitionFrom(form: Form): { definition: RuleDefinition } | { problem: string } {
  if (!form.name.trim()) return { problem: 'A rule needs a name' }
  const threshold = Number(form.threshold)
  if (form.threshold.trim() === '' || !Number.isFinite(threshold)) return { problem: 'The threshold must be a number' }
  const window = Math.round(Number(form.windowMin) * 60)
  if (!Number.isFinite(window) || window < WINDOW_MIN_S || window > WINDOW_MAX_S) return { problem: 'The window runs from 1 minute to 24 hours' }
  const pending = Math.round(Number(form.pendingMin) * 60)
  if (!Number.isFinite(pending) || pending < 0 || pending > PENDING_MAX_S) return { problem: 'The pending duration runs from 0 to 60 minutes' }
  const signal = signalOf(form)
  if (typeof signal === 'string') return { problem: signal }
  return {
    definition: {
      name: form.name.trim(),
      project: form.project || null,
      enabled: form.enabled,
      signal,
      condition: { op: form.op, threshold },
      window_s: window,
      pending_s: pending,
      severity: form.severity,
      no_data: form.noData,
      channels: form.channels,
      page: form.page ? { schedule: form.page } : null,
    },
  }
}
