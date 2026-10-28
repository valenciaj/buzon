import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Navigate, Route, Routes, useLocation } from 'react-router'
import { api } from './lib/api'
import { useApplyUser, useMe } from './lib/session'
import { Alert, Narrow } from './components/ui'
import Setup from './pages/Setup'
import Login from './pages/Login'
import Invite from './pages/Invite'
import Home from './pages/Home'
import Settings from './pages/Settings'

export default function App() {
  const { t } = useTranslation()
  const location = useLocation()
  const setup = useQuery({ queryKey: ['setup'], queryFn: api.setupStatus })
  const me = useMe()
  useApplyUser(me.data)

  if (setup.isLoading || me.isLoading) return <Narrow><p className="text-muted">{t('common.loading')}</p></Narrow>
  if (setup.isError || me.isError) return <Narrow><Alert>{t('common.error')}</Alert></Narrow>

  if (setup.data?.needs_setup && !me.data) {
    return location.pathname === '/setup' ? <Setup /> : <Navigate to="/setup" replace />
  }

  return (
    <Routes>
      <Route path="/invite/:token" element={<Invite />} />
      <Route path="/login" element={me.data ? <Navigate to="/" replace /> : <Login />} />
      <Route path="/setup" element={<Navigate to="/" replace />} />
      <Route path="/settings/:tab?" element={me.data ? <Settings me={me.data} /> : <Navigate to="/login" replace />} />
      <Route path="/" element={me.data ? <Home me={me.data} /> : <Navigate to="/login" replace />} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
