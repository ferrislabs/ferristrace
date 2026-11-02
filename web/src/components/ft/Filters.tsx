import { ChevronDown, SlidersHorizontal } from 'lucide-react'
import { useId, useState } from 'react'
import { cn } from '@/lib/utils'

export function Filters({ active = 0, children, className }: { active?: number; children: React.ReactNode; className?: string }) {
  const [open, setOpen] = useState(false)
  const id = useId()
  return (
    <div className={cn('flex flex-col gap-2', className)}>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen((was) => !was)}
        className={cn(
          'flex min-h-11 w-full cursor-pointer items-center gap-2 rounded-lg border bg-background px-3 text-[15px] font-semibold text-foreground md:hidden',
          active > 0 && 'border-accent-text/40 bg-accent-subtle',
        )}
      >
        <SlidersHorizontal className="size-4 text-muted-foreground" aria-hidden />
        <span className="flex-1 text-left">Filters</span>
        {active > 0 ? <span className="rounded-full bg-primary px-2 py-0.5 font-mono text-[11px] text-primary-foreground">{active} on</span> : null}
        <ChevronDown className={cn('size-4 text-muted-foreground transition-transform', open && 'rotate-180')} aria-hidden />
      </button>
      <div
        id={id}
        className={cn(
          'flex-col gap-2 md:flex md:gap-2',
          open ? 'flex' : 'hidden',
          'max-md:[&_select]:w-full max-md:[&_select]:h-11 max-md:[&_select]:text-[16px] max-md:[&_input]:text-[16px] max-md:[&_[role=search]]:grid max-md:[&_[role=search]]:grid-cols-2 max-md:[&_[role=search]>*]:w-full max-md:[&_[role=search]>:is(div,form)]:col-span-2',
        )}
      >
        {children}
      </div>
    </div>
  )
}
