import { CircleX, Copy, SlidersHorizontal } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useNavigate as useRouterNavigate } from '@tanstack/react-router'
import { Link, useNavigate, useSearchParam } from '@/lib/router'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { NativeSelect } from '@/components/ui/input'
import { Segmented } from '@/components/ui/segmented'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Views } from '@/components/ft/Views'
import { FilterBar } from '@/components/ft/FilterBar'
import { decode, encode, type Chip } from '@/lib/traceQuery'
import { Kbd, Table, Td, Th } from '@/components/ui/misc'
import { EmptyState, PageHeader, ServiceDot } from '@/components/ft/bits'
import { ANY, COUNTS, useTraces, type Sift } from '@/data/traces'
import { orderOf, type Definition, type Order } from '@/data/views'
import { isPlaced, NOWHERE, usePlacement } from '@/data/placement'
import { PlacementPicker, placedCount } from '@/components/ft/Placement'
import { Filters } from '@/components/ft/Filters'
import { because } from '@/auth/client'
import { shortId } from '@/lib/format'
import { cn } from '@/lib/utils'
import { useWindow } from '@/lib/window'

const codeTone = (code: number, errors: number) =>
  code >= 500 || errors > 0 ? 'text-destructive-text' : code >= 400 ? 'text-warning-text' : 'text-muted-foreground'

