import { useEffect, useMemo, useRef, useState } from 'react'
import { Outlet, useNavigate, useParams, NavLink } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, ButtonBase, Chip, Divider, Drawer, IconButton, List, ListItemButton, ListItemIcon, ListItemText, Tooltip, Typography } from '@mui/material'
import LogoutIcon from '@mui/icons-material/Logout'
import MenuIcon from '@mui/icons-material/Menu'
import LightModeIcon from '@mui/icons-material/LightMode'
import DarkModeIcon from '@mui/icons-material/DarkMode'
import ViewKanbanIcon from '@mui/icons-material/ViewKanban'
import AutoAwesomeIcon from '@mui/icons-material/AutoAwesome'
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
import { useColorMode } from '../ThemeModeProvider'
import { Logo, PersonAvatar } from '../ui'

export interface WsCtx { wsId: string; role: string; workspace: Workspace }

const NAV_GROUPS = [
  {
    label: '工作台', items: [
      { to: '', label: '工作看板', icon: <ViewKanbanIcon /> },
      { to: 'assistant', label: 'Fux Pilot', icon: <AutoAwesomeIcon /> },
      { to: 'inbox', label: '收件箱', icon: <InboxIcon /> },
      { to: 'projects', label: '项目', icon: <FolderIcon /> },
    ],
  },
  {
    label: '智能体', items: [
      { to: 'agents', label: '智能体', icon: <PsychologyIcon /> },
      { to: 'workflows', label: '工作流', icon: <AccountTreeIcon /> },
    ],
  },
]

const ADMIN_ITEMS = [
  { to: 'types', label: '任务类型', icon: <CategoryIcon /> },
  { to: 'clis', label: 'CLI 接入', icon: <TerminalIcon /> },
  { to: 'skills', label: 'SKILL', icon: <SchoolIcon /> },
  { to: 'mcp', label: 'MCP', icon: <HubIcon /> },
  { to: 'members', label: '成员', icon: <PeopleIcon /> },
  { to: 'nodes', label: '节点', icon: <DnsIcon /> },
]

