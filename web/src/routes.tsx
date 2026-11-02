import type { QueryClient } from '@tanstack/react-query'
import { Outlet, createRootRouteWithContext, createRoute, createRouter, redirect } from '@tanstack/react-router'
import { accountQuery } from '@/auth/api'
import { isUnauthorized } from '@/auth/client'
import { AppShell } from '@/components/layout/AppShell'
import { Alerts } from '@/pages/Alerts'
import { Docker, Kubernetes } from '@/pages/Infra'
import { IssueDetail } from '@/pages/IssueDetail'
import { Issues } from '@/pages/Issues'
import { Logs } from '@/pages/Logs'
import { Metrics } from '@/pages/Metrics'
import { NotFound } from '@/pages/NotFound'
import { Onboarding } from '@/pages/Onboarding'
import { Overview } from '@/pages/Overview'
import { PublicStatus } from '@/pages/PublicStatus'
import { ReleaseDetail, Releases } from '@/pages/Releases'
import { ServiceDetail } from '@/pages/ServiceDetail'
import { Services } from '@/pages/Services'
import { SignalPage } from '@/pages/SignalPage'
import { StatusPageEditor } from '@/pages/StatusPageEditor'
import { TraceDetail } from '@/pages/TraceDetail'
import { Traces } from '@/pages/Traces'
import { ForgotPassword } from '@/pages/auth/ForgotPassword'
import { ResetPassword } from '@/pages/auth/ResetPassword'
import { SignIn } from '@/pages/auth/SignIn'
import { SettingsLayout } from '@/pages/settings/SettingsLayout'
import { Profile } from '@/pages/settings/Profile'
import { Security } from '@/pages/settings/Security'
import { Preferences } from '@/pages/settings/Preferences'
import { OrgGeneral } from '@/pages/settings/OrgGeneral'
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
  }
  interface Register {
    router: AppRouter
  }
}

type Marker = { mock?: boolean }
const mock: Marker = { mock: true }
const real: Marker = {}

const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()({ component: Outlet, notFoundComponent: NotFound })

const signInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/sign-in',
  component: SignIn,
  validateSearch: (search: Record<string, unknown>): { redirect?: string } => ({
    redirect: typeof search.redirect === 'string' && search.redirect.startsWith('/') ? search.redirect : undefined,
  }),
})

const forgotPasswordRoute = createRoute({ getParentRoute: () => rootRoute, path: '/forgot-password', component: ForgotPassword })

const resetPasswordRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/reset',
  component: ResetPassword,
  validateSearch: (search: Record<string, unknown>): { token?: string } => ({ token: typeof search.token === 'string' ? search.token : undefined }),
})

const publicStatusRoute = createRoute({ getParentRoute: () => rootRoute, path: '/status/$slug', component: PublicStatus, staticData: mock })

const shellRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: 'shell',
  component: AppShell,
  beforeLoad: async ({ context, location }) => {
    try {
      await context.queryClient.ensureQueryData(accountQuery)
    } catch (error) {
      if (isUnauthorized(error)) throw redirect({ to: '/sign-in', search: { redirect: location.href } })
    }
  },
})

const inShell = (path: string, component: () => React.ReactNode, staticData: Marker = mock) => createRoute({ getParentRoute: () => shellRoute, path, component, staticData })

const settingsRoute = createRoute({ getParentRoute: () => shellRoute, path: '/settings', component: SettingsLayout, staticData: mock })

const settingsChildren = [
  createRoute({ getParentRoute: () => settingsRoute, path: '/', beforeLoad: () => { throw redirect({ to: '/settings/general' }) } }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'profile', component: Profile, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'security', component: Security, staticData: mock }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'preferences', component: Preferences, staticData: mock }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'general', component: OrgGeneral, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'members', component: Members, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'teams', component: Teams, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'projects', component: Projects, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'usage', component: Usage, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'billing', component: Billing, staticData: mock }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'project/general', component: ProjectGeneral, staticData: real }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'project/alerts', component: ProjectAlerts, staticData: mock }),
  createRoute({ getParentRoute: () => settingsRoute, path: 'project/api-keys', component: ProjectApiKeys, staticData: real }),
]

const routeTree = rootRoute.addChildren([
  signInRoute,
  forgotPasswordRoute,
  resetPasswordRoute,
  publicStatusRoute,
  shellRoute.addChildren([
    inShell('/', Overview),
    inShell('/onboarding', Onboarding, real),
    inShell('/services', Services, {}),
    inShell('/services/$name', ServiceDetail, {}),
    inShell('/issues', Issues),
    inShell('/issues/$id', IssueDetail),
    inShell('/releases', Releases),
    inShell('/releases/$version', ReleaseDetail),
    inShell('/alerts', Alerts),
    inShell('/infra/kubernetes', Kubernetes),
    inShell('/infra/docker', Docker),
    inShell('/traces', Traces, {}),
    inShell('/traces/$id', TraceDetail, {}),
    inShell('/logs', Logs),
    inShell('/metrics', Metrics),
    inShell('/backend/$view', () => <SignalPage group="backend" />),
    inShell('/frontend/$view', () => <SignalPage group="frontend" />),
    inShell('/status-page', StatusPageEditor),
    settingsRoute.addChildren(settingsChildren),
    inShell('$', NotFound, real),
  ]),
])

export function createAppRouter(queryClient: QueryClient) {
  return createRouter({ routeTree, context: { queryClient }, defaultPreload: 'intent', scrollRestoration: true })
}

export type AppRouter = ReturnType<typeof createAppRouter>
