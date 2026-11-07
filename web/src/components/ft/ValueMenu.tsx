import { Ban, Filter } from 'lucide-react'
import { useState, type ReactNode } from 'react'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import type { FieldRef } from '@/lib/traceQuery'
import { cn } from '@/lib/utils'

export type OnFilter = (field: FieldRef, value: string, keep: boolean) => void

const named = (field: FieldRef) => ('built' in field ? field.built.replace(/_/g, ' ') : field.attr)

export function ValueMenu({
  field,
  value,
  onFilter,
  className,
  children,
}: {
  field: FieldRef
  value: string
  onFilter: OnFilter
  className?: string
  children: ReactNode
}) {
  const [open, setOpen] = useState(false)
  const act = (keep: boolean) => {
    setOpen(false)
    onFilter(field, value, keep)
  }
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          onClick={(event) => event.stopPropagation()}
          aria-label={`Filter on ${named(field)} ${value}`}
          className={cn(
            'inline-flex max-w-full cursor-pointer items-center rounded-sm border-0 bg-transparent p-0 text-left hover:underline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring',
            className,
          )}
        >
          {children}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-[min(320px,calc(100vw-32px))] p-1" onClick={(event) => event.stopPropagation()}>
        <p className="m-0 px-2 py-1.5 text-caption text-muted-foreground">
          <span className="font-mono">{named(field)}</span>
          <span className="mx-1">·</span>
          <span className="font-mono text-foreground">{value}</span>
        </p>
        <button type="button" onClick={() => act(true)} className="flex w-full cursor-pointer items-center gap-2 rounded-sm border-0 bg-transparent px-2 py-1.5 text-left text-control hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring">
          <Filter className="size-3.5 text-muted-foreground" aria-hidden />
          Keep only this value
        </button>
        <button type="button" onClick={() => act(false)} className="flex w-full cursor-pointer items-center gap-2 rounded-sm border-0 bg-transparent px-2 py-1.5 text-left text-control hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring">
          <Ban className="size-3.5 text-muted-foreground" aria-hidden />
          Exclude this value
        </button>
      </PopoverContent>
    </Popover>
  )
}
