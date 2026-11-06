export type Scalar = string | number | boolean

export type BuiltIn = 'service' | 'name' | 'status' | 'kind' | 'duration' | 'http_status_code' | 'trace_id' | 'span_id'

export type FieldRef = { built: BuiltIn } | { attr: string }

export type Op =
  | 'eq' | 'ne' | 'gt' | 'gte' | 'lt' | 'lte'
  | 'contains' | 'not_contains' | 'in' | 'not_in'
  | 'exists' | 'not_exists'

export type Kind = 'text' | 'number' | 'choice'

export type Chip = { field: FieldRef; op: Op; value?: Scalar | Scalar[] }

export const MAX_CHIPS = 32

export const STATUSES = ['ok', 'error', 'unset']

export const KINDS = ['server', 'client', 'internal', 'producer', 'consumer', 'unspecified']

export const BUILT_INS: { name: BuiltIn; label: string; kind: Kind; unit?: string }[] = [
  { name: 'service', label: 'Service', kind: 'text' },
  { name: 'name', label: 'Span name', kind: 'text' },
  { name: 'status', label: 'Status', kind: 'choice' },
  { name: 'kind', label: 'Kind', kind: 'choice' },
  { name: 'duration', label: 'Duration', kind: 'number', unit: 'ms' },
  { name: 'http_status_code', label: 'HTTP status code', kind: 'number' },
  { name: 'trace_id', label: 'Trace ID', kind: 'text' },
  { name: 'span_id', label: 'Span ID', kind: 'text' },
]

export const OPS: Record<Op, { label: string; list?: boolean; bare?: boolean }> = {
  eq: { label: 'is' },
  ne: { label: 'is not' },
  gt: { label: '>' },
  gte: { label: '≥' },
  lt: { label: '<' },
  lte: { label: '≤' },
  contains: { label: 'contains' },
  not_contains: { label: 'does not contain' },
  in: { label: 'is one of', list: true },
  not_in: { label: 'is not one of', list: true },
  exists: { label: 'exists', bare: true },
  not_exists: { label: 'does not exist', bare: true },
}

const TEXT_OPS: Op[] = ['eq', 'ne', 'contains', 'not_contains', 'in', 'not_in']
const CHOICE_OPS: Op[] = ['eq', 'ne', 'in', 'not_in']
const NUMBER_OPS: Op[] = ['eq', 'ne', 'gt', 'gte', 'lt', 'lte', 'in', 'not_in']
const PRESENCE: Op[] = ['exists', 'not_exists']

export function builtIn(name: BuiltIn) {
  return BUILT_INS.find((field) => field.name === name)!
}

export function kindOf(field: FieldRef, numeric: boolean): Kind {
  if ('built' in field) return builtIn(field.built).kind
  return numeric ? 'number' : 'text'
}

export function opsFor(field: FieldRef, numeric: boolean): Op[] {
  const kind = kindOf(field, numeric)
  const own = kind === 'number' ? NUMBER_OPS : kind === 'choice' ? CHOICE_OPS : TEXT_OPS
  return 'attr' in field ? [...own, ...PRESENCE] : own
}

export function labelOf(field: FieldRef): string {
  return 'built' in field ? builtIn(field.built).label : field.attr
}

export function sameField(a: FieldRef, b: FieldRef): boolean {
  return 'built' in a ? 'built' in b && a.built === b.built : 'attr' in b && a.attr === b.attr
}

export function parsedValue(text: string, kind: Kind, list: boolean): Scalar | Scalar[] | null {
  const one = (raw: string): Scalar | null => {
    const trimmed = raw.trim()
    if (trimmed === '') return null
    if (kind !== 'number') return trimmed
    const number = Number(trimmed)
    return Number.isFinite(number) ? number : null
  }
  if (!list) return one(text)
  const parts = text.split(',').map(one)
  if (parts.length === 0 || parts.some((part) => part === null)) return null
  return parts as Scalar[]
}

