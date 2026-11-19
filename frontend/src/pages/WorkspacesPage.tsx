import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Button, Card, CardActionArea, CardContent, Chip, Dialog, DialogActions, DialogContent, DialogTitle, TextField, Typography, AppBar, Toolbar, IconButton, Tooltip, Grid } from '@mui/material'
import LogoutIcon from '@mui/icons-material/Logout'
import AddIcon from '@mui/icons-material/Add'
import GroupAddIcon from '@mui/icons-material/GroupAdd'
import ArrowForwardIcon from '@mui/icons-material/ArrowForward'
import { get, post, type Workspace } from '../api'
import { useAuth } from '../auth'
import { Logo, EmptyState, PersonAvatar, useToast } from '../ui'

export default function WorkspacesPage() {
  const { user, logout } = useAuth()
  const nav = useNavigate()
  const qc = useQueryClient()
  const toast = useToast()
  const { data = [] } = useQuery({
    queryKey: ['workspaces'],
    queryFn: () => get<{ workspace: Workspace; role: string }[]>('/workspaces'),
  })
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [inviteOpen, setInviteOpen] = useState(false)
  const [code, setCode] = useState('')
  const [err, setErr] = useState('')

  const create = async () => {
    try {
      const ws = await post<Workspace>('/workspaces', { name })
      qc.invalidateQueries({ queryKey: ['workspaces'] })
      setOpen(false); setName('')
      toast('工作区已创建', 'success')
      nav(`/ws/${ws.id}`)
    } catch (e: any) { setErr(e.message) }
  }
  const accept = async () => {
    setErr('')
    try {
      const ws = await post<Workspace>(`/invitations/${code.trim()}/accept`)
      toast('已加入工作区', 'success')
      nav(`/ws/${ws.id}`)
    } catch (e: any) { setErr(e.message) }
  }

  return (
    <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column', bgcolor: 'background.default' }}>
      <AppBar position="static" color="transparent" elevation={0} sx={{ borderBottom: '1px solid', borderColor: 'divider', backdropFilter: 'blur(12px)' }}>
        <Toolbar>
          <Logo size={30} />
          <Box sx={{ flexGrow: 1 }} />
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mr: 1 }}>
            <PersonAvatar name={user?.displayName || user?.username} />
            <Typography color="text.secondary">{user?.displayName}</Typography>
          </Box>
          <Tooltip title="退出登录"><IconButton onClick={logout}><LogoutIcon /></IconButton></Tooltip>
        </Toolbar>
      </AppBar>

      <Box sx={{ p: { xs: 3, md: 5 }, maxWidth: 1080, mx: 'auto', width: '100%', overflowY: 'auto' }}>
        <Box sx={{ display: 'flex', alignItems: 'flex-end', gap: 2, mb: 4, flexWrap: 'wrap' }}>
          <Box sx={{ flexGrow: 1 }}>
            <Typography variant="h4">工作区</Typography>
            <Typography color="text.secondary" sx={{ mt: 0.5 }}>每个工作区独立管理项目、智能体、看板与成员</Typography>
          </Box>
          <Button variant="contained" size="large" startIcon={<AddIcon />} onClick={() => { setOpen(true); setErr('') }}>新建工作区</Button>
          <Button variant="outlined" size="large" startIcon={<GroupAddIcon />} onClick={() => { setInviteOpen(true); setErr('') }}>接受邀请</Button>
        </Box>

        {data.length === 0 ? (
          <Card><EmptyState title="还没有工作区" desc="创建第一个工作区，添加项目与智能体，让看板自己跑起来"
            action={<Button variant="contained" startIcon={<AddIcon />} onClick={() => setOpen(true)}>新建工作区</Button>} /></Card>
        ) : (
          <Grid container spacing={2.5}>
            {data.map(({ workspace: w, role }) => (
              <Grid key={w.id} size={{ xs: 12, sm: 6, md: 4 }}>
                <Card sx={{ transition: 'transform .15s, box-shadow .15s', '&:hover': { transform: 'translateY(-3px)', boxShadow: '0 12px 30px rgba(29,36,58,0.1)' } }}>
                  <CardActionArea onClick={() => nav(`/ws/${w.id}`)} sx={{ p: 0.5 }}>
                    <CardContent>
                      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 1.5 }}>
                        <Box sx={{ width: 34, height: 34, borderRadius: 1.25, display: 'grid', placeItems: 'center', fontWeight: 800, bgcolor: '#eceaff', color: 'primary.main' }}>
                          {w.name.slice(0, 1)}
                        </Box>
                        <Typography variant="h6" sx={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{w.name}</Typography>
                        <ArrowForwardIcon sx={{ color: 'text.secondary', fontSize: 18 }} />
                      </Box>
                      <Chip size="small" label={role === 'owner' ? '所有者' : role === 'admin' ? '管理员' : '成员'}
                        color={role === 'owner' ? 'primary' : 'default'} variant={role === 'owner' ? 'filled' : 'outlined'} />
                      <Typography variant="caption" color="text.secondary" display="block" sx={{ mt: 1.5 }}>
                        创建于 {w.createdAt?.slice(0, 10)}
                      </Typography>
                    </CardContent>
                  </CardActionArea>
                </Card>
              </Grid>
            ))}
          </Grid>
        )}
      </Box>

      <Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth="xs">
        <DialogTitle>新建工作区</DialogTitle>
        <DialogContent>
          <TextField autoFocus fullWidth label="名称" value={name} onChange={e => setName(e.target.value)} sx={{ mt: 1 }}
            onKeyDown={e => e.key === 'Enter' && name.trim() && create()} />
          {err && <Typography color="error" sx={{ mt: 1 }}>{err}</Typography>}
        </DialogContent>
        <DialogActions sx={{ px: 3, pb: 2 }}>
          <Button onClick={() => setOpen(false)}>取消</Button>
          <Button variant="contained" disabled={!name.trim()} onClick={create}>创建</Button>
        </DialogActions>
      </Dialog>

      <Dialog open={inviteOpen} onClose={() => setInviteOpen(false)} fullWidth maxWidth="xs">
        <DialogTitle>接受邀请</DialogTitle>
        <DialogContent>
          <TextField autoFocus fullWidth label="邀请码" value={code} onChange={e => setCode(e.target.value)} sx={{ mt: 1 }} />
          {err && <Typography color="error" sx={{ mt: 1 }}>{err}</Typography>}
        </DialogContent>
        <DialogActions sx={{ px: 3, pb: 2 }}>
          <Button onClick={() => setInviteOpen(false)}>取消</Button>
          <Button variant="contained" disabled={!code.trim()} onClick={accept}>加入</Button>
        </DialogActions>
      </Dialog>
    </Box>
  )
}