import { Logo } from '@/components/ft/Logo'

export function AuthFrame({ title, intro, children, footer }: { title: string; intro?: React.ReactNode; children: React.ReactNode; footer?: React.ReactNode }) {
  return (
    <div className="flex min-h-screen items-center justify-center bg-sidebar px-4 py-10">
      <div className="flex w-full max-w-[380px] flex-col gap-4">
        <div className="flex w-full flex-col gap-4 rounded-xl border bg-card p-6 shadow-sm">
          <div className="flex items-center gap-2 text-[17px] font-bold">
            <Logo className="size-5" />
            FerrisTrace
          </div>
          <div className="flex flex-col gap-1">
            <h1 className="text-title">{title}</h1>
            {intro ? <p className="m-0 text-control text-muted-foreground">{intro}</p> : null}
          </div>
          {children}
        </div>
        {footer ? <p className="m-0 px-1 text-caption text-muted-foreground">{footer}</p> : null}
      </div>
    </div>
  )
}
