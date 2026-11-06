import {
  BUILT_INS, CHOICES, decode, kindOf, opsFor, toWire,
  type BuiltIn, type Chip, type FieldRef, type Op, type Scalar,
} from './traceQuery'

export type Node = Chip | { all: Node[] } | { any: Node[] } | { not: Node }

export type Parsed = { ok: true; items: Node[] } | { ok: false; error: string; at: number }

export const MAX_DEPTH = 8
export const MAX_CLAUSES = 32

const KEYWORDS = new Set(['and', 'or', 'not', 'in', 'contains', 'exists'])
const BUILT_NAMES = new Set<string>(BUILT_INS.map((field) => field.name))
const UNITS: Record<string, number> = { us: 0.001, ms: 1, s: 1000, m: 60_000 }

type Token =
  | { t: 'word' | 'str' | 'sym'; v: string; start: number; end: number }
  | { t: 'num'; v: string; n: number; unit: string; start: number; end: number }

class Failure extends Error {
  readonly at: number

  constructor(message: string, at: number) {
    super(message)
    this.at = at
  }
}

export const isChip = (node: Node): node is Chip => 'field' in node

function tokenize(text: string): Token[] {
  const tokens: Token[] = []
  let i = 0
  while (i < text.length) {
    const c = text[i]
    if (/\s/.test(c)) { i += 1; continue }
    if (c === '(' || c === ')' || c === ',') { tokens.push({ t: 'sym', v: c, start: i, end: i + 1 }); i += 1; continue }
    if (c === '"' || c === "'") {
      let j = i + 1
      let value = ''
      while (j < text.length && text[j] !== c) {
        if (text[j] === '\\' && j + 1 < text.length) j += 1
        value += text[j]
        j += 1
      }
      if (j >= text.length) throw new Failure('A quote is not closed', i)
      tokens.push({ t: 'str', v: value, start: i, end: j + 1 })
      i = j + 1
      continue
    }
    const two = text.slice(i, i + 2)
    if (two === '!=' || two === '>=' || two === '<=' || two === '==') { tokens.push({ t: 'sym', v: two === '==' ? '=' : two, start: i, end: i + 2 }); i += 2; continue }
    if (c === '=' || c === '>' || c === '<') { tokens.push({ t: 'sym', v: c, start: i, end: i + 1 }); i += 1; continue }
    let j = i
    while (j < text.length && !/[\s()=!<>,"']/.test(text[j])) j += 1
    if (j === i) throw new Failure(`Unexpected “${c}”`, i)
    const word = text.slice(i, j)
    const number = /^(-?\d+(?:\.\d+)?)(us|ms|s|m)?$/.exec(word)
    tokens.push(number ? { t: 'num', v: word, n: Number(number[1]), unit: number[2] ?? '', start: i, end: j } : { t: 'word', v: word, start: i, end: j })
    i = j
  }
  return tokens
}

const isWord = (token: Token | undefined, word: string): boolean => token?.t === 'word' && token.v.toLowerCase() === word

const OPERATORS: Record<string, Op> = { '=': 'eq', '!=': 'ne', '>': 'gt', '>=': 'gte', '<': 'lt', '<=': 'lte' }

class Parser {
  private at = 0
  private clauses = 0
  private readonly tokens: Token[]
  private readonly size: number

  constructor(tokens: Token[], size: number) {
    this.tokens = tokens
    this.size = size
  }

  parse(): Node | null {
    if (this.tokens.length === 0) return null
    const node = this.or(1)
    const extra = this.tokens[this.at]
    if (extra) throw new Failure(extra.t === 'sym' && extra.v === ')' ? 'A closing parenthesis has no opening one' : `Expected AND or OR before “${this.source(extra)}”`, extra.start)
    return node
  }

  private peek(): Token | undefined {
    return this.tokens[this.at]
  }

  private source(token: Token): string {
    return token.t === 'str' ? `"${token.v}"` : token.v
  }

  private here(): number {
    return this.peek()?.start ?? this.size
  }

  private or(depth: number): Node {
    this.guard(depth)
    const parts = [this.and(depth)]
    while (isWord(this.peek(), 'or')) { this.at += 1; parts.push(this.and(depth)) }
    return parts.length === 1 ? parts[0] : { any: parts }
  }

  private and(depth: number): Node {
    const parts = [this.not(depth)]
    while (isWord(this.peek(), 'and')) { this.at += 1; parts.push(this.not(depth)) }
    return parts.length === 1 ? parts[0] : { all: parts }
  }

  private not(depth: number): Node {
    if (isWord(this.peek(), 'not')) {
      this.at += 1
      this.guard(depth + 1)
      return { not: this.not(depth + 1) }
    }
    return this.atom(depth)
  }

  private atom(depth: number): Node {
    const token = this.peek()
    if (token?.t === 'sym' && token.v === '(') {
      this.at += 1
      const inner = this.or(depth + 1)
      const close = this.peek()
      if (!(close?.t === 'sym' && close.v === ')')) throw new Failure('A parenthesis is not closed', token.start)
      this.at += 1
      return inner
    }
    return this.clause()
  }

  private guard(depth: number): void {
    if (depth > MAX_DEPTH) throw new Failure(`A query nests at most ${MAX_DEPTH} levels`, this.here())
  }

  private clause(): Node {
    const start = this.here()
    const head = this.peek()
    if (!head) throw new Failure('Expected a field', this.size)
    if (head.t === 'sym' || head.t === 'num') throw new Failure('Expected a field', head.start)
    if (head.t === 'word' && KEYWORDS.has(head.v.toLowerCase())) throw new Failure(`“${head.v}” is a keyword; quote it to use it as an attribute`, head.start)
    this.at += 1
    const field: FieldRef = head.t === 'word' && BUILT_NAMES.has(head.v.toLowerCase()) ? { built: head.v.toLowerCase() as BuiltIn } : { attr: head.v }
    if ('attr' in field && field.attr.length > 128) throw new Failure('An attribute key is at most 128 characters', head.start)

    const chip = this.rest(field, start)
    this.clauses += 1
    if (this.clauses > MAX_CLAUSES) throw new Failure(`A query holds at most ${MAX_CLAUSES} conditions`, start)
    return chip
  }

  private rest(field: FieldRef, start: number): Chip {
    const token = this.peek()
    if (!token) throw new Failure('Expected an operator', this.size)
    let negated = false
    let word = token.t === 'word' ? token.v.toLowerCase() : ''
    if (word === 'not') {
      negated = true
      this.at += 1
      word = this.peek()?.t === 'word' ? (this.peek() as Token).v.toLowerCase() : ''
      if (!['in', 'contains', 'exists'].includes(word)) throw new Failure('Expected IN, CONTAINS or EXISTS after NOT', this.here())
    }
    const current = this.peek() as Token
    if (current.t === 'sym' && OPERATORS[current.v]) {
      this.at += 1
      return this.checked(field, OPERATORS[current.v], this.value(field), start)
    }
    if (word === 'exists') {
      this.at += 1
      return this.checked(field, negated ? 'not_exists' : 'exists', undefined, start)
    }
    if (word === 'contains') {
      this.at += 1
      return this.checked(field, negated ? 'not_contains' : 'contains', this.value(field), start)
    }
    if (word === 'in') {
      this.at += 1
      const open = this.peek()
      if (!(open?.t === 'sym' && open.v === '(')) throw new Failure('Expected a list in parentheses after IN', this.here())
      this.at += 1
      const values: Scalar[] = [this.value(field)]
      while (this.peek()?.t === 'sym' && (this.peek() as Token).v === ',') { this.at += 1; values.push(this.value(field)) }
      const close = this.peek()
      if (!(close?.t === 'sym' && close.v === ')')) throw new Failure('A list is not closed', open.start)
      this.at += 1
      return this.checked(field, negated ? 'not_in' : 'in', values, start)
    }
    throw new Failure(`Expected an operator after “${labelOf(field)}”`, current.start)
  }

  private value(field: FieldRef): Scalar {
    const token = this.peek()
    if (!token || (token.t === 'sym' && token.v !== '-')) throw new Failure('Expected a value', this.here())
    this.at += 1
    if (token.t === 'str') return token.v
    if (token.t === 'num') {
      const duration = 'built' in field && field.built === 'duration'
      if (token.unit && !duration) throw new Failure(`“${token.v}” has a unit but ${labelOf(field)} is not a duration`, token.start)
      if (duration) return token.n * (token.unit ? UNITS[token.unit] : 1)
      const numberKind = kindOf(field, true) === 'number'
      return numberKind || 'attr' in field ? token.n : token.v
    }
    if (token.t === 'word') {
      const lower = token.v.toLowerCase()
      if (KEYWORDS.has(lower)) throw new Failure(`“${token.v}” is a keyword; quote it to use it as a value`, token.start)
      if (lower === 'true' && 'attr' in field) return true
      if (lower === 'false' && 'attr' in field) return false
      return token.v
    }
    throw new Failure('Expected a value', token.start)
  }

  private checked(field: FieldRef, op: Op, value: Chip['value'], start: number): Chip {
    const numeric = Array.isArray(value) ? value.every((one) => typeof one === 'number') : typeof value === 'number'
    const allowed = opsFor(field, 'attr' in field ? numeric || ['gt', 'gte', 'lt', 'lte'].includes(op) : false)
    if (!allowed.includes(op) && !('attr' in field)) throw new Failure(`${labelOf(field)} does not take “${opWord(op)}”`, start)
    const scalars = value === undefined ? [] : Array.isArray(value) ? value : [value]
    if (['gt', 'gte', 'lt', 'lte'].includes(op) && scalars.some((one) => typeof one !== 'number')) throw new Failure(`“${opWord(op)}” compares numbers`, start)
    if (['contains', 'not_contains'].includes(op) && scalars.some((one) => typeof one !== 'string')) throw new Failure(`“${opWord(op)}” takes text`, start)
    if ('built' in field) this.checkBuiltIn(field.built, scalars, start)
    return value === undefined ? { field, op } : { field, op, value }
  }

  private checkBuiltIn(name: BuiltIn, scalars: Scalar[], start: number): void {
    const known = CHOICES[name]
    if (known) {
      const wrong = scalars.find((one) => !known.includes(String(one)))
      if (wrong !== undefined) throw new Failure(`${name} is one of ${known.join(', ')}`, start)
    }
    if (name === 'trace_id' || name === 'span_id') {
      const length = name === 'trace_id' ? 32 : 16
      if (scalars.some((one) => typeof one !== 'string' || !new RegExp(`^[0-9a-fA-F]{${length}}$`).test(one))) throw new Failure(`${name} is ${length} hexadecimal digits`, start)
    }
    const numberKind = builtKind(name) === 'number'
    if (numberKind && scalars.some((one) => typeof one !== 'number')) throw new Failure(`${name} is a number`, start)
    if (!numberKind && scalars.some((one) => typeof one !== 'string')) throw new Failure(`${name} is text`, start)
  }
}

const builtKind = (name: BuiltIn) => BUILT_INS.find((field) => field.name === name)!.kind

const labelOf = (field: FieldRef) => ('built' in field ? field.built : field.attr)

const OP_WORDS: Record<Op, string> = {
  eq: '=', ne: '!=', gt: '>', gte: '>=', lt: '<', lte: '<=',
  contains: 'CONTAINS', not_contains: 'NOT CONTAINS', in: 'IN', not_in: 'NOT IN', exists: 'EXISTS', not_exists: 'NOT EXISTS',
}

const opWord = (op: Op) => OP_WORDS[op]

export function parse(text: string): Parsed {
  try {
    const node = new Parser(tokenize(text), text.length).parse()
    return { ok: true, items: node === null ? [] : conjuncts(node) }
  } catch (error) {
    if (error instanceof Failure) return { ok: false, error: error.message, at: error.at }
    throw error
  }
}

export function conjuncts(node: Node): Node[] {
  return 'all' in node ? node.all : [node]
}

const BARE = /^[A-Za-z_][A-Za-z0-9_.\-]*$/

function field(ref: FieldRef): string {
  if ('built' in ref) return ref.built
  const lower = ref.attr.toLowerCase()
  return BARE.test(ref.attr) && !KEYWORDS.has(lower) && !BUILT_NAMES.has(lower) ? ref.attr : quote(ref.attr)
}

function quote(text: string): string {
  return `"${text.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`
}

function scalar(value: Scalar, ref: FieldRef): string {
  if (typeof value === 'number') return 'built' in ref && ref.built === 'duration' ? `${trim(value)}ms` : String(value)
  if (typeof value === 'boolean') return String(value)
  const plain = /^[A-Za-z0-9_.\-:/]+$/.test(value) && !KEYWORDS.has(value.toLowerCase()) && !/^-?\d/.test(value) && !['true', 'false'].includes(value.toLowerCase())
  return plain ? value : quote(value)
}

const trim = (n: number) => String(Number(n.toFixed(3)))

export function clauseText(chip: Chip): string {
  const head = `${field(chip.field)} ${OP_WORDS[chip.op]}`
  if (chip.value === undefined) return head
  if (Array.isArray(chip.value)) return `${head} (${chip.value.map((one) => scalar(one, chip.field)).join(', ')})`
  return `${head} ${scalar(chip.value, chip.field)}`
}

export function nodeText(node: Node, parent: 'top' | 'and' | 'or' | 'not' = 'top'): string {
  if (isChip(node)) return clauseText(node)
  if ('not' in node) return `NOT ${nodeText(node.not, 'not')}`
  const isAll = 'all' in node
  const parts = (isAll ? node.all : node.any).map((part) => nodeText(part, isAll ? 'and' : 'or'))
  const text = parts.join(isAll ? ' AND ' : ' OR ')
  return (parent === 'not' || (isAll && parent === 'or') || (!isAll && parent === 'and')) && parts.length > 1 ? `(${text})` : parent === 'not' ? `(${text})` : text
}

export function itemsText(items: Node[]): string {
  return items.map((item) => (!isChip(item) && 'any' in item && items.length > 1 ? `(${nodeText(item)})` : nodeText(item))).join(' AND ')
}

export function normalise(text: string): string {
  const parsed = parse(text)
  return parsed.ok ? itemsText(parsed.items) : text.trim()
}

export function countClauses(items: Node[]): number {
  return items.reduce<number>((sum, item) => sum + (isChip(item) ? 1 : 'not' in item ? countClauses([item.not]) : countClauses('all' in item ? item.all : item.any)), 0)
}

export type Suggest =
  | { kind: 'field'; prefix: string; from: number }
  | { kind: 'op'; field: FieldRef; prefix: string; from: number }
  | { kind: 'value'; field: FieldRef; prefix: string; from: number; negated: boolean }
  | { kind: 'logic'; prefix: string; from: number }
  | { kind: 'none' }

export function suggestAt(text: string, cursor: number): Suggest {
  const before = text.slice(0, cursor)
  let tokens: Token[]
  try {
    tokens = tokenize(before)
  } catch {
    return { kind: 'none' }
  }
  const last = tokens[tokens.length - 1]
  const typing = last !== undefined && (last.t === 'word' || last.t === 'num') && last.end === before.length
  const prefix = typing ? last.v : ''
  const from = typing ? last.start : before.length
  const settled = typing ? tokens.slice(0, -1) : tokens

  let state: 'start' | 'op' | 'value' | 'list' | 'logic' = 'start'
  let current: FieldRef | null = null
  let negated = false
  for (const token of settled) {
    const word = token.t === 'word' ? token.v.toLowerCase() : ''
    if (state === 'start') {
      if (token.t === 'sym' && token.v === '(') continue
      if (word === 'not') continue
      current = token.t === 'word' && BUILT_NAMES.has(word) ? { built: word as BuiltIn } : { attr: token.v }
      negated = false
      state = 'op'
    } else if (state === 'op') {
      if (word === 'not') { negated = true; continue }
      if (word === 'exists') state = 'logic'
      else if (word === 'in') state = 'list'
      else state = 'value'
    } else if (state === 'value') {
      state = 'logic'
    } else if (state === 'list') {
      if (token.t === 'sym' && token.v === ')') state = 'logic'
    } else if (state === 'logic') {
      if (word === 'and' || word === 'or') state = 'start'
    }
  }
  if (state === 'start') return { kind: 'field', prefix, from }
  if (state === 'op' && current) return { kind: 'op', field: current, prefix, from }
  if ((state === 'value' || state === 'list') && current) return { kind: 'value', field: current, prefix, from, negated }
  if (state === 'logic') return { kind: 'logic', prefix, from }
  return { kind: 'none' }
}

const wireOf = (node: Node): unknown => {
  if (isChip(node)) return toWire(node)
  if ('not' in node) return { not: wireOf(node.not) }
  return 'all' in node ? { all: node.all.map(wireOf) } : { any: node.any.map(wireOf) }
}

export function bodyOf(items: Node[]): unknown | undefined {
  if (items.length === 0) return undefined
  return items.length === 1 ? wireOf(items[0]) : { all: items.map(wireOf) }
}

export const valueText = (value: Scalar, field: FieldRef): string => scalar(value, field)

export const opText = (op: Op): string => OP_WORDS[op]

export function queryTextOf(raw: string | undefined): string {
  if (!raw) return ''
  return raw.trimStart().startsWith('[') ? itemsText(decode(raw)) : raw
}
