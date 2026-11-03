import { useQuery } from '@tanstack/react-query'
import { TriangleAlert } from 'lucide-react'
import { useMatches } from '@tanstack/react-router'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { Sidebar } from './Sidebar'
import { Topbar } from './Topbar'
import { MobileTabs } from './MobileTabs'
import { useGlobalShortcuts } from './useGlobalShortcuts'
import { CommandPalette, ShortcutsDialog } from './CommandPalette'
import { healthQuery } from '@/auth/api'
import { useOrganisations } from '@/auth/session'
import { NewOrganisationForm } from './NewOrganisation'
import { Outlet } from '@/lib/router'

function UnavailableBanner() {
  const health = useQuery(healthQuery)
  if (!health.isError) return null
  return (
    <div role="status" className="flex items-center gap-2.5 border-b border-warning-border bg-warning-subtle px-4 py-2 text-control text-warning-text">
      <TriangleAlert className="size-4 shrink-0" aria-hidden />
      <span className="flex-1"><span className="font-semibold">Backend unavailable.</span> The health check is failing, so anything on screen may be stale.</span>
      <Button variant="ghost" size="sm" className="text-inherit" loading={health.isFetching} onClick={() => void health.refetch()}>Retry now</Button>
    </div>
  )
}

function NoOrganisation() {
  const { list, asking } = useOrganisations()
  if (asking || list.length > 0) return null
  return (
    <Card className="flex max-w-[480px] flex-col gap-3 p-4">
      <div className="flex flex-col gap-1">
        <h2 className="text-heading">You belong to no organisation yet</h2>
        <span className="text-control text-muted-foreground">Create one to start sending data, or ask an operator to grant you a role in an existing one.</span>
      </div>
      <NewOrganisationForm />
    </Card>
  )
}

export function AppShell() {
  const matches = useMatches()
  const mock = matches.some((m) => m.staticData.mock === true)
  const timed = matches.some((m) => m.staticData.timed === true)
  const { palette, setPalette, shortcuts, setShortcuts } = useGlobalShortcuts()
  return (
    <div className="flex min-h-screen bg-background">
      <Sidebar onOpenShortcuts={() => setShortcuts(true)} />
      <div className="flex min-w-0 flex-1 flex-col">
        <Topbar onPalette={() => setPalette(true)} mock={mock} timed={timed} />
        <UnavailableBanner />
        <main className="flex min-w-0 flex-1 flex-col gap-4 px-[max(16px,env(safe-area-inset-left))] pb-[calc(84px+env(safe-area-inset-bottom))] pt-4 md:px-5 md:pb-10">
          <NoOrganisation />
          <Outlet />
        </main>
        <MobileTabs />
      </div>
      <CommandPalette open={palette} onOpenChange={setPalette} />
      <ShortcutsDialog open={shortcuts} onOpenChange={setShortcuts} />
    </div>
  )
}
