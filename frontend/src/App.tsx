import { Routes, Route, Navigate, useParams, useNavigate } from 'react-router-dom'
import { Box, CircularProgress, Button, TextField, Typography, Paper } from '@mui/material'
import { useState } from 'react'
import { useAuth } from './auth'
import { post } from './api'
import LoginPage from './pages/LoginPage'
import WorkspacesPage from './pages/WorkspacesPage'
import WorkspaceLayout from './pages/WorkspaceLayout'
import BoardPage from './pages/BoardPage'
import ProjectsPage from './pages/ProjectsPage'
import TaskTypesPage from './pages/TaskTypesPage'
import AgentsPage from './pages/AgentsPage'
import WorkflowsPage from './pages/WorkflowsPage'
import CliEntriesPage from './pages/CliEntriesPage'
import SkillsPage from './pages/SkillsPage'
import McpPage from './pages/McpPage'
import MembersPage from './pages/MembersPage'
import NodesPage from './pages/NodesPage'
import AssistantPage from './pages/AssistantPage'
import InboxPage from './pages/InboxPage'
import TaskPage from './pages/TaskPage'

function RequireAuth({ children }: { children: React.ReactNode }) {
  const { user, loading } = useAuth()
  if (loading) return <Box sx={{ display: 'grid', placeItems: 'center', height: '100%' }}><CircularProgress /></Box>
  if (!user) return <Navigate to="/login" replace />
  return <>{children}</>
}

function InviteAccept() {
  const { code } = useParams()
  const nav = useNavigate()
  const { user } = useAuth()
  const [err, setErr] = useState('')
  const accept = async () => {
    try {
      const ws = await post(`/invitations/${code}/accept`)
      nav(`/ws/${ws.id}`)
    } catch (e: any) { setErr(e.message) }
  }
  if (!user) return <Navigate to={`/login?next=/invite/${code}`} replace />
  return (
    <Box sx={{ display: 'grid', placeItems: 'center', height: '100%' }}>
      <Paper sx={{ p: 4, textAlign: 'center' }}>
        <Typography variant="h6" gutterBottom>接受工作区邀请</Typography>
        <Typography color="text.secondary" sx={{ mb: 2 }}>邀请码：{code}</Typography>
        {err && <Typography color="error" sx={{ mb: 1 }}>{err}</Typography>}
        <Button variant="contained" onClick={accept}>加入工作区</Button>
      </Paper>
    </Box>
  )
}

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route path="/invite/:code" element={<InviteAccept />} />
      <Route path="/" element={<RequireAuth><WorkspacesPage /></RequireAuth>} />
      <Route path="/ws/:wsId" element={<RequireAuth><WorkspaceLayout /></RequireAuth>}>
        <Route index element={<BoardPage />} />
        <Route path="assistant" element={<AssistantPage />} />
        <Route path="inbox" element={<InboxPage />} />
        <Route path="tasks/:taskId" element={<TaskPage />} />
        <Route path="projects" element={<ProjectsPage />} />
        <Route path="types" element={<TaskTypesPage />} />
        <Route path="agents" element={<AgentsPage />} />
        <Route path="workflows" element={<WorkflowsPage />} />
        <Route path="clis" element={<CliEntriesPage />} />
        <Route path="skills" element={<SkillsPage />} />
        <Route path="mcp" element={<McpPage />} />
        <Route path="members" element={<MembersPage />} />
        <Route path="nodes" element={<NodesPage />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}