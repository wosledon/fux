import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Table, TableHead, TableRow, TableCell, TableBody, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, MenuItem, Alert, IconButton, Tooltip, FormControlLabel, Checkbox } from '@mui/material'
import PersonAddIcon from '@mui/icons-material/PersonAdd'
import DeleteIcon from '@mui/icons-material/Delete'
import ContentCopyIcon from '@mui/icons-material/ContentCopy'
import { get, post, del, type Member, type Invitation } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { useAuth } from '../auth'
import { DeleteButton } from '../ui'

export default function MembersPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const { user } = useAuth()
  const qc = useQueryClient()
  const isAdmin = role === 'owner' || role === 'admin'
  const { data: members = [] } = useQuery({ queryKey: ['members', wsId], queryFn: () => get<Member[]>(`/workspaces/${wsId}/members`) })
  const { data: invites = [] } = useQuery({ queryKey: ['invites', wsId], queryFn: () => get<Invitation[]>(`/workspaces/${wsId}/invitations`), enabled: isAdmin })
  const [open, setOpen] = useState(false)
  const [invRole, setInvRole] = useState('member')
  const [hours, setHours] = useState(72)
  const [oneTime, setOneTime] = useState(true)
  const [created, setCreated] = useState<Invitation | null>(null)
  const [err, setErr] = useState('')
  const invalidate = () => { qc.invalidateQueries({ queryKey: ['members', wsId] }); qc.invalidateQueries({ queryKey: ['invites', wsId] }) }

  const invite = async () => {
    setErr('')
    try {
      const inv = await post<Invitation>(`/workspaces/${wsId}/invitations`, { role: invRole, expiresHours: hours, oneTime })
      setCreated(inv); invalidate()
    } catch (e: any) { setErr(e.message) }
  }
  const inviteUrl = created ? `${location.origin}/invite/${created.code}` : ''

  return (
    <Box sx={{ p: 3, overflowY: 'auto', height: '100%' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', mb: 2 }}>
        <Typography variant="h6" fontWeight={700} sx={{ flexGrow: 1 }}>成员</Typography>
        {isAdmin && <Button variant="contained" startIcon={<PersonAddIcon />} onClick={() => { setOpen(true); setCreated(null) }}>邀请成员</Button>}
      </Box>
      <Table size="small" sx={{ mb: 3 }}>
        <TableHead><TableRow><TableCell>用户</TableCell><TableCell>角色</TableCell><TableCell>加入时间</TableCell><TableCell /></TableRow></TableHead>
        <TableBody>
          {members.map(m => (
            <TableRow key={m.userId} hover>
              <TableCell>{m.displayName}（{m.username}）{m.userId === user?.id && <Chip label="我" sx={{ ml: 1 }} />}</TableCell>
              <TableCell>
                {isAdmin && m.role !== 'owner' && m.userId !== user?.id ? (
                  <TextField select size="small" value={m.role} onChange={async e => {
                    try { await post(`/workspaces/${wsId}/members/${m.userId}/role`, { role: e.target.value }); invalidate() } catch (e2: any) { setErr(e2.message) }
                  }}>
                    <MenuItem value="admin">admin</MenuItem>
                    <MenuItem value="member">member</MenuItem>
                  </TextField>
                ) : <Chip label={m.role} color={m.role === 'owner' ? 'primary' : 'default'} />}
              </TableCell>
              <TableCell>{m.joinedAt?.slice(0, 10)}</TableCell>
              <TableCell align="right">
                {isAdmin && m.role !== 'owner' && m.userId !== user?.id && (
                  <DeleteButton title="移除成员" content="移除后该成员将失去工作区访问权限。" onDelete={() => del(`/workspaces/${wsId}/members/${m.userId}`)} />
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
      {err && <Alert severity="error" sx={{ mb: 2 }} onClose={() => setErr('')}>{err}</Alert>}

      {isAdmin && invites.length > 0 && (
        <>
          <Typography variant="subtitle2" gutterBottom>邀请记录</Typography>
          <Table size="small">
            <TableHead><TableRow><TableCell>邀请码</TableCell><TableCell>角色</TableCell><TableCell>状态</TableCell><TableCell>过期</TableCell></TableRow></TableHead>
            <TableBody>
              {invites.map(i => (
                <TableRow key={i.code}><TableCell><code>{i.code}</code></TableCell><TableCell>{i.role}</TableCell>
                  <TableCell><Chip label={i.status} color={i.status === 'active' ? 'success' : 'default'} /></TableCell>
                  <TableCell>{i.expiresAt?.slice(0, 16).replace('T', ' ') || '永不'}</TableCell></TableRow>
              ))}
            </TableBody>
          </Table>
        </>
      )}

      <Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth="xs">
        <DialogTitle>邀请成员</DialogTitle>
        <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
          {!created ? (
            <>
              <TextField select label="预设角色" value={invRole} onChange={e => setInvRole(e.target.value)}>
                <MenuItem value="member">member（建任务/评论）</MenuItem>
                <MenuItem value="admin">admin（配置/审核批准）</MenuItem>
              </TextField>
              <TextField label="有效期（小时）" type="number" value={hours} onChange={e => setHours(Number(e.target.value))} />
              <FormControlLabel control={<Checkbox checked={oneTime} onChange={e => setOneTime(e.target.checked)} />} label="一次性（接受后失效）" />
              {err && <Alert severity="error">{err}</Alert>}
            </>
          ) : (
            <>
              <Alert severity="success">邀请已创建</Alert>
              <TextField label="邀请链接" value={inviteUrl} fullWidth InputProps={{ readOnly: true, endAdornment: <IconButton onClick={() => navigator.clipboard.writeText(inviteUrl)}><ContentCopyIcon fontSize="small" /></IconButton> }} />
              <TextField label="邀请码" value={created.code} fullWidth InputProps={{ readOnly: true, endAdornment: <IconButton onClick={() => navigator.clipboard.writeText(created.code)}><ContentCopyIcon fontSize="small" /></IconButton> }} />
            </>
          )}
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)}>关闭</Button>
          {!created && <Button variant="contained" onClick={invite}>生成邀请</Button>}
        </DialogActions>
      </Dialog>
    </Box>
  )
}