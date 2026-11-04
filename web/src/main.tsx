import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { MutationCache, QueryCache, QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider } from '@tanstack/react-router'
import { useEffect } from 'react'
import { AuthProvider, useAuth } from 'react-oidc-context'
import { Toaster } from 'sonner'
import { methodsQuery } from '@/auth/api'
import { isUnauthorized } from '@/auth/client'
import { createUserManager, keepFresh, reauthenticate, safeTarget } from '@/auth/oidc'
import { ScopeProvider } from '@/lib/scope'
import { ThemeProvider, useTheme } from '@/lib/theme'
import { createAppRouter } from './routes'
import './styles/globals.css'

function toSignIn(error: unknown) {
  if (!isUnauthorized(error)) return
  const { pathname, href } = router.state.location
  if (pathname === '/sign-in') return
  void router.navigate({ to: '/sign-in', search: { redirect: href }, replace: true })
}

const queryClient = new QueryClient({
  queryCache: new QueryCache({ onError: toSignIn }),
  mutationCache: new MutationCache({ onError: toSignIn }),
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
})

const router = createAppRouter(queryClient)

function ThemedToaster() {
  const { dark } = useTheme()
  return <Toaster theme={dark ? 'dark' : 'light'} position="bottom-right" toastOptions={{ className: 'text-control' }} />
}

function Session({ children }: { children: React.ReactNode }) {
  const { isLoading } = useAuth()
  useEffect(() => keepFresh(() => void reauthenticate()), [])
  return isLoading ? null : children
}

function App() {
  return (
    <ThemeProvider>
      <ScopeProvider>
        <QueryClientProvider client={queryClient}>
          <RouterProvider router={router} />
          <ThemedToaster />
        </QueryClientProvider>
      </ScopeProvider>
    </ThemeProvider>
  )
}

async function start() {
  const methods = await queryClient.fetchQuery(methodsQuery)
  const root = createRoot(document.getElementById('root')!)
  if (!methods.oidc || !methods.issuer || !methods.client_id) {
    root.render(<StrictMode><App /></StrictMode>)
    return
  }
  const userManager = createUserManager(methods.issuer, methods.client_id)
  root.render(
    <StrictMode>
      <AuthProvider
        userManager={userManager}
        onSigninCallback={(user) => router.history.replace(safeTarget(user?.state))}
      >
        <Session>
          <App />
        </Session>
      </AuthProvider>
    </StrictMode>,
  )
}

void start()

if (import.meta.env.PROD && 'serviceWorker' in navigator) {
  window.addEventListener('load', () => {
    void navigator.serviceWorker.register('/sw.js').catch(() => undefined)
  })
}
