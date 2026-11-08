import * as D from '@radix-ui/react-dialog'
import { X } from 'lucide-react'
import { cn } from '@/lib/utils'

export const Dialog = D.Root
export const DialogTrigger = D.Trigger
export const DialogClose = D.Close

export function DialogContent({ className, children, size = 'sm', title, description, ...p }: D.DialogContentProps & { size?: 'sm' | 'md' | 'lg' | 'xl'; title: React.ReactNode; description?: React.ReactNode }) {
  return (
    <D.Portal>
      <D.Overlay className="fixed inset-0 z-40 bg-overlay" />
      <D.Content
        className={cn(
          'fixed left-1/2 top-1/2 z-50 flex max-h-[calc(100vh-48px)] w-[calc(100%-32px)] -translate-x-1/2 -translate-y-1/2 flex-col gap-4 overflow-y-auto rounded-xl border bg-popover p-5 shadow-lg focus:outline-none',
          size === 'sm' ? 'max-w-[440px]' : size === 'md' ? 'max-w-[560px]' : size === 'lg' ? 'max-w-[720px]' : 'max-w-[1080px]',
          className,
        )}
        {...p}
      >
        <div className="flex flex-col gap-1.5 pr-8">
          <D.Title className="text-heading">{title}</D.Title>
          {description ? <D.Description className="text-control text-muted-foreground">{description}</D.Description> : <D.Description className="sr-only">{String(title)}</D.Description>}
        </div>
        {children}
        <D.Close className="absolute right-3 top-3 inline-flex size-7 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring" aria-label="Close">
          <X className="size-4" />
        </D.Close>
      </D.Content>
    </D.Portal>
  )
}

export const DialogFooter = ({ children }: { children: React.ReactNode }) => <div className="flex justify-end gap-2">{children}</div>