export const toWire = (chip: Chip) => {
  const base = 'built' in chip.field ? { field: chip.field.built } : { attr: chip.field.attr }
  const timed = 'built' in chip.field && chip.field.built === 'duration'
  const nanos = (scalar: Scalar): Scalar => (timed && typeof scalar === 'number' ? scalar * 1_000_000 : scalar)
  const value = Array.isArray(chip.value) ? chip.value.map(nanos) : chip.value === undefined ? undefined : nanos(chip.value)
  return value === undefined ? { ...base, op: chip.op } : { ...base, op: chip.op, value }
}

export function toBody(chips: Chip[]): unknown | undefined {
  if (chips.length === 0) return undefined
  const clauses = chips.map(toWire)
  return clauses.length === 1 ? clauses[0] : { all: clauses }
}

export function encode(chips: Chip[]): string | undefined {
  return chips.length === 0 ? undefined : JSON.stringify(chips)
}

const isScalar = (value: unknown): value is Scalar =>
  typeof value === 'string' || typeof value === 'boolean' || (typeof value === 'number' && Number.isFinite(value))

const isField = (value: unknown): value is FieldRef => {
  if (typeof value !== 'object' || value === null) return false
  const field = value as Record<string, unknown>
  if (typeof field.built === 'string') return BUILT_INS.some((known) => known.name === field.built)
  return typeof field.attr === 'string' && field.attr.length > 0 && field.attr.length <= 128
}

export function validChips(parsed: unknown): Chip[] {
  if (!Array.isArray(parsed)) return []
  return parsed
    .filter((item): item is Chip => {
      if (typeof item !== 'object' || item === null) return false
      const chip = item as Record<string, unknown>
      const value = chip.value
      const good = value === undefined || isScalar(value) || (Array.isArray(value) && value.every(isScalar))
      return isField(chip.field) && typeof chip.op === 'string' && chip.op in OPS && good
    })
    .slice(0, MAX_CHIPS)
}

export function decode(raw: string | undefined): Chip[] {
  if (!raw) return []
  try {
    return validChips(JSON.parse(raw))
  } catch {
    return []
  }
}

export function show(value: Chip['value']): string {
  if (value === undefined) return ''
  return Array.isArray(value) ? value.join(', ') : String(value)
}

const managed = (chip: Chip, field: FieldRef) => sameField(chip.field, field) && (chip.op === 'eq' || chip.op === 'in')

export function selectedValues(chips: Chip[], field: FieldRef): string[] {
  const chip = chips.find((one) => managed(one, field))
  if (!chip || chip.value === undefined) return []
  return (Array.isArray(chip.value) ? chip.value : [chip.value]).map(String)
}

export function withValue(chips: Chip[], field: FieldRef, value: string, on: boolean, numeric: boolean): Chip[] {
  const held = selectedValues(chips, field)
  const next = on ? (held.includes(value) ? held : [...held, value]) : held.filter((one) => one !== value)
  const at = chips.findIndex((one) => managed(one, field))
  const typed = (text: string): Scalar => (numeric && Number.isFinite(Number(text)) ? Number(text) : text)
  const chip: Chip | null =
    next.length === 0 ? null : next.length === 1 ? { field, op: 'eq', value: typed(next[0]) } : { field, op: 'in', value: next.map(typed) }
  if (at === -1) return chip ? [...chips, chip] : chips
  return chip ? chips.map((one, index) => (index === at ? chip : one)) : chips.filter((_, index) => index !== at)
}

const DURATION: FieldRef = { built: 'duration' }

const isBound = (chip: Chip) => sameField(chip.field, DURATION) && (chip.op === 'gte' || chip.op === 'lte')

export function durationBounds(chips: Chip[]): { min?: number; max?: number } {
  const bound = (op: Op) => {
    const value = chips.find((chip) => isBound(chip) && chip.op === op)?.value
    return typeof value === 'number' ? value : undefined
  }
  return { min: bound('gte'), max: bound('lte') }
}

export function withDuration(chips: Chip[], min: number | undefined, max: number | undefined): Chip[] {
  const kept = chips.filter((chip) => !isBound(chip))
  const added: Chip[] = []
  if (min !== undefined) added.push({ field: DURATION, op: 'gte', value: min })
  if (max !== undefined) added.push({ field: DURATION, op: 'lte', value: max })
  return [...kept, ...added]
}
