import * as D from '@radix-ui/react-dialog'
import { Command } from 'cmdk'
import { Moon, Search, Sun, Waypoints } from 'lucide-react'
import { ALL_NAV } from './nav'
import { useServices } from '@/data/services'
import { useTraces } from '@/data/traces'
import { Kbd } from '@/components/ui/misc'
import { DialogContent, Dialog } from '@/components/ui/dialog'
import { useTheme } from '@/lib/theme'
import { useNavigate } from '@/lib/router'

const item = 'flex h-8 cursor-pointer items-center gap-2 rounded-sm px-2 text-control data-[selected=true]:bg-muted [&_svg]:size-4 [&_svg]:text-muted-foreground'
const heading = '[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-[.05em] [&_[cmdk-group-heading]]:text-muted-foreground'

export function CommandPalette({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const navigate = useNavigate()
  const { setPref, dark } = useTheme()
  const services = useServices()
  const { rows: traces } = useTraces()
  const go = (to: string) => { onOpenChange(false); navigate(to) }
  return (
    <D.Root open={open} onOpenChange={onOpenChange}>
      <D.Portal>
        <D.Overlay className="fixed inset-0 z-40 bg-overlay" />
        <D.Content className="fixed left-1/2 top-[12vh] z-50 w-[calc(100%-32px)] max-w-[640px] -translate-x-1/2 overflow-hidden rounded-xl border bg-popover shadow-lg focus:outline-none">
          <D.Title className="sr-only">Command palette</D.Title>
          <D.Description className="sr-only">Search pages, services and traces</D.Description>
          <Command label="Command palette" className={heading}>
            <div className="flex h-11 items-center gap-2 border-b px-3">
              <Search className="size-4 text-muted-foreground" aria-hidden />
              <Command.Input autoFocus placeholder="Go to a page, a service, or paste a trace id" className="h-full flex-1 bg-transparent text-body outline-none placeholder:text-faint-foreground" />
              <Kbd>Esc</Kbd>
            </div>
            <Command.List className="max-h-80 overflow-y-auto p-1">
              <Command.Empty className="py-6 text-center text-control text-muted-foreground">No match. Paste a 32-character trace id to open it.</Command.Empty>
              <Command.Group heading="Pages">
                {ALL_NAV.map((n) => (
                  <Command.Item key={n.to} value={`page ${n.label}`} onSelect={() => go(n.to)} className={item}>
                    <n.icon aria-hidden />{n.label}
                    {n.keys ? <span className="ml-auto flex gap-0.5"><Kbd>G</Kbd><Kbd>{n.keys}</Kbd></span> : null}
                  </Command.Item>
                ))}
                <Command.Item value="page settings" onSelect={() => go('/settings')} className={item}>Settings<span className="ml-auto flex gap-0.5"><Kbd>G</Kbd><Kbd>S</Kbd></span></Command.Item>
              </Command.Group>
              <Command.Group heading="Services">
                {services.map((s) => (
                  <Command.Item key={s.name} value={`service ${s.name}`} onSelect={() => go(`/services/${s.name}`)} className={item}>
                    <span className="font-mono">{s.name}</span><span className="ml-auto text-caption text-muted-foreground">{s.rps}</span>
                  </Command.Item>
                ))}
              </Command.Group>
              <Command.Group heading="Recent traces">
                {traces.slice(0, 5).map((t) => (
                  <Command.Item key={t.id} value={`trace ${t.id} ${t.root}`} onSelect={() => go(`/traces/${t.id}`)} className={item}>
                    <Waypoints aria-hidden />{t.root}<span className="ml-auto font-mono text-caption text-muted-foreground">{t.id.slice(0, 8)}…</span>
                  </Command.Item>
                ))}
              </Command.Group>
              <Command.Group heading="Actions">
                <Command.Item value="toggle theme dark light" onSelect={() => { setPref(dark ? 'light' : 'dark'); onOpenChange(false) }} className={item}>
                  {dark ? <Sun aria-hidden /> : <Moon aria-hidden />}Switch to {dark ? 'light' : 'dark'} theme
                </Command.Item>
              </Command.Group>
            </Command.List>
          </Command>
        </D.Content>
      </D.Portal>
    </D.Root>
  )
}

const SHORTCUTS: [string[], string][] = [
  [['/'], 'Search, or paste a trace id'], [['⌘', 'K'], 'The same search'], [['G', 'O'], 'Overview'], [['G', 'V'], 'Services'], [['G', 'I'], 'Issues'], [['G', 'R'], 'Releases'], [['G', 'C'], 'AI spend'], [['G', 'A'], 'Alerts'], [['G', 'T'], 'Traces'],
  [['G', 'L'], 'Logs'], [['G', 'M'], 'Metrics'], [['G', 'P'], 'Status page'], [['G', 'S'], 'Settings'], [['J', 'K'], 'Move in lists'], [['↵'], 'Open the selected row'], [['?'], 'This list'],
]
export function ShortcutsDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent title="Keyboard shortcuts" size="md">
        <dl className="m-0 grid grid-cols-2 gap-x-6 gap-y-2">
          {SHORTCUTS.map(([k, d]) => (
            <div key={d} className="flex items-center gap-2 text-control">
              <dt className="flex gap-0.5">{k.map((x) => <Kbd key={x}>{x}</Kbd>)}</dt>
              <dd className="m-0 text-muted-foreground">{d}</dd>
            </div>
          ))}
        </dl>
      </DialogContent>
    </Dialog>
  )
}
