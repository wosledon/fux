import { useState } from 'react'
import { useNavigate, useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Box, Typography, Button, Card, CardContent, Chip, Stack, Dialog, DialogTitle, DialogContent,
  DialogActions, TextField, MenuItem, IconButton, Alert, Tooltip, Table, TableBody, TableCell,
  TableContainer, TableHead, TableRow,
} from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import ContentCopyIcon from '@mui/icons-material/ContentCopy'
import EditIcon from '@mui/icons-material/Edit'
import HistoryIcon from '@mui/icons-material/History'
import PlayArrowIcon from '@mui/icons-material/PlayArrow'
import AccountTreeIcon from '@mui/icons-material/AccountTree'
import {
  get, post, del, parseJson, STATUS_LABEL, STATUS_COLOR,
  type Workflow, type WorkflowStage, type WorkflowVariable, type Project, type Task,
} from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton, EmptyState, useToast } from '../ui'

export default function WorkflowsPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const nav = useNavigate()
  const toast = useToast()
  const qc = useQueryClient()
  const isAdmin = role === 'owner' || role === 'admin'
  const [running, setRunning] = useState<Workflow | null>(null)
  const [duplicating, setDuplicating] = useState('')

  const { data: workflows = [] } = useQuery({ queryKey: ['workflows', wsId], queryFn: () => get<Workflow[]>(`/workspaces/${wsId}/workflows`) })
  const { data: projects = [] } = useQuery({ queryKey: ['projects', wsId], queryFn: () => get<Project[]>(`/workspaces/${wsId}/projects`) })
  const { data: boardData } = useQuery({
    queryKey: ['board', wsId],
    queryFn: () => get<{ board: { id: string } }>(`/workspaces/${wsId}/board`),
  })
  const boardId = boardData?.board.id
  const { data: tasks = [] } = useQuery({
    queryKey: ['tasks', boardId],
    queryFn: () => get<Task[]>(`/tasks?boardId=${boardId}`),
    enabled: !!boardId,
  })
  const recent = tasks
    .filter(t => !!t.workflowId)
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
    .slice(0, 6)

  const duplicate = async (wf: Workflow) => {
    setDuplicating(wf.id)
    try {
      await post(`/workspaces/${wsId}/workflows`, {
        name: `${wf.name} 副本`,
        description: wf.description,
        variables: parseJson<WorkflowVariable[]>(wf.variables, []),
        stages: parseJson<WorkflowStage[]>(wf.stages, []),
      })
      await qc.invalidateQueries({ queryKey: ['workflows', wsId] })
      toast('工作流已复制', 'success')
    } catch (e: any) {
      toast(e.message, 'error')
    } finally {
      setDuplicating('')
    }
  }

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

      {recent.length > 0 && (
        <Card sx={{ mb: 2.5 }}>
          <CardContent>
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 1 }}>
              <HistoryIcon fontSize="small" color="primary" />
              <Typography variant="subtitle2" fontWeight={700}>最近实例</Typography>
              <Chip size="small" label={`${tasks.filter(t => !!t.workflowId).length} 个流程任务`} variant="outlined" />
            </Box>
            <TableContainer>
              <Table size="small">
                <TableHead>
                  <TableRow>
                    <TableCell>任务</TableCell>
                    <TableCell>工作流</TableCell>
                    <TableCell>进度</TableCell>
                    <TableCell>状态</TableCell>
                    <TableCell>更新时间</TableCell>
                  </TableRow>
                </TableHead>
                <TableBody>
                  {recent.map(task => (
                    <TableRow key={task.id} hover sx={{ cursor: 'pointer' }} onClick={() => nav(`/ws/${wsId}/tasks/${task.id}`)}>
                      <TableCell>
                        <Typography variant="body2" fontWeight={600} noWrap sx={{ maxWidth: 280 }}>{task.title}</Typography>
                        <Typography variant="caption" color="text.secondary">#{task.id.slice(0, 8)}</Typography>
                      </TableCell>
                      <TableCell>{task.workflowName || '—'}</TableCell>
                      <TableCell>{task.stageDone ?? 0}/{task.stageTotal ?? 0} 节点</TableCell>
                      <TableCell>
                        <Chip size="small" label={STATUS_LABEL[task.status]} color={STATUS_COLOR[task.status]} />
                      </TableCell>
                      <TableCell>{task.updatedAt.slice(5, 16).replace('T', ' ')}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </TableContainer>
          </CardContent>
        </Card>
      )}

      {workflows.length === 0 && (
        <EmptyState icon={<AccountTreeIcon />} title="还没有工作流"
          desc="创建一个模板，把「设计 proto → 前后端并行开发 → 联调」这类流程固定下来。"
          action={isAdmin ? <Button variant="contained" onClick={() => nav(`/ws/${wsId}/workflows/new`)}>新建工作流</Button> : undefined} />
      )}

      <Stack spacing={1.5}>
        {workflows.map(wf => {
          const stages = parseJson<WorkflowStage[]>(wf.stages, [])
          const variables = parseJson<WorkflowVariable[]>(wf.variables, [])
          return (
            <Card key={wf.id}>
              <CardContent sx={{ py: 2, '&:last-child': { pb: 2 } }}>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 1.5, flexWrap: 'wrap' }}>
                  <Typography fontWeight={700} sx={{ flexGrow: 1 }}>{wf.name}</Typography>
                  <Chip size="small" variant="outlined" label={`${stages.length} 节点`} />
                  {variables.length > 0 && <Chip size="small" variant="outlined" label={`${variables.length} 变量`} />}
                  <Button size="small" variant="contained" startIcon={<PlayArrowIcon />} onClick={() => setRunning(wf)}>发起</Button>
                  {isAdmin && <Tooltip title="复制"><IconButton size="small" disabled={!!duplicating} onClick={() => void duplicate(wf)}><ContentCopyIcon fontSize="small" /></IconButton></Tooltip>}
                  {isAdmin && <Tooltip title="编辑"><IconButton size="small" onClick={() => nav(`/ws/${wsId}/workflows/${wf.id}`)}><EditIcon fontSize="small" /></IconButton></Tooltip>}
                  {isAdmin && <DeleteButton title="删除工作流" onDelete={async () => {
                    await del(`/workflows/${wf.id}`)
                    await qc.invalidateQueries({ queryKey: ['workflows', wsId] })
                  }} />}
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

      {running && <RunDialog wf={running} projects={projects}
        onClose={() => setRunning(null)} onCreated={() => {
          setRunning(null)
          void qc.invalidateQueries({ queryKey: ['tasks', boardId] })
          toast('已发起，等待调度器认领', 'success')
        }} />}
    </Box>
  )
}

function RunDialog({ wf, projects, onClose, onCreated }: {
  wf: Workflow; projects: Project[]; onClose: () => void; onCreated: () => void
}) {
  const variables = parseJson<WorkflowVariable[]>(wf.variables, [])
  const [title, setTitle] = useState('')
  const [content, setContent] = useState('')
  const [projectId, setProjectId] = useState('')
  const [tags, setTags] = useState('')
  const [priority, setPriority] = useState(0)
  const [values, setValues] = useState<Record<string, string>>(() => Object.fromEntries(
    variables.map(v => [v.key, v.default || '']),
  ))
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)
  const missing = variables.some(v => v.required && !(values[v.key] || '').trim())
  const start = async () => {
    setBusy(true); setErr('')
    try {
      await post(`/workflows/${wf.id}/run`, {
        title,
        content,
        projectId: projectId || undefined,
        tags: tags.split(',').map(x => x.trim()).filter(Boolean),
        priority,
        variables: values,
      })
      onCreated()
    } catch (e: any) { setErr(e.message) } finally { setBusy(false) }
  }
  return (
    <Dialog open onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>发起：{wf.name}</DialogTitle>
      <DialogContent>
        {err && <Alert severity="error" sx={{ mb: 2 }}>{err}</Alert>}
        <Stack spacing={2} sx={{ mt: 1 }}>
          <TextField autoFocus label="任务标题" value={title} onChange={e => setTitle(e.target.value)}
            helperText="支持 {{变量名}}；发起时自动替换" required />
          <TextField label="任务内容（目标 / 约束 / 验收）" value={content} onChange={e => setContent(e.target.value)} multiline minRows={5} />
          {variables.map(v => (
            <TextField
              key={v.key}
              select={(v.options || []).length > 0}
              label={`${v.label}${v.required ? ' *' : ''}`}
              value={values[v.key] || ''}
              helperText={v.description}
              onChange={e => setValues(prev => ({ ...prev, [v.key]: e.target.value }))}
            >
              {(v.options || []).length > 0 && <MenuItem value="">请选择</MenuItem>}
              {(v.options || []).map(option => <MenuItem key={option} value={option}>{option}</MenuItem>)}
            </TextField>
          ))}
          <TextField select label="主项目（可空）" value={projectId} onChange={e => setProjectId(e.target.value)}>
            <MenuItem value="">由节点定义决定</MenuItem>
            {projects.map(p => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
          </TextField>
          <Box sx={{ display: 'grid', gridTemplateColumns: '1fr 140px', gap: 1.5 }}>
            <TextField label="标签（逗号分隔）" value={tags} onChange={e => setTags(e.target.value)} />
            <TextField select label="优先级" value={priority} onChange={e => setPriority(Number(e.target.value))}>
              <MenuItem value={0}>普通</MenuItem>
              <MenuItem value={1}>P1</MenuItem>
              <MenuItem value={2}>P2</MenuItem>
              <MenuItem value={3}>P3</MenuItem>
            </TextField>
          </Box>
          <Typography variant="caption" color="text.secondary">
            发起后直接进入待办，调度器认领后按依赖关系自动推进（可并行）；全部节点完成后进入审核。
          </Typography>
        </Stack>
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2 }}>
        <Button onClick={onClose}>取消</Button>
        <Button variant="contained" disabled={busy || !title.trim() || missing} onClick={() => void start()}>发起</Button>
      </DialogActions>
    </Dialog>
  )
}