export default function WorkspaceLayout() {
  const { wsId = '' } = useParams()
  const nav = useNavigate()
  const qc = useQueryClient()
  const { user, logout } = useAuth()
  const { mode, toggleMode } = useColorMode()
  const [mobileOpen, setMobileOpen] = useState(false)
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

  const sidebar = (
    <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column', color: '#d5dbea', bgcolor: '#171b26' }}>
      <Box sx={{ height: 72, px: 2.5, display: 'flex', alignItems: 'center', gap: 1.25, flexShrink: 0 }}>
        <Logo size={28} inverted />
        <Typography variant="body2" fontWeight={700} color="#f4f6fa">Fux</Typography>
        <Chip label="AI 工作台" size="small" sx={{ ml: 'auto', height: 20, fontSize: 9.5, color: '#8f9ab0', bgcolor: 'rgba(255,255,255,0.06)' }} />
      </Box>
      <Box sx={{ px: 1.5, pb: 2 }}>
        <ButtonBase onClick={() => { nav('/'); setMobileOpen(false) }} aria-label="切换工作区" sx={{
          width: '100%', p: 1.25, borderRadius: 2, textAlign: 'left', gap: 1.25,
          bgcolor: 'rgba(255,255,255,0.05)', border: '1px solid rgba(255,255,255,0.07)',
          transition: 'background-color .15s, border-color .15s',
          '&:hover': { bgcolor: 'rgba(255,255,255,0.085)', borderColor: 'rgba(255,255,255,0.11)' },
        }}>
          <Box sx={{
            width: 32, height: 32, borderRadius: 1.5, display: 'grid', placeItems: 'center', flexShrink: 0,
            color: '#dbeafe', bgcolor: 'rgba(37,99,235,0.28)', fontWeight: 700, fontSize: 13,
          }}>{ctx.workspace.name.slice(0, 1)}</Box>
          <Box sx={{ flexGrow: 1, minWidth: 0 }}>
            <Typography variant="body2" fontWeight={600} color="#f4f6fa" noWrap>{ctx.workspace.name}</Typography>
            <Typography variant="caption" sx={{ color: '#8f9ab0', fontSize: 10.5 }}>{roleLabel}</Typography>
          </Box>
          <Tooltip title="切换工作区">
            <ChevronRightIcon sx={{ fontSize: 18, color: '#8f9ab0' }} />
          </Tooltip>
        </ButtonBase>
      </Box>
      <Box component="nav" aria-label="工作区主导航" sx={{
        overflowY: 'auto', flexGrow: 1, py: 1, scrollbarWidth: 'thin', scrollbarColor: '#465066 transparent',
        '&::-webkit-scrollbar': { width: 6 }, '&::-webkit-scrollbar-thumb': { bgcolor: '#465066', borderRadius: 3 },
        '&::-webkit-scrollbar-track': { bgcolor: 'transparent' }, '&::-webkit-scrollbar-button': { display: 'none' },
      }}>
        {NAV_GROUPS.map(group => (
          <Box key={group.label} sx={{ mb: 1.25 }}>
            <Typography variant="caption" sx={{
              px: 3, py: 0.75, display: 'block', color: '#717d93',
              fontSize: 10.5, fontWeight: 600, letterSpacing: '0.1em',
            }}>
              {group.label}
            </Typography>
            <List dense disablePadding>
              {group.items.map(n => (
                <ListItemButton key={n.to} component={NavLink} to={`/ws/${wsId}${n.to ? '/' + n.to : ''}`} end={n.to === ''}
                  onClick={() => setMobileOpen(false)}
                  sx={{
                    position: 'relative', color: '#aeb7c8', height: 42, mx: 1.5, my: 0.125, px: 1.5,
                    '& .MuiListItemIcon-root': { color: '#7f8ba1', transition: 'color .15s' },
                    '&:hover': { color: '#f4f6fa', bgcolor: 'rgba(255,255,255,0.045)', '& .MuiListItemIcon-root': { color: '#d5dbea' } },
                    '&.active': {
                      bgcolor: 'rgba(37,99,235,0.2)', color: '#ffffff',
                      '&::before': { content: '""', position: 'absolute', left: -6, top: 10, bottom: 10, width: 3, borderRadius: 2, bgcolor: '#60a5fa' },
                      '& .MuiListItemIcon-root': { color: '#7db4ff' },
                    },
                  }}>
                  <ListItemIcon sx={{ minWidth: 34, '& svg': { fontSize: 18 } }}>{n.icon}</ListItemIcon>
                  <ListItemText primary={n.label} primaryTypographyProps={{ fontSize: 13, fontWeight: 500 }} />
                  {n.to === 'inbox' && unread > 0 && <Chip size="small" label={unread > 99 ? '99+' : unread} sx={{ height: 20, minWidth: 20, fontSize: 11, color: '#dbeafe', bgcolor: 'rgba(37,99,235,0.35)' }} />}
                </ListItemButton>
              ))}
            </List>
          </Box>
        ))}
        <Box sx={{ mt: 0.5 }}>
          <Divider sx={{ mx: 1.5, borderColor: 'rgba(255,255,255,0.08)' }} />
          <Typography variant="caption" sx={{
            px: 3, pt: 1.25, pb: 0.75, display: 'block', color: '#717d93',
            fontSize: 10.5, fontWeight: 600, letterSpacing: '0.1em',
          }}>
            配置
          </Typography>
          {ADMIN_ITEMS.map(n => (
            <ListItemButton key={n.to} component={NavLink} to={`/ws/${wsId}/${n.to}`}
              onClick={() => setMobileOpen(false)}
              sx={{
                position: 'relative', color: '#9da7ba', height: 42, mx: 1.5, my: 0.125, px: 1.5, borderRadius: 1.25,
                '& .MuiListItemIcon-root': { color: '#6f7b91', transition: 'color .15s' },
                '&:hover': { color: '#f4f6fa', bgcolor: 'rgba(255,255,255,0.045)', '& .MuiListItemIcon-root': { color: '#d5dbea' } },
                '&.active': {
                  bgcolor: 'rgba(37,99,235,0.2)', color: '#ffffff',
                  '&::before': { content: '""', position: 'absolute', left: -6, top: 10, bottom: 10, width: 3, borderRadius: 2, bgcolor: '#60a5fa' },
                  '& .MuiListItemIcon-root': { color: '#7db4ff' },
                },
              }}>
              <ListItemIcon sx={{ minWidth: 34, '& svg': { fontSize: 18 } }}>{n.icon}</ListItemIcon>
              <ListItemText primary={n.label} primaryTypographyProps={{ fontSize: 13, fontWeight: 500 }} />
            </ListItemButton>
          ))}
        </Box>
      </Box>
      <Box sx={{ p: 2, borderTop: '1px solid rgba(255,255,255,0.07)' }}>
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.25, px: 0.5 }}>
          <PersonAvatar name={user?.displayName || user?.username} size={32} />
          <Box sx={{ flexGrow: 1, minWidth: 0 }}>
            <Typography variant="body2" fontWeight={600} color="#eef1f7" noWrap>{user?.displayName}</Typography>
            <Typography variant="caption" sx={{ color: '#8f9ab0' }} noWrap display="block">@{user?.username}</Typography>
          </Box>
          <Tooltip title={mode === 'dark' ? '切换到浅色模式' : '切换到深色模式'}>
            <IconButton size="small" aria-label="切换明暗主题" onClick={toggleMode} sx={{ color: '#8f9ab0', '&:hover': { color: '#fff', bgcolor: 'rgba(255,255,255,0.08)' } }}>
              {mode === 'dark' ? <LightModeIcon fontSize="small" /> : <DarkModeIcon fontSize="small" />}
            </IconButton>
          </Tooltip>
          <Tooltip title="退出登录"><IconButton size="small" onClick={logout} sx={{ color: '#8f9ab0', '&:hover': { color: '#fff', bgcolor: 'rgba(255,255,255,0.08)' } }}><LogoutIcon fontSize="small" /></IconButton></Tooltip>
        </Box>
      </Box>
    </Box>
  )

  return (
    <Box sx={{ display: 'flex', height: '100dvh', bgcolor: 'background.default' }}>
      <Box component="aside" sx={{ width: 248, flexShrink: 0, display: { xs: 'none', md: 'block' } }}>
        {sidebar}
      </Box>
      <Drawer open={mobileOpen} onClose={() => setMobileOpen(false)} ModalProps={{ keepMounted: true }}
        sx={{ display: { xs: 'block', md: 'none' }, '& .MuiDrawer-paper': { width: 280, border: 0 } }}>
        {sidebar}
      </Drawer>

      <Box component="main" sx={{ flexGrow: 1, minWidth: 0, minHeight: 0, display: 'flex', flexDirection: 'column', overflow: 'hidden', bgcolor: 'background.default' }}>
        <Box sx={{ height: 64, px: 2, display: { xs: 'flex', md: 'none' }, alignItems: 'center', gap: 1.5, flexShrink: 0, bgcolor: 'background.paper', borderBottom: '1px solid', borderColor: 'divider' }}>
          <IconButton aria-label="打开导航" onClick={() => setMobileOpen(true)}><MenuIcon /></IconButton>
          <Logo size={26} />
          <Typography variant="body2" fontWeight={700}>Fux</Typography>
          <Box sx={{ flexGrow: 1 }} />
          <Tooltip title={mode === 'dark' ? '切换到浅色模式' : '切换到深色模式'}>
            <IconButton aria-label="切换明暗主题" onClick={toggleMode}>
              {mode === 'dark' ? <LightModeIcon /> : <DarkModeIcon />}
            </IconButton>
          </Tooltip>
          <PersonAvatar name={user?.displayName || user?.username} size={30} />
        </Box>
        <Box sx={{ flexGrow: 1, minHeight: 0, overflow: 'hidden' }}>
          <Outlet context={ctx} />
        </Box>
      </Box>
    </Box>
  )
}