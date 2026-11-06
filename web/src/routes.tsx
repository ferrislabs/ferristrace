import type { QueryClient } from '@tanstack/react-query'
import { Outlet, createRootRouteWithContext, createRoute, createRouter, redirect, retainSearchParams } from '@tanstack/react-router'
import { accountQuery } from '@/auth/api'
import { isUnauthorized } from '@/auth/client'
import { reauthenticate } from '@/auth/oidc'
import { AppShell } from '@/components/layout/AppShell'
import { Ai } from '@/pages/Ai'
import { Alerts } from '@/pages/Alerts'
import { IncidentDetail } from '@/pages/IncidentDetail'
import { Incidents } from '@/pages/Incidents'
import { Kubernetes } from '@/pages/Infra'
import { KubernetesNode } from '@/pages/InfraNode'
import { IssueDetail } from '@/pages/IssueDetail'
import { Issues } from '@/pages/Issues'
import { Logs } from '@/pages/Logs'
import { Metrics } from '@/pages/Metrics'
import { NotFound } from '@/pages/NotFound'
import { Onboarding } from '@/pages/Onboarding'
import { Overview } from '@/pages/Overview'
import { PublicPostmortem, PublicStatus } from '@/pages/PublicStatus'
import { ReleaseDetail, Releases } from '@/pages/Releases'
import { ServiceDetail } from '@/pages/ServiceDetail'
import { Services } from '@/pages/Services'
import { SignalPage } from '@/pages/SignalPage'
import { StatusPageEditor } from '@/pages/StatusPageEditor'
import { TraceDetail } from '@/pages/TraceDetail'
import { TracesErrors, TracesExplore, TracesRecent, TracesSlow } from '@/pages/Traces'
import { Welcome } from '@/pages/auth/Welcome'
import { SettingsLayout } from '@/pages/settings/SettingsLayout'
import { Profile } from '@/pages/settings/Profile'
import { Preferences } from '@/pages/settings/Preferences'
import { OrgGeneral } from '@/pages/settings/OrgGeneral'
import { Agents } from '@/pages/settings/Agents'
import { Mcp } from '@/pages/settings/Mcp'
import { AuditLog } from '@/pages/settings/AuditLog'
import { LlmPrices } from '@/pages/settings/LlmPrices'
import { Members } from '@/pages/settings/Members'
import { Teams } from '@/pages/settings/Teams'
import { Projects } from '@/pages/settings/Projects'
import { Usage } from '@/pages/settings/Usage'
import { Billing } from '@/pages/settings/Billing'
import { ProjectGeneral } from '@/pages/settings/ProjectGeneral'
import { ProjectAlerts } from '@/pages/settings/ProjectAlerts'
import { ProjectApiKeys } from '@/pages/settings/ProjectApiKeys'

declare module '@tanstack/react-router' {
  interface StaticDataRouteOption {
    mock?: boolean
    timed?: boolean
  }
  interface Register {
    router: AppRouter
  }
}

type Marker = { mock?: boolean; timed?: boolean }
const mock: Marker = { mock: true }
const real: Marker = {}
const timed: Marker = { timed: true }

const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()({ component: Outlet, notFoundComponent: NotFound })

const publicStatusRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/status/$slug',
  component: PublicStatus,
  staticData: real,
  validateSearch: (search: Record<string, unknown>): { notice?: string } => ({ notice: typeof search.notice === 'string' ? search.notice : undefined }),
})

const publicPostmortemRoute = createRoute({ getParentRoute: () => rootRoute, path: '/status/$slug/postmortems/$incident', component: PublicPostmortem, staticData: real })

async function signedIn(queryClient: QueryClient, href: string) {
  try {
    return await queryClient.ensureQueryData(accountQuery)
  } catch (error) {
    if (isUnauthorized(error)) {
      await reauthenticate(href)
      return new Promise<never>(() => undefined)
    }
    return undefined
  }
}

const welcomeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/welcome',
  component: Welcome,
  beforeLoad: async ({ context, location }) => {
    const account = await signedIn(context.queryClient, location.href)
    if (account && account.memberships.length > 0) throw redirect({ to: '/' })
  },
})

const shellRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: 'shell',
  component: AppShell,
  validateSearch: (search: Record<string, unknown>): { window?: string } => ({
    window: typeof search.window === 'string' ? search.window : undefined,
  }),
  search: { middlewares: [retainSearchParams(['window'])] },
  beforeLoad: async ({ context, location }) => {
    const account = await signedIn(context.queryClient, location.href)
    if (account?.memberships.length === 0) throw redirect({ to: '/welcome' })
  },
})

const inShell = (path: string, component: () => React.ReactNode, staticData: Marker = mock) => createRoute({ getParentRoute: () => shellRoute, path, component, staticData })

const settingsRoute = createRoute({ getParentRoute: () => shellRoute, path: '/settings', component: SettingsLayout, staticData: real })

const settingsChildren = [
  createRoute({ getParentRoute: () => settingsRoute, path: '/', beforeLoad: () => { throw redirect({ to: '/settings/general' }) } }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'profile', component: Profile, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'preferences', component: Preferences, staticData: mock }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'general', component: OrgGeneral, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'members', component: Members, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'teams', component: Teams, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'projects', component: Projects, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'usage', component: Usage, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'agents', component: Agents, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'mcp', component: Mcp, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'audit', component: AuditLog, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'llm-prices', component: LlmPrices, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'billing', component: Billing, staticData: mock }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'project/general', component: ProjectGeneral, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'project/alerts', component: ProjectAlerts, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'project/api-keys', component: ProjectApiKeys, staticData: real }),
]

const routeTree = rootRoute.addChildren([
  welcomeRoute,
  publicStatusRoute,
  publicPostmortemRoute,
  shellRoute.addChildren([
    inShell('/', Overview, timed),
    inShell('/onboarding', Onboarding, real),
    inShell('/services', Services, timed),
    inShell('/services/$name', ServiceDetail, timed),
    inShell('/issues', Issues, real),
    inShell('/issues/$id', IssueDetail, real),
    inShell('/releases', Releases, real),
    inShell('/releases/$service/$version', ReleaseDetail, real),
    inShell('/ai', Ai, real),
    inShell('/alerts', Alerts, real),
    inShell('/incidents', Incidents, real),
    inShell('/incidents/$id', IncidentDetail, real),
    inShell('/infra/kubernetes', Kubernetes, real),
    inShell('/infra/kubernetes/nodes/$name', KubernetesNode, real),
    inShell('/traces', TracesRecent, timed),
    inShell('/traces/errors', TracesErrors, timed),
    inShell('/traces/slow', TracesSlow, timed),
    inShell('/traces/explore', TracesExplore, timed),
    inShell('/traces/$id', TraceDetail, {}),
    inShell('/logs', Logs, timed),
    inShell('/metrics', Metrics, timed),
    inShell('/backend/$view', () => <SignalPage group="backend" />),
    inShell('/frontend/$view', () => <SignalPage group="frontend" />),
    inShell('/status-page', StatusPageEditor, real),
    settingsRoute.addChildren(settingsChildren),
    inShell('$', NotFound, real),
  ]),
])

export function createAppRouter(queryClient: QueryClient) {
  return createRouter({ routeTree, context: { queryClient }, defaultPreload: 'intent', scrollRestoration: true })
}

export type AppRouter = ReturnType<typeof createAppRouter>
