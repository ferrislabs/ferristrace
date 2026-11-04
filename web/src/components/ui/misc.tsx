import type { ComponentProps } from 'react'
import { useAuthorisedUrl } from '@/auth/blob'
import { cn } from '@/lib/utils'

export const Kbd = ({ className, ...p }: ComponentProps<'kbd'>) => (
  <kbd className={cn('inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-sm border border-b-2 bg-background px-1 font-mono text-[10.5px] leading-none text-muted-foreground', className)} {...p} />
)

export const Skeleton = ({ className }: { className?: string }) => <div className={cn('h-2.5 animate-pulse-soft rounded-sm bg-muted', className)} />

export function Alert({ variant = 'info', icon, title, children, action, className }: { variant?: 'info' | 'warning' | 'destructive' | 'success'; icon?: React.ReactNode; title?: React.ReactNode; children?: React.ReactNode; action?: React.ReactNode; className?: string }) {
  const v = {
    info: 'bg-accent-subtle text-accent-text border-transparent',
    warning: 'bg-warning-subtle text-warning-text border-warning-border',
    destructive: 'bg-destructive-subtle text-destructive-text border-destructive-border',
    success: 'bg-success-subtle text-success-text border-success-border',
  }[variant]
  return (
    <div role={variant === 'destructive' ? 'alert' : 'status'} className={cn('flex items-start gap-2.5 rounded-md border px-3 py-2.5 text-control [&>svg]:mt-0.5 [&>svg]:size-4 [&>svg]:shrink-0', v, className)}>
      {icon}
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        {title ? <span className="font-semibold">{title}</span> : null}
        {children}
      </div>
      {action}
    </div>
  )
}

export function Table({ className, ...p }: ComponentProps<'table'>) {
  return <table className={cn('w-full border-separate border-spacing-0 text-control', className)} {...p} />
}
export const Th = ({ className, numeric, ...p }: ComponentProps<'th'> & { numeric?: boolean }) => (
  <th className={cn('sticky top-0 h-8 whitespace-nowrap border-b bg-background px-3 text-left text-caption font-semibold text-muted-foreground', numeric && 'text-right', className)} {...p} />
)
export const Td = ({ className, numeric, ...p }: ComponentProps<'td'> & { numeric?: boolean }) => (
  <td className={cn('h-8 whitespace-nowrap border-b border-border-subtle px-3 align-middle', numeric && 'text-right tabular font-mono', className)} {...p} />
)
export const Tr = ({ className, ...p }: ComponentProps<'tr'>) => <tr className={cn('hover:[&>td]:bg-row-hover', className)} {...p} />

export const Avatar = ({ initials, src, className }: { initials: string; src?: string | null; className?: string }) => {
  const shown = useAuthorisedUrl(src)
  return (
  <span aria-hidden className={cn('relative inline-flex size-7 shrink-0 items-center justify-center overflow-hidden rounded-full bg-accent-subtle text-[12px] font-bold text-accent-text', className)}>
    {initials}
    {shown ? <img src={shown} alt="" className="absolute inset-0 size-full object-cover" onError={(e) => { e.currentTarget.style.display = 'none' }} /> : null}
  </span>
  )
}
