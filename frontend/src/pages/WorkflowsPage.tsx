import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Box, Typography, Button, Card, CardContent, Chip, Stack, Dialog, DialogTitle, DialogContent,
  DialogActions, TextField, MenuItem, IconButton, Alert, Divider, Tooltip, Paper,
} from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import EditIcon from '@mui/icons-material/Edit'
import PlayArrowIcon from '@mui/icons-material/PlayArrow'
import AccountTreeIcon from '@mui/icons-material/AccountTree'
import { get, post, patch, del, parseJson, type Workflow, type WorkflowStage, type Agent, type Project } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton, EmptyState, useToast } from '../ui'
import WorkflowCanvas, { normalizeStages, reaches } from './WorkflowCanvas'

export default function WorkflowsPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const qc = useQueryClient()
  const toast = useToast()
  const isAdmin = role === 'owner' || role === 'admin'
  const [editing, setEditing] = useState<Workflow | 'new' | null>(null)
  const [running, setRunning] = useState<Workflow | null>(null)
  const invalidate = () => qc.invalidateQueries({ queryKey: ['workflows', wsId] })

  const { data: workflows = [] } = useQuery({ queryKey: ['workflows', wsId], queryFn: () => get<Workflow[]>(`/workspaces/${wsId}/workflows`) })
  const { data: agents = [] } = useQuery({ queryKey: ['agents', wsId], queryFn: () => get<Agent[]>(`/workspaces/${wsId}/agents`) })
  const { data: projects = [] } = useQuery({ queryKey: ['projects', wsId], queryFn: () => get<Project[]>(`/workspaces/${wsId}/projects`) })

  return (
    <Box sx={{ p: { xs: 2, md: 3.5 }, overflowY: 'auto', height: '100%', width: '100%', maxWidth: 1500, mx: 'auto' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, mb: 0.5 }}>
        <AccountTreeIcon sx={{ color: 'primary.main' }} />
        <Typography variant="h6" fontWeight={700} sx={{ flexGrow: 1 }}>工作流</Typography>
        {isAdmin && <Button variant="contained" startIcon={<AddIcon />} onClick={() => setEditing('new')}>新建工作流</Button>}
      </Box>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 3 }}>
        把标准做法固化成节点 DAG：每个节点绑定智能体与项目，依赖满足即自动并行推进；产物通过 git 分支交接。
      </Typography>

      {workflows.length === 0 && (
        <EmptyState icon={<AccountTreeIcon />} title="还没有工作流"
          desc="创建一个模板，把「设计 proto → 前后端并行开发 → 联调」这类流程固定下来。"
          action={isAdmin ? <Button variant="contained" onClick={() => setEditing('new')}>新建工作流</Button> : undefined} />
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
                  {isAdmin && <Tooltip title="编辑"><IconButton size="small" onClick={() => setEditing(wf)}><EditIcon fontSize="small" /></IconButton></Tooltip>}
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

      {editing && <WorkflowDialog data={editing === 'new' ? null : editing} wsId={wsId} agents={agents} projects={projects}
        onClose={() => setEditing(null)} onSaved={() => { setEditing(null); invalidate() }} />}
      {running && <RunDialog wf={running} wsId={wsId} projects={projects}
        onClose={() => setRunning(null)} onCreated={() => { setRunning(null); toast('已发起，等待调度器认领', 'success') }} />}
    </Box>
  )
}

