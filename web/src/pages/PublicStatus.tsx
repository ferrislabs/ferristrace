import { StatusPageView } from '@/components/ft/StatusPageView'
import { MockData } from '@/components/ft/MockData'
import { useStatusConfig } from '@/data/status-page'

export function PublicStatus() {
  const config = useStatusConfig()
  return (
    <>
      <div className="flex justify-center bg-background px-4 py-2">
        <MockData />
      </div>
      <StatusPageView c={config} />
    </>
  )
}
