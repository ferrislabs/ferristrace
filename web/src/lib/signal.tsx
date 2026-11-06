import { createContext, useContext } from 'react'
import type { Signal } from '@/lib/traceQuery'

const SignalContext = createContext<Signal>('traces')

export const SignalProvider = SignalContext.Provider

export const useSignal = (): Signal => useContext(SignalContext)
