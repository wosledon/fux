import { useState } from 'react'
import { useNavigate, useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Table, TableHead, TableRow, TableCell, TableBody, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, MenuItem, Switch, FormControlLabel, Checkbox, Alert, IconButton, Tooltip, FormGroup, Autocomplete, CircularProgress, Divider } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import EditIcon from '@mui/icons-material/Edit'
import RefreshIcon from '@mui/icons-material/Refresh'
import OpenInNewIcon from '@mui/icons-material/OpenInNew'
import { get, post, patch, del, parseJson, type Agent, type CliEntry, type Skill, type McpServer } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton } from '../ui'

const EFFORTS = ['minimal', 'low', 'medium', 'high']

interface ModelChoice { id: string; name: string; description?: string | null }
interface ModelCatalog { models: ModelChoice[]; currentModelId?: string | null; efforts: ModelChoice[]; currentEffortId?: string | null }
interface ModelsProbe { available: boolean; catalog?: ModelCatalog; error?: string }
const POLICIES = [
  { v: 'read_auto', label: '只读自动（写操作需审批）' },
  { v: 'write_ask', label: '写操作需审批' },
  { v: 'full_auto', label: '全自动' },
]

export default function AgentsPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const qc = useQueryClient()
  const isAdmin = role === 'owner' || role === 'admin'
  const { data: agents = [] } = useQuery({ queryKey: ['agents', wsId], queryFn: () => get<Agent[]>(`/workspaces/${wsId}/agents`) })
  const { data: clis = [] } = useQuery({ queryKey: ['clis', wsId], queryFn: () => get<CliEntry[]>(`/cli-entries?workspaceId=${wsId}`) })
  const { data: skills = [] } = useQuery({ queryKey: ['skills', wsId], queryFn: () => get<Skill[]>(`/workspaces/${wsId}/skills`) })
  const { data: mcps = [] } = useQuery({ queryKey: ['mcps', wsId], queryFn: () => get<McpServer[]>(`/workspaces/${wsId}/mcp-servers`) })
  const [editing, setEditing] = useState<Agent | 'new' | null>(null)
  const [err, setErr] = useState('')

  const invalidate = () => qc.invalidateQueries({ queryKey: ['agents', wsId] })

  return (
    <Box sx={{ p: { xs: 2, md: 3.5 }, overflowY: 'auto', height: '100%', width: '100%', maxWidth: 1500, mx: 'auto' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', mb: 2 }}>
        <Typography variant="h6" fontWeight={700} sx={{ flexGrow: 1 }}>智能体</Typography>
        {isAdmin && <Button variant="contained" startIcon={<AddIcon />} onClick={() => setEditing('new')}>新建智能体</Button>}
      </Box>
      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 2 }}>
        一切智能体 = ACP CLI 绑定 + 模型 + 推理强度（无内置 loop）。首批支持 Codex / OpenCode / Pi / Copilot CLI。
      </Typography>
      {err && <Alert severity="error" sx={{ mb: 2 }} onClose={() => setErr('')}>{err}</Alert>}
      <Table size="small">
        <TableHead><TableRow>
          <TableCell>名称</TableCell><TableCell>CLI</TableCell><TableCell>模型</TableCell><TableCell>推理强度</TableCell><TableCell>审批策略</TableCell><TableCell>并发</TableCell><TableCell>暂停</TableCell><TableCell />
        </TableRow></TableHead>
        <TableBody>
          {agents.map(a => (
            <TableRow key={a.id} hover>
              <TableCell>{a.name}</TableCell>
              <TableCell><Chip label={clis.find(c => c.id === a.cliEntryId)?.name || a.cliEntryId} variant="outlined" /></TableCell>
              <TableCell>{a.model || '默认'}</TableCell>
              <TableCell>{a.reasoningEffort}</TableCell>
              <TableCell>{POLICIES.find(p => p.v === a.policy)?.label || a.policy}</TableCell>
              <TableCell>{a.maxConcurrent}</TableCell>
              <TableCell>
                <Switch size="small" checked={a.paused === 1} disabled={!isAdmin}
                  onChange={async e => { await post(`/agents/${a.id}/pause`, { paused: e.target.checked }); invalidate() }} />
              </TableCell>
              <TableCell align="right">
                {isAdmin && <>
                  <Tooltip title="编辑"><IconButton size="small" onClick={() => setEditing(a)}><EditIcon fontSize="small" /></IconButton></Tooltip>
                  <DeleteButton title="删除智能体" onDelete={() => del(`/agents/${a.id}`)} />
                </>}
              </TableCell>
            </TableRow>
          ))}
          {agents.length === 0 && <TableRow><TableCell colSpan={8}><Typography color="text.secondary">还没有智能体</Typography></TableCell></TableRow>}
        </TableBody>
      </Table>
      {editing && <AgentDialog agent={editing === 'new' ? null : editing} wsId={wsId} clis={clis} skills={skills} mcps={mcps}
        onClose={() => setEditing(null)} onSaved={() => { invalidate(); setEditing(null) }} />}
    </Box>
  )
}

