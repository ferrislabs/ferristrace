import { ChevronsUpDown, Folder } from 'lucide-react'
import { useOrganisations } from '@/auth/session'
import { NavLink, Outlet } from '@/lib/router'
import { NativeSelect } from '@/components/ui/input'
import { useProjects } from '@/data/projects'
import { useScope } from '@/lib/scope'
import { cn } from '@/lib/utils'

const ADMINISTERED = new Set(['audit'])

const GROUPS: { label: string; items: [string, string][] }[] = [
  { label: 'Personal', items: [['profile', 'Profile'], ['security', 'Login & security'], ['preferences', 'Preferences']] },
  { label: 'Organisation', items: [['general', 'General'], ['members', 'Members'], ['teams', 'Teams'], ['projects', 'Projects'], ['usage', 'Usage'], ['audit', 'Audit log'], ['billing', 'Billing']] },
  { label: 'Project', items: [['project/general', 'General'], ['project/alerts', 'Exception alerts'], ['project/api-keys', 'Api keys']] },
]

export function SettingsLayout() {
  const projects = useProjects()
  const { project, chooseProject } = useScope()
  const { active } = useOrganisations()
  const administers = active?.role === 'owner' || active?.role === 'admin'
  return (
    <div className="flex flex-wrap items-start gap-8">
      <nav aria-label="Settings sections" className="flex w-[200px] flex-col gap-0.5 pt-1 max-md:w-full">
        <h1 className="mb-2 text-title">Settings</h1>
        {GROUPS.map((g) => (
          <div key={g.label} className="flex flex-col gap-0.5">
            <span className="px-2.5 pb-1 pt-2.5 text-[11px] font-semibold uppercase tracking-[.05em] text-muted-foreground">{g.label}</span>
            {g.label === 'Project' ? (
              <div className="relative mb-1">
                <Folder className="pointer-events-none absolute left-2.5 top-2 size-3.5 text-muted-foreground" aria-hidden />
                <NativeSelect aria-label="Project to configure" value={projects.find((p) => p.id === project)?.id ?? projects[0]?.id ?? ''} onChange={(e) => chooseProject(e.target.value)} className="h-[30px] appearance-none pl-7 font-semibold">
                  {projects.length > 0 ? projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>) : <option value="">No project yet</option>}
                </NativeSelect>
                <ChevronsUpDown className="pointer-events-none absolute right-2 top-2 size-3.5 text-muted-foreground" aria-hidden />
              </div>
            ) : null}
            {g.items.filter(([to]) => administers || !ADMINISTERED.has(to)).map(([to, label]) => (
              <NavLink key={to} to={`/settings/${to}`} className={({ isActive }) => cn('flex h-[30px] items-center rounded-md px-2.5 text-[13.5px] font-semibold text-muted-foreground no-underline hover:bg-muted hover:text-foreground', isActive && 'bg-muted text-foreground')}>{label}</NavLink>
            ))}
          </div>
        ))}
      </nav>
      <div className="flex min-w-0 max-w-[900px] flex-[999_1_560px] flex-col gap-4">
        <Outlet />
      </div>
    </div>
  )
}
