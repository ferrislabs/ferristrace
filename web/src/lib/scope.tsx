import { createContext, useContext, useState, type ReactNode } from 'react'

type Chosen = { org: string | undefined; project: string | undefined }
type Ctx = Chosen & { chooseOrg: (org: string) => void; chooseProject: (project: string | undefined) => void }

const ScopeContext = createContext<Ctx | null>(null)
const ORG = 'ft-org'
const PROJECT = 'ft-project'

function remembered(key: string): string | undefined {
  try {
    return localStorage.getItem(key) ?? undefined
  } catch {
    return undefined
  }
}

function remember(key: string, value: string | undefined) {
  try {
    if (value === undefined) localStorage.removeItem(key)
    else localStorage.setItem(key, value)
  } catch {
  }
}

export function ScopeProvider({ children }: { children: ReactNode }) {
  const [chosen, setChosen] = useState<Chosen>(() => ({
    org: remembered(ORG),
    project: remembered(PROJECT),
  }))
  const chooseOrg = (org: string) => {
    remember(ORG, org)
    remember(PROJECT, undefined)
    setChosen({ org, project: undefined })
  }
  const chooseProject = (project: string | undefined) => {
    remember(PROJECT, project)
    setChosen((was) => ({ ...was, project }))
  }
  return <ScopeContext.Provider value={{ ...chosen, chooseOrg, chooseProject }}>{children}</ScopeContext.Provider>
}

export function useScope(): Ctx {
  const scope = useContext(ScopeContext)
  if (!scope) throw new Error('useScope outside provider')
  return scope
}
