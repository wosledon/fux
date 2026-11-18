import { useEffect, useMemo, useRef, useState } from 'react'
import { Outlet, useNavigate, useParams, NavLink } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { AppBar, Toolbar, Typography, IconButton, Drawer, List, ListItemButton, ListItemIcon, ListItemText, Box, Chip, Badge, Popover, Divider, Tooltip, Button } from '@mui/material'
import ArrowBackIcon from '@mui/icons-material/ArrowBack'
import NotificationsIcon from '@mui/icons-material/Notifications'
import SmartToyIcon from '@mui/icons-material/SmartToy'
import LogoutIcon from '@mui/icons-material/Logout'
import ViewKanbanIcon from '@mui/icons-material/ViewKanban'
import FolderIcon from '@mui/icons-material/Folder'
import CategoryIcon from '@mui/icons-material/Category'
import PsychologyIcon from '@mui/icons-material/Psychology'
import GroupsIcon from '@mui/icons-material/Groups'
import TerminalIcon from '@mui/icons-material/Terminal'
import SchoolIcon from '@mui/icons-material/School'
import HubIcon from '@mui/icons-material/Hub'
import PeopleIcon from '@mui/icons-material/People'
import DnsIcon from '@mui/icons-material/Dns'
import { get, post, workspaceEventsUrl, type Workspace } from '../api'
import { useAuth } from '../auth'
import { Logo, PersonAvatar } from '../ui'
import AssistantPanel from './AssistantPanel'

export interface WsCtx { wsId: string; role: string; workspace: Workspace }

