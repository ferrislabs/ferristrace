import * as D from '@radix-ui/react-dialog'
import { X } from 'lucide-react'
import { cn } from '@/lib/utils'

export const Sheet = D.Root
export const SheetClose = D.Close

export function SheetContent({ className, children, title, ...p }: D.DialogContentProps & { title: React.ReactNode }) {
  return (
    <D.Portal>
      <D.Overlay className="fixed inset-0 z-40 bg-overlay" />
      <D.Content
        className={cn(
          'fixed inset-x-0 bottom-0 z-50 flex max-h-[88dvh] flex-col rounded-t-2xl border-t bg-popover shadow-lg focus:outline-none',
          'pb-[max(12px,env(safe-area-inset-bottom))]',
          className,
        )}
        {...p}
      >
        <div className="flex shrink-0 items-center gap-2 px-4 pt-2.5 pb-1">
          <span aria-hidden className="absolute left-1/2 top-2 h-1 w-10 -translate-x-1/2 rounded-full bg-border" />
          <D.Title className="mt-3 flex-1 text-heading">{title}</D.Title>
          <D.Description className="sr-only">{String(title)}</D.Description>
          <D.Close className="mt-3 inline-flex size-10 cursor-pointer items-center justify-center rounded-md text-muted-foreground hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring" aria-label="Close">
            <X className="size-5" />
          </D.Close>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-2">{children}</div>
      </D.Content>
    </D.Portal>
  )
}