export function Traces() {
  const navigate = useNavigate()
  const goTo = useRouterNavigate()
  const { ms: windowMs, label: windowLabel } = useWindow()
  const [limit, setLimit] = useState(ANY.limit)
  const [density, setDensity] = useState<'compact' | 'comfy'>('compact')
  const [cursor, setCursor] = useState(0)
  const rawChips = useSearchParam('q')
  const chips = useMemo(() => decode(rawChips), [rawChips])
  const order = orderOf(useSearchParam('o'))
  const viewParam = useSearchParam('view')
  const viewKey = viewParam ?? (chips.length === 0 && order === 'started' ? 'recent' : undefined)
  const sift = useMemo<Sift>(() => ({ ...ANY, rangeMs: windowMs, order, limit }), [windowMs, order, limit])
  const { placement, place } = usePlacement()
  const { rows, loading, failed } = useTraces(sift, placement, chips)
  const current = useMemo<Definition>(() => ({ q: chips, o: order }), [chips, order])
  const show = (next: Partial<{ q: Chip[]; o: Order; view: string | undefined }>) => {
    const was = { q: chips, o: order, view: viewParam }
    const merged = { ...was, ...next }
    void goTo({
      to: '.' as never,
      search: ((search: Record<string, unknown>) => ({
        ...search,
        q: encode(merged.q),
        o: merged.o === 'started' ? undefined : merged.o,
        view: merged.view,
      })) as never,
      replace: true,
    })
    setCursor(0)
  }
  const setChips = (next: Chip[]) => show({ q: next })
  const select = (key: string | undefined, definition: Definition) => show({ q: definition.q, o: definition.o, view: key })
  const clear = () => { show({ q: [], o: 'started', view: undefined }); place(NOWHERE) }
  const sifting = chips.length > 0 || order !== 'started' || isPlaced(placement)

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

  return (
    <>
      <PageHeader title="Traces" meta={`Last ${windowLabel}`}>
        <Views current={current} viewKey={viewKey} onSelect={select} />
        <Popover>
          <PopoverTrigger asChild>
            <Button variant="ghost" size="sm" aria-label="Display options"><SlidersHorizontal />Display</Button>
          </PopoverTrigger>
          <PopoverContent align="end" className="flex w-[280px] flex-col gap-3 p-3">
            <div className="flex items-center justify-between gap-2 text-control">
              Row density
              <Segmented label="Row density" size="sm" value={density} onChange={setDensity} options={[{ value: 'compact', label: 'Compact' }, { value: 'comfy', label: 'Comfortable' }]} />
            </div>
            <div className="flex items-center justify-between gap-2 text-control">
              Traces read
              <NativeSelect aria-label="How many to read" value={limit} onChange={(e) => { setLimit(Number(e.target.value)); setCursor(0) }} className="h-7 w-[100px]">
                {COUNTS.map((n) => <option key={n} value={n}>{n}</option>)}
              </NativeSelect>
            </div>
            <span className="inline-flex items-center gap-1 text-caption text-muted-foreground"><Kbd>J</Kbd><Kbd>K</Kbd>move<Kbd>↵</Kbd>open</span>
          </PopoverContent>
        </Popover>
      </PageHeader>
      <Filters active={chips.length + placedCount(placement)}>
        <div className="flex flex-col gap-2" role="search">
          <FilterBar chips={chips} onChange={setChips} rangeMs={sift.rangeMs} />
          <div className="flex flex-wrap items-center gap-2">
            <NativeSelect aria-label="Order" value={order} onChange={(e) => show({ o: orderOf(e.target.value) })} className="w-[160px]">
              <option value="started">Newest first</option>
              <option value="duration">Slowest first</option>
              <option value="spans">Most spans first</option>
            </NativeSelect>
            <Button variant="ghost" size="sm" disabled={!sifting} onClick={clear}>Reset</Button>
          </div>
          <PlacementPicker placement={placement} onChange={(part) => { place(part); setCursor(0) }} windowMs={sift.rangeMs} />
        </div>
      </Filters>
      {failed ? (
        <EmptyState title="The traces could not be read">{because(failed, 'The span store did not answer')}</EmptyState>
      ) : rows.length === 0 ? (
        <EmptyState title={loading ? 'Reading the traces…' : sifting ? 'No traces match' : 'No traces in this window'} actions={sifting ? <Button size="sm" onClick={clear}>Reset filters</Button> : null}>
          {loading ? 'Asking the span store.' : sifting ? 'Try fewer filters, a wider window or no placement.' : 'Nothing arrived in the window you are looking at. Try a wider one.'}
        </EmptyState>
      ) : (
        <div className="-mx-4 overflow-x-auto border-t md:-mx-5">
          <Table aria-label="Traces">
            <thead><tr><Th className="w-[130px] max-md:w-[96px]">Started</Th><Th className="w-[150px] max-md:hidden">Service</Th><Th>Root span</Th><Th numeric className="w-[80px] max-md:w-[64px]">Code</Th><Th numeric className="w-[100px] max-md:w-[80px]">Duration</Th><Th className="w-[170px] max-md:hidden">Trace id</Th></tr></thead>
            <tbody>
              {rows.map((t, i) => (
                <tr key={t.id} onClick={() => navigate(`/traces/${t.id}`)} className={cn('cursor-pointer hover:[&>td]:bg-row-hover', i === cursor && '[&>td]:bg-accent-subtle')}>
                  <Td className={cn(density === 'comfy' && 'h-10', 'font-mono text-control max-md:h-[52px] max-md:text-caption')}>{t.start.length > 8 && t.start.includes(':') ? <><span className="md:hidden">{t.start.slice(0, 8)}</span><span className="max-md:hidden">{t.start}</span></> : t.start}</Td>
                  <Td className="max-md:hidden"><span className="inline-flex items-center gap-1.5 text-control"><ServiceDot name={t.svc} />{t.svc}</span></Td>
                  <Td className="max-md:max-w-0">
                    <span className="flex min-w-0 flex-col">
                      <Link to={`/traces/${t.id}`} className="truncate font-semibold text-foreground" onClick={(e) => e.stopPropagation()}>{t.root}</Link>
                      <span className="inline-flex items-center gap-1.5 truncate text-caption text-muted-foreground md:hidden"><ServiceDot name={t.svc} />{t.svc}</span>
                    </span>
                  </Td>
                  <Td numeric>
                    {t.code === null
                      ? t.errors
                        ? <span className="inline-flex text-destructive-text" title={`${t.errors} errors`}><CircleX className="size-3.5" aria-label={`${t.errors} errors`} /></span>
                        : <span className="text-muted-foreground">—</span>
                      : <span className={codeTone(t.code, t.errors)} title={t.errors ? `${t.errors} errors` : undefined}>{t.code}</span>}
                  </Td>
                  <Td numeric>{t.dur}</Td>
                  <Td className="max-md:hidden"><button type="button" onClick={(e) => { e.stopPropagation(); copy(t.id) }} className="inline-flex cursor-pointer items-center gap-1.5 rounded-sm font-mono text-caption text-muted-foreground hover:text-foreground" title={t.id} aria-label={`Copy trace id ${t.id}`}>{shortId(t.id)}<Copy className="size-3" aria-hidden /></button></Td>
                </tr>
              ))}
            </tbody>
          </Table>
          <div className="flex flex-wrap items-center gap-3 px-5 py-3">
            <span className="text-caption text-muted-foreground">
              {rows.length === sift.limit ? `The newest ${sift.limit} of this window.` : `${rows.length} trace${rows.length === 1 ? '' : 's'} in this window.`}
            </span>
          </div>
        </div>
      )}
    </>
  )
}
