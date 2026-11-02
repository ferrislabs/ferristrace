import { Link as RouterLink, Outlet, useLocation as useRouterLocation, useNavigate as useRouterNavigate, useParams as useRouterParams } from '@tanstack/react-router'
import type { ComponentProps } from 'react'

type AnchorProps = Omit<ComponentProps<'a'>, 'href'>

export { Outlet }

export function Link({ to, search, ...props }: AnchorProps & { to: string; search?: Record<string, string> }) {
  return <RouterLink to={to as never} search={search as never} {...props} />
}

export const isActivePath = (pathname: string, to: string, end = false) =>
  end || to === '/' ? pathname === to : pathname === to || pathname.startsWith(`${to}/`)

type NavLinkProps = Omit<AnchorProps, 'className'> & { to: string; end?: boolean; className?: string | ((state: { isActive: boolean }) => string) }

export function NavLink({ to, end, className, children, ...props }: NavLinkProps) {
  const { pathname } = useRouterLocation()
  const isActive = isActivePath(pathname, to, end)
  return (
    <Link to={to} className={typeof className === 'function' ? className({ isActive }) : className} {...props}>
      {children}
    </Link>
  )
}

export function useNavigate() {
  const navigate = useRouterNavigate()
  return (to: string, options?: { replace?: boolean }) => void navigate({ to: to as never, replace: options?.replace })
}

export const useLocation = useRouterLocation

export const useParams = () => useRouterParams({ strict: false }) as Record<string, string | undefined>

export function useSearchParam(name: string): string | undefined {
  const search = useRouterLocation().search as Record<string, unknown>
  const value = search[name]
  return typeof value === 'string' ? value : undefined
}
