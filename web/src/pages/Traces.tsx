import { CircleX, Copy, Search } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { Link, useNavigate } from '@/lib/router'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { InputGroup, NativeSelect } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Switch } from '@/components/ui/switch'
import { Kbd, Table, Td, Th } from '@/components/ui/misc'
import { EmptyState, PageHeader, ServiceDot } from '@/components/ft/bits'
import { ANY, COUNTS, RANGES, useTraces, type Order, type Sift } from '@/data/traces'
import { useServiceNames } from '@/data/services'
import { because } from '@/auth/client'
import { shortId } from '@/lib/format'
import { useSettled } from '@/lib/settle'
import { cn } from '@/lib/utils'

export function Traces() {
  const navigate = useNavigate()
  const [sift, setSift] = useState<Sift>(ANY)
  const [typed, setTyped] = useState('')
  const [density, setDensity] = useState<'compact' | 'comfy'>('compact')
  const [cursor, setCursor] = useState(0)
  const name = useSettled(typed)
  const asked = useMemo(() => ({ ...sift, name }), [sift, name])
  const { rows, loading, failed } = useTraces(asked)
  const { names, unreachable } = useServiceNames(sift.rangeMs)
  const change = (part: Partial<Sift>) => { setSift((was) => ({ ...was, ...part })); setCursor(0) }
  const clear = () => { setTyped(''); setSift((was) => ({ ...ANY, rangeMs: was.rangeMs, order: was.order, limit: was.limit })) }
  const sifting = Boolean(sift.service || typed || sift.errorsOnly || sift.atLeastMs)

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement
      if (el.tagName === 'INPUT' || el.tagName === 'SELECT') return
      if (e.key === 'j' || e.key === 'ArrowDown') { e.preventDefault(); setCursor((c) => Math.min(rows.length - 1, c + 1)) }
      if (e.key === 'k' || e.key === 'ArrowUp') { e.preventDefault(); setCursor((c) => Math.max(0, c - 1)) }
      if (e.key === 'Enter' && rows[cursor]) navigate(`/traces/${rows[cursor].id}`)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [rows, cursor, navigate])

  const copy = (id: string) => { void navigator.clipboard?.writeText(id); toast.success('Trace id copied', { description: shortId(id) }) }
  const range = RANGES.find(([, ms]) => ms === sift.rangeMs)?.[0] ?? 'a window'

  return (
    <>
      <PageHeader title="Traces" meta={sift.order === 'started' ? `Newest first · last ${range}` : `Last ${range}`}>
        <Segmented label="Row density" value={density} onChange={setDensity} options={[{ value: 'compact', label: 'Compact' }, { value: 'comfy', label: 'Comfortable' }]} />
        <span className="hidden items-center gap-1 text-caption text-muted-foreground lg:inline-flex"><Kbd>J</Kbd><Kbd>K</Kbd>move<Kbd>↵</Kbd>open</span>
      </PageHeader>
      <div className="flex flex-wrap items-center gap-2" role="search">
        <NativeSelect aria-label="Time range" value={sift.rangeMs} onChange={(e) => change({ rangeMs: Number(e.target.value) })} className="w-[150px]">
          {RANGES.map(([label, ms]) => <option key={ms} value={ms}>Last {label}</option>)}
        </NativeSelect>
        <NativeSelect aria-label="Service" value={sift.service} onChange={(e) => change({ service: e.target.value })} className="w-[190px]">
          <option value="">Service: any</option>
          {names.map((s) => <option key={s} value={s}>{s}</option>)}
        </NativeSelect>
        <InputGroup className="w-[280px] max-w-full"><Search aria-hidden /><input aria-label="Span name" placeholder="Span name, e.g. psp.charge" value={typed} onChange={(e) => { setTyped(e.target.value); setCursor(0) }} /></InputGroup>
        <NativeSelect aria-label="Minimum duration" value={sift.atLeastMs} onChange={(e) => change({ atLeastMs: Number(e.target.value) })} className="w-[160px]">
          <option value={0}>Any duration</option>
          {[100, 500, 1000, 5000].map((ms) => <option key={ms} value={ms}>Slower than {ms < 1000 ? `${ms} ms` : `${ms / 1000} s`}</option>)}
        </NativeSelect>
        <NativeSelect aria-label="Order" value={sift.order} onChange={(e) => change({ order: e.target.value as Order })} className="w-[160px]">
          <option value="started">Newest first</option>
          <option value="duration">Slowest first</option>
          <option value="spans">Most spans first</option>
        </NativeSelect>
        <Switch checked={sift.errorsOnly} onCheckedChange={(on) => change({ errorsOnly: on })} label="Errors only" />
        <Button variant="ghost" size="sm" disabled={!sifting} onClick={clear}>Clear</Button>
      </div>
      {unreachable ? <p className="m-0 text-caption text-muted-foreground">{because(unreachable, 'The services could not be listed, so that filter is empty')}</p> : null}
      {failed ? (
        <EmptyState title="The traces could not be read">{because(failed, 'The span store did not answer')}</EmptyState>
      ) : rows.length === 0 ? (
        <EmptyState title={loading ? 'Reading the traces…' : sifting ? 'No traces match' : 'No traces in this window'} actions={sifting ? <Button size="sm" onClick={clear}>Clear filters</Button> : null}>
          {loading ? 'Asking the span store.' : sifting ? 'Try a shorter span name, a wider window, or no service.' : 'Nothing arrived in the window you are looking at. Try a wider one.'}
        </EmptyState>
      ) : (
        <div className="-mx-4 overflow-x-auto border-t md:-mx-5">
          <Table aria-label="Traces">
            <thead><tr><Th className="w-[44px]"><span className="sr-only">Status</span></Th><Th>Root span</Th><Th className="w-[150px]">Service</Th><Th className="w-[130px]">Started</Th><Th numeric className="w-[100px]">Duration</Th><Th numeric className="w-[80px]">Spans</Th><Th className="w-[170px]">Trace id</Th></tr></thead>
            <tbody>
              {rows.map((t, i) => (
                <tr key={t.id} onClick={() => navigate(`/traces/${t.id}`)} className={cn('cursor-pointer hover:[&>td]:bg-row-hover', i === cursor && '[&>td]:bg-accent-subtle')}>
                  <Td className={cn(density === 'comfy' && 'h-10')}>{t.errors ? <span className="inline-flex text-destructive-text" title={`${t.errors} errors`}><CircleX className="size-3.5" aria-label={`${t.errors} errors`} /></span> : null}</Td>
                  <Td><Link to={`/traces/${t.id}`} className="font-semibold text-foreground" onClick={(e) => e.stopPropagation()}>{t.root}</Link></Td>
                  <Td><span className="inline-flex items-center gap-1.5 text-control"><ServiceDot name={t.svc} />{t.svc}</span></Td>
                  <Td className="font-mono text-control">{t.start}</Td>
                  <Td numeric>{t.dur}</Td>
                  <Td numeric>{t.spans}</Td>
                  <Td><button type="button" onClick={(e) => { e.stopPropagation(); copy(t.id) }} className="inline-flex cursor-pointer items-center gap-1.5 rounded-sm font-mono text-caption text-muted-foreground hover:text-foreground" title={t.id} aria-label={`Copy trace id ${t.id}`}>{shortId(t.id)}<Copy className="size-3" aria-hidden /></button></Td>
                </tr>
              ))}
            </tbody>
          </Table>
          <div className="flex flex-wrap items-center gap-3 px-5 py-3">
            <span className="text-caption text-muted-foreground">
              {rows.length === sift.limit ? `The newest ${sift.limit} of this window.` : `${rows.length} trace${rows.length === 1 ? '' : 's'} in this window.`}
            </span>
            <NativeSelect aria-label="How many to read" value={sift.limit} onChange={(e) => change({ limit: Number(e.target.value) })} className="h-7 w-[130px]">
              {COUNTS.map((n) => <option key={n} value={n}>Read {n}</option>)}
            </NativeSelect>
          </div>
        </div>
      )}
    </>
  )
}
