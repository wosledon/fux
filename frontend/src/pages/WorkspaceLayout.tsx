import { useEffect, useMemo, useRef } from 'react'
import { Outlet, useNavigate, useParams, NavLink } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, ButtonBase, Chip, Divider, IconButton, List, ListItemButton, ListItemIcon, ListItemText, Tooltip, Typography } from '@mui/material'
import LogoutIcon from '@mui/icons-material/Logout'
import ViewKanbanIcon from '@mui/icons-material/ViewKanban'
import SmartToyIcon from '@mui/icons-material/SmartToy'
import InboxIcon from '@mui/icons-material/Inbox'
import FolderIcon from '@mui/icons-material/Folder'
import CategoryIcon from '@mui/icons-material/Category'
import PsychologyIcon from '@mui/icons-material/Psychology'
import AccountTreeIcon from '@mui/icons-material/AccountTree'
import TerminalIcon from '@mui/icons-material/Terminal'
import SchoolIcon from '@mui/icons-material/School'
import HubIcon from '@mui/icons-material/Hub'
import PeopleIcon from '@mui/icons-material/People'
import DnsIcon from '@mui/icons-material/Dns'
import ChevronRightIcon from '@mui/icons-material/ChevronRight'
import { get, workspaceEventsUrl, type Workspace } from '../api'
import { useAuth } from '../auth'
import { Logo, PersonAvatar } from '../ui'

export interface WsCtx { wsId: string; role: string; workspace: Workspace }

const NAV_GROUPS = [
  {
    label: '工作台', items: [
      { to: '', label: '工作看板', icon: <ViewKanbanIcon /> },
      { to: 'assistant', label: 'AI 助理', icon: <SmartToyIcon /> },
      { to: 'inbox', label: '收件箱', icon: <InboxIcon /> },
      { to: 'projects', label: '项目', icon: <FolderIcon /> },
      { to: 'types', label: '任务类型', icon: <CategoryIcon /> },
    ],
  },
  {
    label: '智能体', items: [
      { to: 'agents', label: '智能体', icon: <PsychologyIcon /> },
      { to: 'workflows', label: '工作流', icon: <AccountTreeIcon /> },
      { to: 'clis', label: 'CLI 接入', icon: <TerminalIcon /> },
      { to: 'skills', label: 'SKILL', icon: <SchoolIcon /> },
      { to: 'mcp', label: 'MCP', icon: <HubIcon /> },
    ],
  },
  {
    label: '系统', items: [
      { to: 'members', label: '成员', icon: <PeopleIcon /> },
      { to: 'nodes', label: '节点', icon: <DnsIcon /> },
    ],
  },
]

