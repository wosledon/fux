import { useState } from 'react'
import { useNavigate, useOutletContext } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import {
  Box, Typography, Button, Card, CardContent, Chip, Stack, Dialog, DialogTitle, DialogContent,
  DialogActions, TextField, MenuItem, IconButton, Alert, Tooltip,
} from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import EditIcon from '@mui/icons-material/Edit'
import PlayArrowIcon from '@mui/icons-material/PlayArrow'
import AccountTreeIcon from '@mui/icons-material/AccountTree'
import { get, post, del, parseJson, type Workflow, type WorkflowStage, type Project } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton, EmptyState, useToast } from '../ui'

export default function WorkflowsPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const nav = useNavigate()
  const toast = useToast()
  const isAdmin = role === 'owner' || role === 'admin'
  const [running, setRunning] = useState<Workflow | null>(null)

  const { data: workflows = [] } = useQuery({ queryKey: ['workflows', wsId], queryFn: () => get<Workflow[]>(`/workspaces/${wsId}/workflows`) })
  const { data: projects = [] } = useQuery({ queryKey: ['projects', wsId], queryFn: () => get<Project[]>(`/workspaces/${wsId}/projects`) })

  return (
    <Box sx={{ p: { xs: 2, md: 3.5 }, overflowY: 'auto', height: '100%', width: '100%', maxWidth: 1500, mx: 'auto' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 0.5 }}>
        <AccountTreeIcon sx={{ color: 'primary.main' }} />
        <Typography variant="h6" fontWeight={700} sx={{ flexGrow: 1 }}>工作流</Typography>
        {isAdmin && <Button variant="contained" startIcon={<AddIcon />} onClick={() => nav(`/ws/${wsId}/workflows/new`)}>新建工作流</Button>}
      </Box>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 3 }}>
        把标准做法固化成节点 DAG：每个节点绑定智能体与项目，依赖满足即自动并行推进；产物通过 git 分支交接。
      </Typography>

      {workflows.length === 0 && (
        <EmptyState icon={<AccountTreeIcon />} title="还没有工作流"
          desc="创建一个模板，把「设计 proto → 前后端并行开发 → 联调」这类流程固定下来。"
          action={isAdmin ? <Button variant="contained" onClick={() => nav(`/ws/${wsId}/workflows/new`)}>新建工作流</Button> : undefined} />
      )}

      <Stack spacing={1.5}>
        {workflows.map(wf => {
          const stages = parseJson<WorkflowStage[]>(wf.stages, [])
          return (
            <Card key={wf.id}>
              <CardContent sx={{ py: 2, '&:last-child': { pb: 2 } }}>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5, flexWrap: 'wrap' }}>
                  <Typography fontWeight={700} sx={{ flexGrow: 1 }}>{wf.name}</Typography>
                  <Chip size="small" variant="outlined" label={`${stages.length} 节点`} />
                  <Button size="small" variant="contained" startIcon={<PlayArrowIcon />} onClick={() => setRunning(wf)}>发起</Button>
                  {isAdmin && <Tooltip title="编辑"><IconButton size="small" onClick={() => nav(`/ws/${wsId}/workflows/${wf.id}`)}><EditIcon fontSize="small" /></IconButton></Tooltip>}
                  {isAdmin && <DeleteButton title="删除工作流" onDelete={() => del(`/workflows/${wf.id}`)} />}
                </Box>
                {wf.description && <Typography variant="body2" color="text.secondary" sx={{ mt: 0.75 }}>{wf.description}</Typography>}
                <Box sx={{ display: 'flex', gap: 0.75, mt: 1.5, flexWrap: 'wrap' }}>
                  {stages.map(s => (
                    <Chip key={s.key} size="small" label={s.name || s.key} variant="outlined"
                      color={s.dependsOn.length === 0 ? 'primary' : 'default'} />
                  ))}
                </Box>
              </CardContent>
            </Card>
          )
        })}
      </Stack>

      {running && <RunDialog wf={running} wsId={wsId} projects={projects}
        onClose={() => setRunning(null)} onCreated={() => { setRunning(null); toast('已发起，等待调度器认领', 'success') }} />}
    </Box>
  )
}

function RunDialog({ wf, wsId, projects, onClose, onCreated }: {
  wf: Workflow; wsId: string; projects: Project[]; onClose: () => void; onCreated: () => void
}) {
  const [title, setTitle] = useState('')
  const [content, setContent] = useState('')
  const [projectId, setProjectId] = useState('')
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)
  const start = async () => {
    setBusy(true); setErr('')
    try {
      await post(`/workflows/${wf.id}/run`, { title, content, projectId: projectId || undefined })
      onCreated()
    } catch (e: any) { setErr(e.message) } finally { setBusy(false) }
  }
  return (
    <Dialog open onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>发起：{wf.name}</DialogTitle>
      <DialogContent>
        {err && <Alert severity="error" sx={{ mb: 2 }}>{err}</Alert>}
        <Stack spacing={2} sx={{ mt: 1 }}>
          <TextField autoFocus label="任务标题" value={title} onChange={e => setTitle(e.target.value)} required />
          <TextField label="任务内容（目标 / 约束 / 验收）" value={content} onChange={e => setContent(e.target.value)} multiline minRows={5} />
          <TextField select label="主项目（可空）" value={projectId} onChange={e => setProjectId(e.target.value)}>
            <MenuItem value="">由节点定义决定</MenuItem>
            {projects.map(p => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
          </TextField>
          <Typography variant="caption" color="text.secondary">
            发起后直接进入待办，调度器认领后按依赖关系自动推进（可并行）；全部节点完成后进入审核。
          </Typography>
        </Stack>
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2 }}>
        <Button onClick={onClose}>取消</Button>
        <Button variant="contained" disabled={busy || !title.trim()} onClick={() => void start()}>发起</Button>
      </DialogActions>
    </Dialog>
  )
}