import { Search } from 'lucide-react'
import { useEffect, useId, useMemo, useRef, useState } from 'react'
import { Input } from '@/components/ui/input'
import { useAttributeKeys, useAttributeValues } from '@/data/attributes'
import { useServiceNames } from '@/data/services'
import { opText, parse, suggestAt, valueText, normalise } from '@/lib/queryLang'
import { builtInsFor, CHOICES, kindOf, opsFor } from '@/lib/traceQuery'
import { useSettled } from '@/lib/settle'
import { useSignal } from '@/lib/signal'
import { cn } from '@/lib/utils'

type Option = { label: string; insert: string; hint?: string }

const LOGIC: Option[] = [{ label: 'AND', insert: 'AND' }, { label: 'OR', insert: 'OR' }]

export function QueryBar({ text, onCommit, rangeMs }: { text: string; onCommit: (text: string) => void; rangeMs: number }) {
  const signal = useSignal()
  const [draft, setDraft] = useState(text)
  const [cursor, setCursor] = useState(text.length)
  const [focused, setFocused] = useState(false)
  const [active, setActive] = useState(-1)
  const [dismissed, setDismissed] = useState(false)
  const input = useRef<HTMLInputElement>(null)
  const next = useRef<number | null>(null)
  const list = useId()

  useEffect(() => { setDraft(text) }, [text])
  useEffect(() => {
    if (next.current !== null && input.current) {
      input.current.setSelectionRange(next.current, next.current)
      next.current = null
    }
  })

  const parsed = useMemo(() => parse(draft), [draft])
  const hint = suggestAt(draft, cursor)
  const prefix = hint.kind === 'none' ? '' : hint.prefix
  const settledPrefix = useSettled(prefix, 150)
  const field = hint.kind === 'op' || hint.kind === 'value' ? hint.field : null
  const wantKeys = focused && hint.kind === 'field'
  const { keys } = useAttributeKeys(rangeMs, settledPrefix, wantKeys)
  const attribute = hint.kind === 'value' && field && 'attr' in field ? field.attr : undefined
  const { values } = useAttributeValues(focused ? attribute : undefined, rangeMs, settledPrefix)
  const { names } = useServiceNames(rangeMs)

  const options = useMemo<Option[]>(() => {
    const wanted = prefix.toLowerCase()
    const starts = (label: string) => label.toLowerCase().includes(wanted)
    if (hint.kind === 'field') {
      return [
        ...builtInsFor(signal).map<Option>((one) => ({ label: one.name, insert: one.name, hint: one.label })).filter((one) => starts(one.label)),
        ...keys.map<Option>((key) => ({ label: key.key, insert: key.key.includes(' ') ? `"${key.key}"` : key.key, hint: key.numeric ? 'number' : 'text' })).filter((one) => starts(one.label)),
      ].slice(0, 14)
    }
    if (hint.kind === 'op' && field) {
      return opsFor(field, 'attr' in field).map<Option>((op) => ({ label: opText(op), insert: opText(op) })).filter((one) => starts(one.label))
    }
    if (hint.kind === 'value' && field) {
      const pool = 'built' in field
        ? field.built === 'service' ? names : CHOICES[field.built] ?? []
        : values.map((one) => one.value)
      const numeric = kindOf(field, false) === 'number'
      return pool.filter(starts).slice(0, 12).map<Option>((value) => ({ label: value, insert: numeric ? value : valueText(value, field) }))
    }
    if (hint.kind === 'logic') return LOGIC.filter((one) => starts(one.label))
    return []
  }, [hint.kind, prefix, keys, values, names, field])

  const open = focused && !dismissed && options.length > 0
  useEffect(() => { setActive(-1) }, [options.length, hint.kind, prefix])

  const accept = (option: Option) => {
    if (hint.kind === 'none') return
    const before = draft.slice(0, hint.from)
    const after = draft.slice(cursor)
    const insertion = `${option.insert}${after.startsWith(' ') ? '' : ' '}`
    const value = `${before}${insertion}${after}`
    setDraft(value)
    next.current = before.length + insertion.length + (after.startsWith(' ') ? 1 : 0)
    setCursor(next.current)
    setDismissed(false)
  }

  const commit = () => {
    if (!parsed.ok) return
    const normal = normalise(draft)
    if (normal !== normalise(text)) onCommit(normal)
    else setDraft(text)
  }

  const error = !parsed.ok && draft.trim() !== '' ? parsed : null
  return (
    <div className="relative min-w-0">
      <div className={cn('flex items-center gap-2 rounded-md border bg-background px-2.5 shadow-sm focus-within:outline-2 focus-within:outline-ring', error && 'border-destructive')}>
        <Search className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
        <Input
          ref={input}
          value={draft}
          spellCheck={false}
          autoComplete="off"
          role="combobox"
          aria-expanded={open}
          aria-controls={list}
          aria-autocomplete="list"
          aria-invalid={error ? true : undefined}
          aria-activedescendant={open && active >= 0 ? `${list}-${active}` : undefined}
          aria-label="Query"
          placeholder='Query, e.g. service = checkout AND http.status_code >= 500'
          mono
          className="h-8 min-w-0 flex-1 border-0 bg-transparent px-0 shadow-none focus-visible:outline-none"
          onChange={(event) => { setDraft(event.target.value); setCursor(event.target.selectionStart ?? event.target.value.length); setDismissed(false) }}
          onSelect={(event) => setCursor((event.target as HTMLInputElement).selectionStart ?? draft.length)}
          onFocus={() => setFocused(true)}
          onBlur={() => { setFocused(false); commit() }}
          onKeyDown={(event) => {
            if (event.key === 'ArrowDown' && options.length > 0) { event.preventDefault(); setDismissed(false); setActive((was) => (was + 1) % options.length) }
            else if (event.key === 'ArrowUp' && options.length > 0) { event.preventDefault(); setActive((was) => (was <= 0 ? options.length - 1 : was - 1)) }
            else if (event.key === 'Escape') { setDismissed(true); setActive(-1) }
            else if ((event.key === 'Enter' || event.key === 'Tab') && open && active >= 0) { event.preventDefault(); accept(options[active]) }
            else if (event.key === 'Enter') { event.preventDefault(); commit() }
          }}
        />
      </div>
      {error && !error.ok ? <p role="alert" className="m-0 mt-1 text-caption text-destructive-text">{error.error}{error.at < draft.length ? ` (character ${error.at + 1})` : ''}</p> : null}
      {open ? (
        <div id={list} role="listbox" aria-label="Suggestions" className="absolute left-0 top-[38px] z-30 flex max-h-[260px] w-[min(420px,100%)] flex-col overflow-y-auto rounded-lg border bg-popover p-1 shadow-md">
          {options.map((option, index) => (
            <button
              key={`${option.label}-${index}`}
              id={`${list}-${index}`}
              type="button"
              role="option"
              aria-selected={index === active}
              tabIndex={-1}
              onMouseDown={(event) => { event.preventDefault(); accept(option) }}
              className={cn('flex h-7 shrink-0 cursor-pointer items-center justify-between gap-3 rounded-sm px-2 text-left font-mono text-[12.5px] hover:bg-muted', index === active && 'bg-muted')}
            >
              <span className="truncate">{option.label}</span>
              {option.hint ? <span className="shrink-0 font-sans text-[11px] text-faint-foreground">{option.hint}</span> : null}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  )
}