const NAV_GROUPS = [
  {
    label: '工作台', items: [
      { to: '', label: '工作看板', icon: <ViewKanbanIcon /> },
      { to: 'projects', label: '项目', icon: <FolderIcon /> },
      { to: 'types', label: '任务类型', icon: <CategoryIcon /> },
    ],
  },
  {
    label: '智能体', items: [
      { to: 'agents', label: '智能体', icon: <PsychologyIcon /> },
      { to: 'teams', label: '团队', icon: <GroupsIcon /> },
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
  const [assistantOpen, setAssistantOpen] = useState(false)
  const [notifAnchor, setNotifAnchor] = useState<HTMLElement | null>(null)
  const timer = useRef<number>(0)

  const { data: notifs = [] } = useQuery({
    queryKey: ['notifications', wsId],
    queryFn: () => get<{ notification: any; read: boolean }[]>(`/workspaces/${wsId}/notifications`),
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

  return (
    <Box sx={{ display: 'flex', height: '100%', bgcolor: 'background.default' }}>
      <Drawer variant="permanent" sx={{ width: 216, flexShrink: 0, '& .MuiDrawer-paper': { width: 216, boxSizing: 'border-box', bgcolor: 'background.paper' } }}>
        <Box sx={{ px: 2, py: 1.75, display: 'flex', alignItems: 'center' }}>
          <Logo size={28} />
        </Box>
        <Divider />
        <Box sx={{ px: 2, py: 1.5 }}>
          <Typography variant="caption" color="text.secondary" sx={{ letterSpacing: '0.06em' }}>当前工作区</Typography>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mt: 0.5 }}>
            <Typography fontWeight={700} sx={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {ctx.workspace.name}
            </Typography>
            <Chip size="small" label={ctx.role} variant="outlined" />
          </Box>
        </Box>
        <Divider />
        <Box sx={{ overflowY: 'auto', flexGrow: 1, py: 1 }}>
          {NAV_GROUPS.map(g => (
            <Box key={g.label} sx={{ mb: 1 }}>
              <Typography variant="caption" color="text.secondary" sx={{ px: 2.5, py: 0.5, display: 'block', letterSpacing: '0.06em' }}>
                {g.label}
              </Typography>
              <List dense disablePadding>
                {g.items.map(n => (
                  <ListItemButton key={n.to} component={NavLink} to={`/ws/${wsId}${n.to ? '/' + n.to : ''}`} end={n.to === ''}
                    sx={{
                      '&.active': { bgcolor: 'action.selected', color: 'primary.light', '& .MuiListItemIcon-root': { color: 'primary.light' } },
                    }}>
                    <ListItemIcon sx={{ minWidth: 34, color: 'text.secondary' }}>{n.icon}</ListItemIcon>
                    <ListItemText primary={n.label} primaryTypographyProps={{ fontSize: 13, fontWeight: 600 }} />
                  </ListItemButton>
                ))}
              </List>
            </Box>
          ))}
        </Box>
        <Divider />
        <Box sx={{ p: 1.5, display: 'flex', alignItems: 'center', gap: 1 }}>
          <PersonAvatar name={user?.displayName || user?.username} size={28} />
          <Typography variant="body2" sx={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {user?.displayName}
          </Typography>
          <Tooltip title="退出登录"><IconButton size="small" onClick={logout}><LogoutIcon fontSize="small" /></IconButton></Tooltip>
        </Box>
      </Drawer>

      <Box sx={{ flexGrow: 1, display: 'flex', flexDirection: 'column', minWidth: 0 }}>
        <AppBar position="static" color="transparent" elevation={0} sx={{ borderBottom: '1px solid', borderColor: 'divider', bgcolor: 'rgba(16,20,28,0.7)', backdropFilter: 'blur(12px)' }}>
          <Toolbar variant="dense" sx={{ gap: 0.5 }}>
            <Tooltip title="返回工作区列表">
              <IconButton size="small" onClick={() => nav('/')} sx={{ mr: 0.5 }}><ArrowBackIcon fontSize="small" /></IconButton>
            </Tooltip>
            <Typography fontWeight={700} sx={{ flexGrow: 1 }}>{ctx.workspace.name}</Typography>
            <Button size="small" startIcon={<SmartToyIcon />} onClick={() => setAssistantOpen(true)}
              variant={assistantOpen ? 'contained' : 'outlined'} sx={{ mr: 1 }}>
              AI 助理
            </Button>
            <Tooltip title="通知">
              <IconButton onClick={e => setNotifAnchor(e.currentTarget)}>
                <Badge badgeContent={unread} color="error"><NotificationsIcon /></Badge>
              </IconButton>
            </Tooltip>
          </Toolbar>
        </AppBar>
        <Box sx={{ flexGrow: 1, minHeight: 0, overflow: 'hidden' }}>
          <Outlet context={ctx} />
        </Box>
      </Box>

      <Popover open={!!notifAnchor} anchorEl={notifAnchor} onClose={() => setNotifAnchor(null)}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'right' }} transformOrigin={{ vertical: 'top', horizontal: 'right' }}
        slotProps={{ paper: { sx: { borderRadius: 3, border: '1px solid', borderColor: 'divider', mt: 1 } } }}>
        <Box sx={{ width: 380, maxHeight: 440, overflow: 'auto' }}>
          <Box sx={{ px: 2, py: 1.5, display: 'flex', alignItems: 'center' }}>
            <Typography fontWeight={700} sx={{ flexGrow: 1 }}>通知</Typography>
            {unread > 0 && <Chip size="small" label={`${unread} 未读`} color="error" />}
          </Box>
          <Divider />
          {notifs.length === 0 && <Typography color="text.secondary" sx={{ p: 3, textAlign: 'center' }}>暂无通知</Typography>}
          {notifs.map(({ notification: n, read }) => (
            <Box key={n.id} sx={{ px: 2, py: 1.5, borderBottom: '1px solid', borderColor: 'divider', bgcolor: read ? 'transparent' : 'rgba(109,141,255,0.06)', display: 'flex', gap: 1, alignItems: 'center' }}>
              <Box sx={{ width: 8, height: 8, borderRadius: '50%', bgcolor: read ? 'transparent' : 'primary.main', flexShrink: 0 }} />
              <Box sx={{ flexGrow: 1, minWidth: 0 }}>
                <Typography variant="body2">{n.message}</Typography>
                <Typography variant="caption" color="text.secondary">{n.createdAt?.replace('T', ' ').slice(0, 19)}</Typography>
              </Box>
              {!read && <Button size="small" onClick={async () => { await post(`/notifications/${n.id}/read`); qc.invalidateQueries({ queryKey: ['notifications', wsId] }) }}>已读</Button>}
            </Box>
          ))}
        </Box>
      </Popover>

      <AssistantPanel wsId={wsId} open={assistantOpen} onClose={() => setAssistantOpen(false)} />
    </Box>
  )
}