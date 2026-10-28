import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect } from 'react'
import { api, ApiError, type Me } from './api'
import { applyLook } from './look'
import { setLanguage } from '../i18n'

/** Usuario actual, o `null` si no hay sesión. */
export function useMe() {
  return useQuery({
    queryKey: ['me'],
    queryFn: async (): Promise<Me | null> => {
      try {
        return await api.me()
      } catch (e) {
        if (e instanceof ApiError && e.status === 401) return null
        throw e
      }
    },
    retry: false,
  })
}

/** Guarda el usuario en caché y aplica su idioma y apariencia. */
export function useSetMe() {
  const qc = useQueryClient()
  return (me: Me | null) => qc.setQueryData(['me'], me)
}

export function useApplyUser(me: Me | null | undefined) {
  useEffect(() => {
    if (!me) return
    applyLook(me.preferences)
    setLanguage(me.language)
  }, [me])
}