function AgentDialog({ agent, wsId, clis, skills, mcps, onClose, onSaved }: {
  agent: Agent | null; wsId: string; clis: CliEntry[]; skills: Skill[]; mcps: McpServer[]; onClose: () => void; onSaved: () => void
}) {
  const nav = useNavigate()
  const [name, setName] = useState(agent?.name || '')
  const [cliEntryId, setCliEntryId] = useState(agent?.cliEntryId || clis[0]?.id || '')
  const [model, setModel] = useState(agent?.model || '')
  const [effort, setEffort] = useState(agent?.reasoningEffort || 'medium')
  const [systemPrompt, setSystemPrompt] = useState(agent?.systemPrompt || '')
  const [policy, setPolicy] = useState(agent?.policy || 'write_ask')
  const [maxConcurrent, setMaxConcurrent] = useState(agent?.maxConcurrent || 1)
  const [matchTags, setMatchTags] = useState(parseJson<string[]>(agent?.matchTags, []).join(','))
  const [skillIds, setSkillIds] = useState<string[]>(agent?.skillIds || [])
  const [mcpIds, setMcpIds] = useState<string[]>((agent?.mcpServers || []).map(m => m.id))
  const [gates, setGates] = useState(agent?.gates && agent.gates !== '{}' ? agent.gates : '')
  const [models, setModels] = useState<ModelChoice[]>([])
  const [efforts, setEfforts] = useState<ModelChoice[]>([])
  const [modelsLoading, setModelsLoading] = useState(false)
  const [modelsMsg, setModelsMsg] = useState('')
  const [err, setErr] = useState('')

  const loadModels = async () => {
    if (!cliEntryId) return
    setModelsLoading(true)
    setModelsMsg('')
    try {
      const r = await post<ModelsProbe>(`/cli-entries/${cliEntryId}/models`)
      if (r.available) {
        const found = r.catalog?.models || []
        setModels(found)
        setEfforts(r.catalog?.efforts || [])
        setModelsMsg(found.length > 0
          ? `已从 ACP 读取 ${found.length} 个模型${r.catalog?.currentModelId ? `，当前默认 ${r.catalog.currentModelId}` : ''}`
          : '该 CLI 未声明模型列表，可继续手动填写')
        if (!model && r.catalog?.currentModelId) setModel(r.catalog.currentModelId)
      } else {
        setModels([])
        setEfforts([])
        setModelsMsg(r.error || '无法读取模型')
      }
    } catch (e: any) {
      setModelsMsg(e.message)
    } finally {
      setModelsLoading(false)
    }
  }
  const effortOptions = Array.from(new Set([...EFFORTS, ...efforts.map(e => e.id)]))

  const save = async () => {
    setErr('')
    try {
      const body = {
        name, cliEntryId, model, reasoningEffort: effort, systemPrompt, policy, maxConcurrent,
        matchTags: matchTags.split(',').map(s => s.trim()).filter(Boolean),
        skillIds,
        mcpServers: mcpIds.map(id => ({ id, toolWhitelist: [] })),
        gates: gates ? JSON.parse(gates) : {},
      }
      if (agent) await patch(`/agents/${agent.id}`, body)
      else await post(`/workspaces/${wsId}/agents`, body)
      onSaved()
    } catch (e: any) { setErr(e.message) }
  }

  return (
    <Dialog open onClose={onClose} fullWidth maxWidth="md">
      <DialogTitle>{agent ? '编辑智能体' : '新建智能体'}</DialogTitle>
      <DialogContent sx={{ pt: '12px !important' }}>
        <Box sx={{ display: 'grid', gap: 2, gridTemplateColumns: { xs: '1fr', md: 'minmax(0,1fr) minmax(0,1fr)' } }}>
          <Box sx={{ display: 'grid', gap: 2, alignContent: 'start' }}>
            <TextField label="名称 *" value={name} onChange={e => setName(e.target.value)} />
            <TextField select label="CLI 绑定 *" value={cliEntryId} onChange={e => { setCliEntryId(e.target.value); setModels([]); setEfforts([]); setModelsMsg('') }}>
              {clis.map(c => <MenuItem key={c.id} value={c.id}>{c.name}（{c.command}）</MenuItem>)}
            </TextField>
            <Box sx={{ display: 'flex', gap: 1, alignItems: 'flex-start' }}>
              <Autocomplete
                fullWidth freeSolo size="small"
                options={models.map(m => m.id)}
                value={model}
                onChange={(_, v) => setModel(v || '')}
                onInputChange={(_, v) => setModel(v)}
                renderOption={({ key, ...props }, option) => {
                  const m = models.find(x => x.id === option)
                  return (
                    <li key={key} {...props}>
                      <Box>
                        <Typography variant="body2">{m?.name || option}</Typography>
                        {m?.name && m.name !== option && (
                          <Typography variant="caption" color="text.secondary" display="block">{option}</Typography>
                        )}
                      </Box>
                    </li>
                  )
                }}
                renderInput={params => (
                  <TextField {...params} label="模型" placeholder="留空用 CLI 默认"
                    helperText={modelsMsg || '可手动填写，或从 ACP 读取该 CLI 的模型列表'} />
                )}
              />
              <Button variant="outlined" size="small" onClick={() => void loadModels()}
                disabled={modelsLoading || !cliEntryId}
                startIcon={modelsLoading ? <CircularProgress size={14} /> : <RefreshIcon />}
                sx={{ mt: 0.25, whiteSpace: 'nowrap' }}>
                {modelsLoading ? '读取中…' : '读取 ACP 模型'}
              </Button>
            </Box>
            <TextField select label="推理强度" value={effort} onChange={e => setEffort(e.target.value)}>
              {effortOptions.map(x => <MenuItem key={x} value={x}>{x}</MenuItem>)}
            </TextField>
            <TextField label="系统提示词（角色/working agreement）" value={systemPrompt} onChange={e => setSystemPrompt(e.target.value)} multiline minRows={3} />
            <TextField select label="工具审批策略" value={policy} onChange={e => setPolicy(e.target.value)}>
              {POLICIES.map(p => <MenuItem key={p.v} value={p.v}>{p.label}</MenuItem>)}
            </TextField>
            <TextField label="最大并发任务" type="number" value={maxConcurrent} onChange={e => setMaxConcurrent(Number(e.target.value))} />
            <TextField label="匹配标签（逗号分隔，空=匹配所有任务）" value={matchTags} onChange={e => setMatchTags(e.target.value)} />
          </Box>

          <Box sx={{ display: 'grid', gap: 2, alignContent: 'start' }}>
            <TextField label="质量阀覆盖（JSON，可空）" value={gates} onChange={e => setGates(e.target.value)} multiline minRows={2}
              placeholder='{"loopWindow":5,"silenceSecs":120,"maxTurns":200,"maxWallSecs":3600}' />
            <Divider />
            <Box>
              <Typography variant="subtitle2">SKILL 授权（已选 {skillIds.length} / 共 {skills.length}）</Typography>
              <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1 }}>
                授权后运行时写入会话目录 .fux/skills/ 并在提示词中引用。
              </Typography>
              <FormGroup>
                {skills.map(s => <FormControlLabel key={s.id} control={<Checkbox size="small" checked={skillIds.includes(s.id)}
                  onChange={e => setSkillIds(ids => e.target.checked ? [...ids, s.id] : ids.filter(x => x !== s.id))} />} label={s.name} />)}
                {skills.length === 0 && (
                  <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                    <Typography variant="caption" color="text.secondary">SKILL 库为空</Typography>
                    <Button size="small" endIcon={<OpenInNewIcon sx={{ fontSize: 14 }} />}
                      onClick={() => { onClose(); nav(`/ws/${wsId}/skills`) }}>去导入 SKILL</Button>
                  </Box>
                )}
              </FormGroup>
            </Box>
            <Box>
              <Typography variant="subtitle2">MCP 授权（已选 {mcpIds.length} / 共 {mcps.length}）</Typography>
              <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 1 }}>
                授权后经 ACP session/new 透传给 CLI，密钥在运行时解密注入。
              </Typography>
              <FormGroup>
                {mcps.map(m => <FormControlLabel key={m.id} control={<Checkbox size="small" checked={mcpIds.includes(m.id)}
                  onChange={e => setMcpIds(ids => e.target.checked ? [...ids, m.id] : ids.filter(x => x !== m.id))} />} label={m.name} />)}
                {mcps.length === 0 && (
                  <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                    <Typography variant="caption" color="text.secondary">未注册 MCP 服务</Typography>
                    <Button size="small" endIcon={<OpenInNewIcon sx={{ fontSize: 14 }} />}
                      onClick={() => { onClose(); nav(`/ws/${wsId}/mcp`) }}>去注册 MCP</Button>
                  </Box>
                )}
              </FormGroup>
            </Box>
            {err && <Alert severity="error">{err}</Alert>}
          </Box>
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>取消</Button>
        <Button variant="contained" onClick={save} disabled={!name.trim() || !cliEntryId}>保存</Button>
      </DialogActions>
    </Dialog>
  )
}