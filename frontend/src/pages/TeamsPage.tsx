import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Card, CardContent, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, MenuItem, Alert, IconButton, List, ListItem, ListItemText } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import DeleteIcon from '@mui/icons-material/Delete'
import EditIcon from '@mui/icons-material/Edit'
import ArrowUpwardIcon from '@mui/icons-material/ArrowUpward'
import ArrowDownwardIcon from '@mui/icons-material/ArrowDownward'
import { get, post, patch, del, type Agent, type Team, type TeamMember } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton } from '../ui'

export default function TeamsPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const qc = useQueryClient()
  const isAdmin = role === 'owner' || role === 'admin'
  const { data: teams = [] } = useQuery({ queryKey: ['teams', wsId], queryFn: () => get<{ team: Team; members: TeamMember[] }[]>(`/workspaces/${wsId}/teams`) })
  const { data: agents = [] } = useQuery({ queryKey: ['agents', wsId], queryFn: () => get<Agent[]>(`/workspaces/${wsId}/agents`) })
  const [editing, setEditing] = useState<{ team: Team; members: TeamMember[] } | 'new' | null>(null)
  const invalidate = () => qc.invalidateQueries({ queryKey: ['teams', wsId] })

  return (
    <Box sx={{ p: 3, overflowY: 'auto', height: '100%' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', mb: 2 }}>
        <Typography variant="h6" fontWeight={700} sx={{ flexGrow: 1 }}>智能体团队</Typography>
        {isAdmin && <Button variant="contained" startIcon={<AddIcon />} onClick={() => setEditing('new')}>新建团队</Button>}
      </Box>
      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 2 }}>
        流水线：成员按阶段交接（对应工作子泳道）；主管：lead 拆解分派汇总（v1 基础实现）。
      </Typography>
      <Box sx={{ display: 'grid', gap: 2, gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))' }}>
        {teams.map(({ team, members }) => (
          <Card key={team.id}>
            <CardContent>
              <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                <Typography fontWeight={700} sx={{ flexGrow: 1 }}>{team.name}</Typography>
                <Chip label={team.mode === 'pipeline' ? '流水线' : '主管'} />
                {isAdmin && <>
                  <IconButton size="small" onClick={() => setEditing({ team, members })}><EditIcon fontSize="small" /></IconButton>
                  <DeleteButton title="删除团队" onDelete={() => del(`/teams/${team.id}`)} />
                </>}
              </Box>
              <List dense>
                {members.map(m => (
                  <ListItem key={m.agentId} disableGutters>
                    <ListItemText primary={`${m.position + 1}. ${agents.find(a => a.id === m.agentId)?.name || m.agentId}`} secondary={m.role} />
                  </ListItem>
                ))}
              </List>
            </CardContent>
          </Card>
        ))}
        {teams.length === 0 && <Typography color="text.secondary">还没有团队</Typography>}
      </Box>
      {editing && <TeamDialog data={editing === 'new' ? null : editing} wsId={wsId} agents={agents}
        onClose={() => setEditing(null)} onSaved={() => { invalidate(); setEditing(null) }} />}
    </Box>
  )
}

function TeamDialog({ data, wsId, agents, onClose, onSaved }: {
  data: { team: Team; members: TeamMember[] } | null; wsId: string; agents: Agent[]; onClose: () => void; onSaved: () => void
}) {
  const [name, setName] = useState(data?.team.name || '')
  const [mode, setMode] = useState(data?.team.mode || 'pipeline')
  const [members, setMembers] = useState<{ agentId: string; role: string }[]>(
    data ? data.members.slice().sort((a, b) => a.position - b.position).map(m => ({ agentId: m.agentId, role: m.role })) : [])
  const [err, setErr] = useState('')

  const move = (i: number, dir: -1 | 1) => {
    setMembers(m => {
      const copy = [...m]
      const j = i + dir
      if (j < 0 || j >= copy.length) return m
      ;[copy[i], copy[j]] = [copy[j], copy[i]]
      return copy
    })
  }
  const save = async () => {
    setErr('')
    try {
      const body = { name, mode, members: members.map((m, i) => ({ agentId: m.agentId, role: m.role, position: i })) }
      if (data) await patch(`/teams/${data.team.id}`, body)
      else await post(`/workspaces/${wsId}/teams`, body)
      onSaved()
    } catch (e: any) { setErr(e.message) }
  }

  return (
    <Dialog open onClose={onClose} fullWidth maxWidth="xs">
      <DialogTitle>{data ? '编辑团队' : '新建团队'}</DialogTitle>
      <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
        <TextField label="名称 *" value={name} onChange={e => setName(e.target.value)} />
        <TextField select label="编排模式" value={mode} onChange={e => setMode(e.target.value as any)}>
          <MenuItem value="pipeline">流水线（阶段交接）</MenuItem>
          <MenuItem value="supervisor">主管（分派-汇总）</MenuItem>
        </TextField>
        <Box>
          <Typography variant="subtitle2" gutterBottom>成员（按阶段排序）</Typography>
          {members.map((m, i) => (
            <Box key={i} sx={{ display: 'flex', gap: 0.5, alignItems: 'center', mb: 1 }}>
              <TextField select size="small" value={m.agentId} onChange={e => setMembers(ms => ms.map((x, j) => j === i ? { ...x, agentId: e.target.value } : x))} sx={{ flexGrow: 1 }}>
                {agents.map(a => <MenuItem key={a.id} value={a.id}>{a.name}</MenuItem>)}
              </TextField>
              <TextField size="small" label="角色" value={m.role} onChange={e => setMembers(ms => ms.map((x, j) => j === i ? { ...x, role: e.target.value } : x))} sx={{ width: 100 }} />
              <IconButton size="small" onClick={() => move(i, -1)}><ArrowUpwardIcon fontSize="small" /></IconButton>
              <IconButton size="small" onClick={() => move(i, 1)}><ArrowDownwardIcon fontSize="small" /></IconButton>
              <IconButton size="small" onClick={() => setMembers(ms => ms.filter((_, j) => j !== i))}><DeleteIcon fontSize="small" /></IconButton>
            </Box>
          ))}
          <Button size="small" startIcon={<AddIcon />} disabled={agents.length === 0}
            onClick={() => setMembers(m => [...m, { agentId: agents[0]?.id || '', role: 'worker' }])}>添加成员</Button>
        </Box>
        {err && <Alert severity="error">{err}</Alert>}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>取消</Button>
        <Button variant="contained" onClick={save} disabled={!name.trim() || members.length === 0}>保存</Button>
      </DialogActions>
    </Dialog>
  )
}