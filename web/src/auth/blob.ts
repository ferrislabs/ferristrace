import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'
import { get } from './client'

export function useAuthorisedUrl(src: string | null | undefined): string | undefined {
  const own = typeof src === 'string' && src.startsWith('/') && !src.startsWith('//')
  const { data } = useQuery({
    queryKey: ['blob', src],
    queryFn: async () => (await get(src as string)).blob(),
    enabled: own,
    retry: false,
    staleTime: Infinity,
  })
  const [url, setUrl] = useState<string>()
  useEffect(() => {
    if (!data) {
      setUrl(undefined)
      return
    }
    const made = URL.createObjectURL(data)
    setUrl(made)
    return () => URL.revokeObjectURL(made)
  }, [data])
  return own ? url : (src ?? undefined)
}

export async function download(path: string, fallback: string): Promise<void> {
  const response = await get(path)
  const blob = await response.blob()
  const named = /filename\*?=(?:UTF-8'')?"?([^";]+)"?/i.exec(response.headers.get('content-disposition') ?? '')
  const link = document.createElement('a')
  const url = URL.createObjectURL(blob)
  link.href = url
  link.download = named ? decodeURIComponent(named[1]) : fallback
  document.body.append(link)
  link.click()
  link.remove()
  setTimeout(() => URL.revokeObjectURL(url), 10_000)
}
