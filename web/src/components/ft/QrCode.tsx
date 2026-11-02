import qrcode from 'qrcode-generator'
import { useMemo } from 'react'
import { cn } from '@/lib/utils'

const QUIET = 4

export function QrCode({ value, label, className }: { value: string; label: string; className?: string }) {
  const drawn = useMemo(() => {
    const code = qrcode(0, 'M')
    code.addData(value)
    code.make()
    const modules = code.getModuleCount()
    const side = modules + QUIET * 2
    const dark: string[] = []
    for (let row = 0; row < modules; row += 1) {
      for (let column = 0; column < modules; column += 1) {
        if (code.isDark(row, column)) dark.push(`M${column + QUIET} ${row + QUIET}h1v1h-1z`)
      }
    }
    return { side, path: dark.join('') }
  }, [value])
  return (
    <svg
      role="img"
      aria-label={label}
      viewBox={`0 0 ${drawn.side} ${drawn.side}`}
      shapeRendering="crispEdges"
      className={cn('size-[148px] shrink-0 rounded-lg border bg-white p-1 text-black', className)}
    >
      <path d={drawn.path} fill="currentColor" />
    </svg>
  )
}
