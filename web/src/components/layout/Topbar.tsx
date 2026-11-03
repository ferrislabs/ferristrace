import { useIsFetching, useQueryClient } from '@tanstack/react-query'
import { ChevronDown, Clock, Folder, RefreshCw, Search } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/misc'
import { OrganisationSwitcher } from './OrganisationSwitcher'
import { PagesButton } from './Pages'
import { useProjects } from '@/data/projects'
import { Link } from '@/lib/router'
import { useScope } from '@/lib/scope'
import { WINDOWS, useWindow } from '@/lib/window'
import { cn } from '@/lib/utils'
import { Logo } from '@/components/ft/Logo'
import { MockData } from '@/components/ft/MockData'

const pad = (n: number) => String(n).padStart(2, '0')
const clock = (at: Date) => `${pad(at.getHours())}:${pad(at.getMinutes())}:${pad(at.getSeconds())}`

function WindowPicker() {
  const window = useWindow()
  const [open, setOpen] = useState(false)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button size="sm" aria-label={`Time window: last ${window.label}`} className="shrink-0 max-md:h-9 max-md:gap-1 max-md:border-transparent max-md:bg-transparent max-md:px-1.5 max-md:shadow-none"><Clock /><span className="md:hidden">{window.short}</span><span className="max-md:hidden">Last {window.short}</span><span className="hidden font-mono text-[11px] text-faint-foreground lg:inline">→ now</span><ChevronDown className="text-muted-foreground max-md:hidden" /></Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[200px] p-1">
        <div role="listbox" aria-label="Time window">
          {WINDOWS.map((w) => (
            <button key={w.key} type="button" role="option" aria-selected={w.key === window.key} onClick={() => { window.choose(w.key); setOpen(false) }} className="flex h-[30px] w-full cursor-pointer items-center rounded-sm px-2 text-left text-control hover:bg-muted max-md:h-11">
              Last {w.label}{w.key === window.key ? <span className="ml-auto text-accent-text" aria-hidden>✓</span> : null}
            </button>
          ))}
        </div>
      </PopoverContent>
    </Popover>
  )
}

function RefreshButton() {
  const client = useQueryClient()
  const fetching = useIsFetching() > 0
  const [updated, setUpdated] = useState<Date | null>(null)
  useEffect(() => {
    if (!fetching) setUpdated(new Date())
  }, [fetching])
  const label = updated ? `Refresh, last updated at ${clock(updated)}` : 'Refresh'
  return (
    <>
      {updated ? <span className="hidden font-mono text-[11px] text-faint-foreground tabular xl:inline" aria-hidden>{clock(updated)}</span> : null}
      <Button variant="ghost" size="icon-sm" aria-label={label} title={label} aria-busy={fetching || undefined} className="max-md:hidden" onClick={() => void client.invalidateQueries({ refetchType: 'active' })}>
        <RefreshCw className={cn(fetching && 'animate-spin')} />
      </Button>
    </>
  )
}

export function Topbar({ onPalette, mock, timed }: { onPalette: () => void; mock: boolean; timed: boolean }) {
  const projects = useProjects()
  const { project, chooseProject } = useScope()
  const named = projects.find((p) => p.id === project)?.name ?? 'All projects'
  return (
    <header className="sticky top-0 z-20 flex h-12 shrink-0 items-center gap-1.5 border-b bg-background/95 pl-[max(16px,env(safe-area-inset-left))] pr-[max(12px,env(safe-area-inset-right))] backdrop-blur md:gap-2 md:px-4">
      <Link to="/" className="text-foreground md:hidden" aria-label="FerrisTrace home"><Logo className="size-7" /></Link>
      <span className="hidden text-caption text-muted-foreground sm:inline">Organisation</span>
      <OrganisationSwitcher />
      <span className="hidden text-faint-foreground sm:inline" aria-hidden>/</span>
      <Popover>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="sm" aria-haspopup="listbox" aria-label={`Project: ${named}`} className="min-w-0 max-sm:px-1.5"><Folder /><span className="max-w-[96px] truncate sm:max-w-none">{named}</span><ChevronDown className="text-muted-foreground max-sm:hidden" /></Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-[260px] p-1">
          {[{ id: undefined, name: 'All projects' }, ...projects].map((p) => (
            <button key={p.id ?? 'all'} type="button" onClick={() => chooseProject(p.id)} className="flex h-[30px] w-full cursor-pointer items-center rounded-sm px-2 text-left text-control hover:bg-muted">
              {p.name}{p.id === project ? <span className="ml-auto text-accent-text">✓</span> : null}
            </button>
          ))}
        </PopoverContent>
      </Popover>
      {mock ? <MockData className="max-sm:hidden" /> : null}
      <span className="flex-1" />
      <Button variant="ghost" size="icon-sm" aria-label="Search" onClick={onPalette} className="size-9 lg:hidden"><Search /></Button>
      <PagesButton />
      <button type="button" onClick={onPalette} aria-label="Search" className="hidden h-8 w-[260px] cursor-pointer items-center gap-2 rounded-md border border-input bg-background px-2.5 text-control text-faint-foreground shadow-sm hover:bg-muted lg:flex">
        <Search className="size-3.5" aria-hidden /><span className="flex-1 text-left">Search or paste an id</span><Kbd>/</Kbd><Kbd>⌘</Kbd><Kbd>K</Kbd>
      </button>
      {timed ? <WindowPicker /> : null}
      <RefreshButton />
    </header>
  )
}
