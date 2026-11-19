import { useEffect, useRef, useState } from 'react'
import { useNavigate, useOutletContext, useParams } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Alert, Box, Button, Chip, CircularProgress, Divider, IconButton, MenuItem, Stack,
  TextField, Tooltip, Typography,
} from '@mui/material'
import ArrowBackIcon from '@mui/icons-material/ArrowBack'
import SaveOutlinedIcon from '@mui/icons-material/SaveOutlined'
import AddIcon from '@mui/icons-material/Add'
import { get, post, patch, parseJson, type Workflow, type WorkflowStage, type Agent, type Project } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { useConfirm, useToast } from '../ui'
import WorkflowCanvasEditor from './WorkflowCanvasEditor'
import { normalizeStages, reaches } from './WorkflowCanvas'

const snap = (name: string, description: string, stages: WorkflowStage[]) =>
  JSON.stringify({ name, description, stages: normalizeStages(stages) })

export default function WorkflowEditorPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const { wfId } = useParams()
  const isNew = !wfId
  const isAdmin = role === 'owner' || role === 'admin'
  const nav = useNavigate()
  const qc = useQueryClient()
  const toast = useToast()
  const confirm = useConfirm()

  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [stages, setStages] = useState<WorkflowStage[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [initializedFor, setInitializedFor] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const baselineRef = useRef('')

  const { data: workflow, isLoading: wfLoading } = useQuery({
    queryKey: ['workflow', wfId],
    queryFn: () => get<Workflow>(`/workflows/${wfId}`),
    enabled: !!wfId,
  })
  const { data: agents = [], isFetched: agentsFetched } = useQuery({
    queryKey: ['agents', wsId],
    queryFn: () => get<Agent[]>(`/workspaces/${wsId}/agents`),
  })
  const { data: projects = [] } = useQuery({
    queryKey: ['projects', wsId],
    queryFn: () => get<Project[]>(`/workspaces/${wsId}/projects`),
  })

  const editorKey = wfId ?? 'new'

  useEffect(() => {
    if (initializedFor === editorKey) return
    if (!isNew && !workflow) return
    if (isNew && !agentsFetched) return
    if (isNew && agents.length === 0) return
    const initName = isNew ? '' : (workflow?.name || '')
    const initDesc = isNew ? '' : (workflow?.description || '')
    const initStages: WorkflowStage[] = isNew
      ? (agents.length > 0
          ? [{ key: 'node-1', name: '节点 1', agentId: agents[0].id, dependsOn: [], x: 48, y: 48 }]
          : [])
      : normalizeStages(parseJson<WorkflowStage[]>(workflow?.stages, []))
    setName(initName)
    setDescription(initDesc)
    setStages(initStages)
    setSelected(null)
    baselineRef.current = snap(initName, initDesc, initStages)
    setInitializedFor(editorKey)
  }, [initializedFor, editorKey, isNew, workflow, agents, agentsFetched])

  const sel = stages.find(s => s.key === selected) || null
  const dirty = initializedFor === editorKey && snap(name, description, stages) !== baselineRef.current

  const issues: string[] = []
  const keyCount = new Map<string, number>()
  for (const s of stages) keyCount.set(s.key, (keyCount.get(s.key) || 0) + 1)
  for (const s of stages) {
    if (!/^[a-z0-9-]{1,40}$/.test(s.key)) issues.push(`节点「${s.name || s.key}」的 key 不合法（小写字母/数字/连字符）`)
    else if ((keyCount.get(s.key) || 0) > 1) issues.push(`key 重复：${s.key}`)
    if (!s.agentId) issues.push(`节点「${s.name || s.key}」未绑定智能体`)
  }
  if (stages.length === 0) issues.push('工作流至少需要一个节点')

  const update = (key: string, patchStage: Partial<WorkflowStage>) => {
    setStages(prev => prev.map(s => s.key === key ? { ...s, ...patchStage } : s))
  }
  const renameKey = (oldKey: string, newKey: string) => {
    if (newKey !== oldKey && stages.some(s => s.key === newKey)) return
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

  const goBack = async () => {
    if (dirty) {
      const ok = await confirm({ title: '放弃未保存的修改？', content: '当前工作流的修改尚未保存。', danger: true, okText: '放弃' })
      if (!ok) return
    }
    nav(`/ws/${wsId}/workflows`)
  }

  const save = async () => {
    if (issues.length > 0) return
    setBusy(true)
    setErr('')
    try {
      const body = { name: name.trim(), description, stages: normalizeStages(stages) }
      if (isNew) await post(`/workspaces/${wsId}/workflows`, body)
      else await patch(`/workflows/${wfId}`, body)
      await qc.invalidateQueries({ queryKey: ['workflows', wsId] })
      toast('工作流已保存', 'success')
      nav(`/ws/${wsId}/workflows`)
    } catch (e: any) {
      setErr(e.message)
    } finally {
      setBusy(false)
    }
  }

  if (!isAdmin) {
    return (
      <Box sx={{ p: 3.5 }}>
        <Alert severity="warning" action={<Button onClick={() => nav(`/ws/${wsId}/workflows`)}>返回工作流</Button>}>
          只有工作区管理员可以编辑工作流。
        </Alert>
      </Box>
    )
  }

  if (!isNew && wfLoading) {
    return <Box sx={{ height: '100%', display: 'grid', placeItems: 'center' }}><CircularProgress size={28} /></Box>
  }

  if (isNew && agentsFetched && agents.length === 0) {
    return (
      <Box sx={{ p: 3.5 }}>
        <Alert severity="warning" action={<Button onClick={() => nav(`/ws/${wsId}/agents`)}>去创建智能体</Button>}>
          工作流节点必须绑定智能体，请先在工作区创建至少一个智能体。
        </Alert>
      </Box>
    )
  }

  const upstreamOptions = sel
    ? stages.filter(x => x.key !== sel.key && !sel.dependsOn.includes(x.key) && !reaches(stages, x.key, sel.key))
    : []

  return (
    <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column', bgcolor: '#f7f8fb', minHeight: 0 }}>
      <Box sx={{
        px: { xs: 2, md: 2.5 }, py: 1.25, bgcolor: '#fff', borderBottom: '1px solid', borderColor: 'divider',
        display: 'flex', alignItems: 'center', gap: 1.25, flexWrap: 'wrap', flexShrink: 0,
      }}>
        <Tooltip title="返回工作流">
          <IconButton size="small" onClick={() => void goBack()}><ArrowBackIcon fontSize="small" /></IconButton>
        </Tooltip>
        <Typography variant="subtitle1" fontWeight={700} sx={{ mr: 1 }}>
          {isNew ? '新建工作流' : '编辑工作流'}
        </Typography>
        <TextField size="small" label="名称" value={name} onChange={e => setName(e.target.value)}
          required sx={{ width: { xs: '100%', sm: 260 } }} />
        <TextField size="small" label="说明" value={description} onChange={e => setDescription(e.target.value)}
          sx={{ flexGrow: 1, minWidth: { xs: '100%', sm: 240 } }} />
        <Chip size="small" variant="outlined" label={`${stages.length} 节点`} />
        {dirty && <Chip size="small" color="warning" label="未保存" />}
        <Button variant="contained" startIcon={<SaveOutlinedIcon />} onClick={() => void save()}
          disabled={busy || !name.trim() || issues.length > 0}>
          保存
        </Button>
      </Box>

      {err && <Alert severity="error" sx={{ borderRadius: 0 }}>{err}</Alert>}
      {issues.length > 0 && <Alert severity="warning" sx={{ borderRadius: 0 }}>{issues.join('；')}</Alert>}

      <Box sx={{
        flexGrow: 1, minHeight: 0, display: 'grid',
        gridTemplateColumns: { xs: '1fr', md: 'minmax(0,1fr) 360px' },
        gridTemplateRows: { xs: 'minmax(420px, 1fr) auto', md: 'minmax(0, 1fr)' },
      }}>
        <Box sx={{ minWidth: 0, minHeight: 0 }}>
          <WorkflowCanvasEditor key={editorKey} stages={stages} agents={agents} projects={projects}
            selected={selected} onChange={setStages} onSelect={setSelected} />
        </Box>

        <Box sx={{ borderLeft: { md: '1px solid' }, borderTop: { xs: '1px solid', md: 'none' }, borderColor: 'divider', bgcolor: '#fff', p: 2, overflowY: 'auto' }}>
          {!sel && (
            <Stack spacing={1.5} sx={{ color: 'text.secondary' }}>
              <Typography variant="subtitle2" color="text.primary" fontWeight={700}>画布</Typography>
              <Typography variant="body2">拖动节点摆位置；拖动画布空白处平移；滚轮缩放，右下角可适应画布。</Typography>
              <Typography variant="body2">点「连线」后依次点两个节点建立依赖（先 → 后），点击连线可删除。</Typography>
              <Typography variant="body2">依赖满足的节点会<strong>自动并行</strong>执行，无需配置并行度。</Typography>
              <Divider />
              <Typography variant="overline">节点列表</Typography>
              <Stack spacing={0.75}>
                {stages.map(s => (
                  <Box key={s.key} onClick={() => setSelected(s.key)} sx={{
                    px: 1.25, py: 0.9, borderRadius: 1.25, border: '1px solid', borderColor: 'divider',
                    cursor: 'pointer', '&:hover': { bgcolor: 'action.hover' },
                  }}>
                    <Typography variant="body2" fontWeight={600} noWrap>{s.name || s.key}</Typography>
                    <Typography variant="caption" color="text.secondary" noWrap display="block">
                      {s.key} · {agents.find(a => a.id === s.agentId)?.name || '未绑定'}
                    </Typography>
                  </Box>
                ))}
                {stages.length === 0 && <Typography variant="caption">还没有节点，点画布上方「添加节点」开始。</Typography>}
              </Stack>
              <Button size="small" startIcon={<AddIcon />} onClick={() => {
                const key = nextNodeKey(stages)
                setStages(prev => [...prev, {
                  key, name: `节点 ${prev.length + 1}`, agentId: agents[0]?.id || '', dependsOn: [], x: 48, y: 48,
                }])
                setSelected(key)
              }}>添加节点</Button>
            </Stack>
          )}

          {sel && (
            <Stack spacing={2}>
              <Typography variant="subtitle2" fontWeight={700}>节点属性</Typography>
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
    </Box>
  )
}

function nextNodeKey(stages: WorkflowStage[]): string {
  for (let i = 1; i < 999; i++) {
    const key = `node-${i}`
    if (!stages.some(s => s.key === key)) return key
  }
  return `node-${Date.now()}`
}