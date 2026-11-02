import { ChevronDown, Clock, Folder, RefreshCw, Search } from 'lucide-react'
import { useState } from 'react'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/misc'
import { OrganisationSwitcher } from './OrganisationSwitcher'
import { useProjects } from '@/data/projects'
import { Link } from '@/lib/router'
import { useScope } from '@/lib/scope'
import { Logo } from '@/components/ft/Logo'
import { MockData } from '@/components/ft/MockData'

const RANGES = ['Last 15 m', 'Last 1 h', 'Last 24 h', 'Last 7 d'] as const

export function Topbar({ onPalette, mock }: { onPalette: () => void; mock: boolean }) {
  const projects = useProjects()
  const { project, chooseProject } = useScope()
  const named = projects.find((p) => p.id === project)?.name ?? 'All projects'
  const [range, setRange] = useState<string>('Last 1 h')
  return (
    <header className="sticky top-0 z-20 flex h-12 shrink-0 items-center gap-2 border-b bg-background px-4">
      <Link to="/" className="text-foreground md:hidden" aria-label="Ferristrace home"><Logo className="size-5" /></Link>
      <span className="hidden text-caption text-muted-foreground sm:inline">Organisation</span>
      <OrganisationSwitcher />
      <span className="hidden text-faint-foreground sm:inline" aria-hidden>/</span>
      <Popover>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="sm" aria-haspopup="listbox"><Folder />{named}<ChevronDown className="text-muted-foreground" /></Button>
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
      <button type="button" onClick={onPalette} aria-label="Search" className="hidden h-8 w-[260px] cursor-pointer items-center gap-2 rounded-md border border-input bg-background px-2.5 text-control text-faint-foreground shadow-sm hover:bg-muted lg:flex">
        <Search className="size-3.5" aria-hidden /><span className="flex-1 text-left">Search or paste an id</span><Kbd>/</Kbd><Kbd>⌘</Kbd><Kbd>K</Kbd>
      </button>
      <Popover>
        <PopoverTrigger asChild>
          <Button size="sm"><Clock />{range}<span className="hidden font-mono text-[11px] text-faint-foreground sm:inline">→ now</span><ChevronDown className="text-muted-foreground" /></Button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-[200px] p-1">
          {RANGES.map((r, i) => (
            <button key={r} type="button" onClick={() => setRange(r)} className="flex h-[30px] w-full cursor-pointer items-center rounded-sm px-2 text-left text-control hover:bg-muted">
              {r}<span className="ml-auto">{r === range ? <span className="text-accent-text">✓</span> : <Kbd>{i + 1}</Kbd>}</span>
            </button>
          ))}
        </PopoverContent>
      </Popover>
      <Button variant="ghost" size="icon-sm" aria-label="Refresh"><RefreshCw /></Button>
    </header>
  )
}