export default function WorkspaceLayout() {
  const { wsId = '' } = useParams()
  const nav = useNavigate()
  const qc = useQueryClient()
  const { user, logout } = useAuth()
  const { data } = useQuery({ queryKey: ['workspace', wsId], queryFn: () => get<{ workspace: Workspace; role: string }>(`/workspaces/${wsId}`) })
  const timer = useRef<number>(0)

  const { data: notifs = [] } = useQuery({
    queryKey: ['notifications', wsId],
    queryFn: () => get<{ notification: { id: string }; read: boolean }[]>(`/workspaces/${wsId}/notifications`),
    refetchInterval: 15000,
  })
  const unread = notifs.filter(n => !n.read).length

  useEffect(() => {
    if (!wsId) return
    const es = new EventSource(workspaceEventsUrl(wsId))
    es.onmessage = (ev) => {
      try {
        const env = JSON.parse(ev.data)
        if (String(env.kind).startsWith('task.') || env.kind === 'pr.merged_detected') {
          window.clearTimeout(timer.current)
          timer.current = window.setTimeout(() => {
            qc.invalidateQueries({ queryKey: ['tasks'] })
            qc.invalidateQueries({ queryKey: ['notifications', wsId] })
          }, 250)
        }
      } catch { /* ignore */ }
    }
    return () => es.close()
  }, [wsId, qc])

  const ctx: WsCtx | null = useMemo(() => data ? { wsId, role: data.role, workspace: data.workspace } : null, [data, wsId])
  if (!ctx) return null

  const roleLabel = ctx.role === 'owner' ? '所有者' : ctx.role === 'admin' ? '管理员' : '成员'

  return (
    <Box sx={{ display: 'flex', height: '100%', bgcolor: 'background.default' }}>
      <Box component="aside" sx={{
        width: 236, flexShrink: 0, display: 'flex', flexDirection: 'column',
        bgcolor: '#fcfcfe', borderRight: '1px solid', borderColor: 'divider',
      }}>
        <Box sx={{ px: 3, py: 3 }}><Logo size={30} /></Box>
        <Box sx={{ px: 2, pb: 2.5 }}>
          <ButtonBase onClick={() => nav('/')} sx={{
            width: '100%', p: 1.25, borderRadius: 3, textAlign: 'left', gap: 1.25,
            bgcolor: '#f1f2f7', transition: 'background-color .15s, transform .15s',
            '&:hover': { bgcolor: '#ebecf3', transform: 'translateY(-1px)' },
          }}>
            <Box sx={{
              width: 34, height: 34, borderRadius: 2.5, display: 'grid', placeItems: 'center', flexShrink: 0,
              color: '#5b50d6', bgcolor: '#e5e3ff', fontWeight: 800,
            }}>{ctx.workspace.name.slice(0, 1)}</Box>
            <Box sx={{ flexGrow: 1, minWidth: 0 }}>
              <Typography variant="body2" fontWeight={700} noWrap>{ctx.workspace.name}</Typography>
              <Typography variant="caption" color="text.secondary">{roleLabel}工作区</Typography>
            </Box>
            <ChevronRightIcon sx={{ fontSize: 18, color: 'text.secondary' }} />
          </ButtonBase>
        </Box>
        <Divider />
        <Box component="nav" sx={{ overflowY: 'auto', flexGrow: 1, py: 1.5 }}>
          {NAV_GROUPS.map(g => (
            <Box key={g.label} sx={{ mb: 1.25 }}>
              <Typography variant="caption" sx={{ px: 3, py: 0.75, display: 'block', color: '#a0a7b5', fontSize: 11, fontWeight: 700, letterSpacing: '0.08em' }}>
                {g.label}
              </Typography>
              <List dense disablePadding>
                {g.items.map(n => (
                  <ListItemButton key={n.to} component={NavLink} to={`/ws/${wsId}${n.to ? '/' + n.to : ''}`} end={n.to === ''}
                    sx={{
                      color: '#697386', minHeight: 40,
                      '& .MuiListItemIcon-root': { color: '#8c96a8', transition: 'color .15s' },
                      '&:hover': { color: 'text.primary', '& .MuiListItemIcon-root': { color: 'text.primary' } },
                      '&.active': {
                        bgcolor: '#eceaff', color: '#5549cf',
                        '& .MuiListItemIcon-root': { color: '#635bff' },
                      },
                    }}>
                    <ListItemIcon sx={{ minWidth: 34, '& svg': { fontSize: 20 } }}>{n.icon}</ListItemIcon>
                    <ListItemText primary={n.label} primaryTypographyProps={{ fontSize: 13.5, fontWeight: 600 }} />
                    {n.to === 'inbox' && unread > 0 && <Chip size="small" label={unread} color="primary" sx={{ height: 20, minWidth: 20, fontSize: 11 }} />}
                  </ListItemButton>
                ))}
              </List>
            </Box>
          ))}
        </Box>
        <Box sx={{ p: 2, borderTop: '1px solid', borderColor: 'divider' }}>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.25, px: 0.5 }}>
            <PersonAvatar name={user?.displayName || user?.username} size={32} />
            <Box sx={{ flexGrow: 1, minWidth: 0 }}>
              <Typography variant="body2" fontWeight={600} noWrap>{user?.displayName}</Typography>
              <Typography variant="caption" color="text.secondary" noWrap display="block">@{user?.username}</Typography>
            </Box>
            <Tooltip title="退出登录"><IconButton size="small" onClick={logout}><LogoutIcon fontSize="small" /></IconButton></Tooltip>
          </Box>
        </Box>
      </Box>

      <Box component="main" sx={{
        flexGrow: 1, minWidth: 0, minHeight: 0, overflow: 'hidden', bgcolor: 'background.default',
      }}>
        <Outlet context={ctx} />
      </Box>
    </Box>
  )
}