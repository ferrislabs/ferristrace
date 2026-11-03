import { noteResponse } from '@/lib/offline'

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly body: string,
  ) {
    super(`the server answered ${status}`)
    this.name = 'ApiError'
  }
}

export const isUnauthorized = (error: unknown) => error instanceof ApiError && error.status === 401

export const because = (error: unknown, otherwise: string) =>
  error instanceof ApiError && error.body.length > 0 ? error.body : otherwise

const REQUESTED_WITH = { 'x-requested-with': 'ferristrace-web' }

async function send(path: string, init: RequestInit): Promise<Response> {
  const response = await fetch(path, { credentials: 'include', ...init, headers: { ...REQUESTED_WITH, ...init.headers } })
  noteResponse(response)
  if (!response.ok) throw new ApiError(response.status, (await response.text()).trim())
  return response
}

export const get = (path: string) => send(path, { method: 'GET' })

const carrying = (method: string) => (path: string, body?: unknown) =>
  send(path, {
    method,
    headers: body === undefined ? {} : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })

export const post = carrying('POST')

export const offer = (path: string, body: Blob) => send(path, { method: 'POST', body })
export const patch = carrying('PATCH')
export const put = carrying('PUT')
export const del = carrying('DELETE')

export async function json<T>(response: Response): Promise<T> {
  if (!response.headers.get('content-type')?.includes('json')) {
    throw new ApiError(response.status, 'the server answered a page rather than data, so it is older than this application and does not serve this path yet')
  }
  return (await response.json()) as T
}