function WorkflowDialog({ data, wsId, agents, projects, onClose, onSaved }: {
  data: Workflow | null; wsId: string; agents: Agent[]; projects: Project[]; onClose: () => void; onSaved: () => void
}) {
  const [name, setName] = useState(data?.name || '')
  const [description, setDescription] = useState(data?.description || '')
  const [stages, setStages] = useState<WorkflowStage[]>(() => {
    if (data) return normalizeStages(parseJson<WorkflowStage[]>(data.stages, []))
    return [{ key: 'node-1', name: '节点 1', agentId: agents[0]?.id || '', dependsOn: [], x: 48, y: 48 }]
  })
  const [selected, setSelected] = useState<string | null>(null)
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)

  const update = (key: string, patchStage: Partial<WorkflowStage>) => {
    setStages(prev => prev.map(s => s.key === key ? { ...s, ...patchStage } : s))
  }
  const renameKey = (oldKey: string, newKey: string) => {
    setStages(prev => prev.map(s => ({
      ...s,
      key: s.key === oldKey ? newKey : s.key,
      dependsOn: s.dependsOn.map(d => d === oldKey ? newKey : d),
    })))
    setSelected(newKey)
  }
  const removeNode = (key: string) => {
    setStages(prev => prev.filter(s => s.key !== key).map(s => ({ ...s, dependsOn: s.dependsOn.filter(d => d !== key) })))
    setSelected(null)
  }
  const save = async () => {
    setBusy(true); setErr('')
    try {
      const body = { name, description, stages: normalizeStages(stages) }
      if (data) await patch(`/workflows/${data.id}`, body)
      else await post(`/workspaces/${wsId}/workflows`, body)
      onSaved()
    } catch (e: any) { setErr(e.message) } finally { setBusy(false) }
  }

  const sel = stages.find(s => s.key === selected) || null
  const upstreamOptions = sel
    ? stages.filter(x => x.key !== sel.key && !sel.dependsOn.includes(x.key) && !reaches(stages, x.key, sel.key))
    : []
  const issues: string[] = []
  const keyCount = new Map<string, number>()
  for (const s of stages) keyCount.set(s.key, (keyCount.get(s.key) || 0) + 1)
  for (const s of stages) {
    if (!/^[a-z0-9-]{1,40}$/.test(s.key)) issues.push(`节点「${s.name || s.key}」的 key 不合法（小写字母/数字/连字符）`)
    else if ((keyCount.get(s.key) || 0) > 1) issues.push(`key 重复：${s.key}`)
    if (!s.agentId) issues.push(`节点「${s.name || s.key}」未绑定智能体`)
  }
  if (stages.length === 0) issues.push('工作流至少需要一个节点')

  return (
    <Dialog open onClose={onClose} fullWidth maxWidth="lg">
      <DialogTitle>{data ? '编辑工作流' : '新建工作流'}</DialogTitle>
      <DialogContent sx={{ px: 0, pb: 0 }}>
        {err && <Alert severity="error" sx={{ mx: 3, mb: 1 }}>{err}</Alert>}
        {issues.length > 0 && <Alert severity="warning" sx={{ mx: 3, mb: 1 }}>{issues.join('；')}</Alert>}
        <Box sx={{ px: 3, display: 'grid', gap: 1.5, gridTemplateColumns: { xs: '1fr', sm: '2fr 3fr' } }}>
          <TextField size="small" label="名称" value={name} onChange={e => setName(e.target.value)} required />
          <TextField size="small" label="说明" value={description} onChange={e => setDescription(e.target.value)} />
        </Box>
        <Box sx={{
          display: 'grid', mt: 2, borderTop: '1px solid', borderColor: 'divider', height: 520,
          gridTemplateColumns: { xs: '1fr', md: 'minmax(0,1fr) 340px' },
        }}>
          <WorkflowCanvas stages={stages} agents={agents} projects={projects}
            selected={selected} onChange={setStages} onSelect={setSelected} />
          <Box sx={{ borderLeft: { md: '1px solid' }, borderColor: 'divider', p: 2, overflowY: 'auto', bgcolor: '#fff' }}>
            {!sel && (
              <Stack spacing={1.5} sx={{ color: 'text.secondary' }}>
                <Typography variant="subtitle2" color="text.primary">画布</Typography>
                <Typography variant="body2">拖动节点摆位置；点「连线」后依次点两个节点建立依赖（先 → 后）。</Typography>
                <Typography variant="body2">依赖满足的节点会<strong>自动并行</strong>执行，无需配置并行度。</Typography>
                <Typography variant="caption">点击节点编辑智能体、项目、跳过条件与提示词。</Typography>
              </Stack>
            )}
            {sel && (
              <Stack spacing={2}>
                <Typography variant="subtitle2">节点属性</Typography>
                <TextField size="small" label="key" value={sel.key} onChange={e => renameKey(sel.key, e.target.value)}
                  helperText="小写字母 / 数字 / 连字符" error={!/^[a-z0-9-]{1,40}$/.test(sel.key)} />
                <TextField size="small" label="名称" value={sel.name} onChange={e => update(sel.key, { name: e.target.value })} />
                <TextField size="small" select label="智能体" value={sel.agentId} onChange={e => update(sel.key, { agentId: e.target.value })}>
                  {agents.map(a => <MenuItem key={a.id} value={a.id}>{a.name}</MenuItem>)}
                </TextField>
                <TextField size="small" select label="执行项目" value={sel.projectId || ''}
                  onChange={e => update(sel.key, { projectId: e.target.value || undefined })}>
                  <MenuItem value="">继承任务主项目</MenuItem>
                  {projects.map(p => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
                </TextField>
                <TextField size="small" label="跳过条件（tag:x / path:x / var:k=v）" value={sel.when || ''}
                  onChange={e => update(sel.key, { when: e.target.value || undefined })} />
                <TextField size="small" label="追加提示词" value={sel.promptAppend || ''} multiline minRows={3}
                  onChange={e => update(sel.key, { promptAppend: e.target.value || undefined })} />
                <Divider />
                <Typography variant="caption" color="text.secondary">上游依赖</Typography>
                <Box sx={{ display: 'flex', gap: 0.5, flexWrap: 'wrap' }}>
                  {sel.dependsOn.length === 0 && <Typography variant="caption" color="text.secondary">入口节点（无依赖）</Typography>}
                  {sel.dependsOn.map(d => (
                    <Chip key={d} size="small" label={d} onDelete={() => update(sel.key, { dependsOn: sel.dependsOn.filter(x => x !== d) })} />
                  ))}
                </Box>
                <TextField size="small" select label="添加依赖" value="" onChange={e => {
                  const v = e.target.value
                  if (v) update(sel.key, { dependsOn: [...sel.dependsOn, v] })
                }}>
                  <MenuItem value="">选择上游节点…</MenuItem>
                  {upstreamOptions.map(x => <MenuItem key={x.key} value={x.key}>{x.name || x.key}</MenuItem>)}
                </TextField>
                <Divider />
                <Button size="small" color="error" disabled={stages.length <= 1} onClick={() => removeNode(sel.key)}>删除节点</Button>
              </Stack>
            )}
          </Box>
        </Box>
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2 }}>
        <Button onClick={onClose}>取消</Button>
        <Button variant="contained" disabled={busy || !name.trim() || issues.length > 0} onClick={() => void save()}>保存</Button>
      </DialogActions>
    </Dialog>
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