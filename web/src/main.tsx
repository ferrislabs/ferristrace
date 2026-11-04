import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { MutationCache, QueryCache, QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider } from '@tanstack/react-router'
import { useEffect, useState } from 'react'
import { AuthProvider, useAuth } from 'react-oidc-context'
import { Toaster } from 'sonner'
import { methodsQuery } from '@/auth/api'
import { isUnauthorized } from '@/auth/client'
import { STALLED, createUserManager, isPublicPath, keepFresh, reauthenticate, safeTarget, signedOut } from '@/auth/oidc'
import { SignedOut, SignInStalled, SignInUnavailable } from '@/pages/auth/SignInTrouble'
import { ScopeProvider } from '@/lib/scope'
import { ThemeProvider, useTheme } from '@/lib/theme'
import { createAppRouter } from './routes'
import './styles/globals.css'

function toProvider(error: unknown) {
  if (!isUnauthorized(error)) return
  void reauthenticate(router.state.location.href)
}

const queryClient = new QueryClient({
  queryCache: new QueryCache({ onError: toProvider }),
  mutationCache: new MutationCache({ onError: toProvider }),
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
})

const router = createAppRouter(queryClient)

function ThemedToaster() {
  const { dark } = useTheme()
  return <Toaster theme={dark ? 'dark' : 'light'} position="bottom-right" toastOptions={{ className: 'text-control' }} />
}

function Session({ children }: { children: React.ReactNode }) {
  const { isLoading, isAuthenticated, error } = useAuth()
  const [stalled, setStalled] = useState(false)
  const open = isPublicPath(window.location.pathname)
  useEffect(() => {
    const stall = () => setStalled(true)
    window.addEventListener(STALLED, stall)
    return () => window.removeEventListener(STALLED, stall)
  }, [])
  useEffect(() => keepFresh(() => void reauthenticate()), [])
  useEffect(() => {
    if (!isLoading && !isAuthenticated && !open) void reauthenticate()
  }, [isLoading, isAuthenticated, open])
  if (!isLoading && !isAuthenticated && !open && signedOut()) return <SignedOut />
  if (stalled || (error && !open)) return <SignInStalled />
  if (isLoading || (!isAuthenticated && !open)) return null
  return children
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
    root.render(<StrictMode>{isPublicPath(window.location.pathname) ? <App /> : <SignInUnavailable />}</StrictMode>)
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
