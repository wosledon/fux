import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Table, TableHead, TableRow, TableCell, TableBody, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, MenuItem, Switch, FormControlLabel, Checkbox, Alert, IconButton, Tooltip, FormGroup } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import EditIcon from '@mui/icons-material/Edit'
import DeleteIcon from '@mui/icons-material/Delete'
import { get, post, patch, del, parseJson, type Agent, type CliEntry, type Skill, type McpServer } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton } from '../ui'

const EFFORTS = ['minimal', 'low', 'medium', 'high']
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
  const [err, setErr] = useState('')

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
    <Dialog open onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>{agent ? '编辑智能体' : '新建智能体'}</DialogTitle>
      <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
        <TextField label="名称 *" value={name} onChange={e => setName(e.target.value)} />
        <TextField select label="CLI 绑定 *" value={cliEntryId} onChange={e => setCliEntryId(e.target.value)}>
          {clis.map(c => <MenuItem key={c.id} value={c.id}>{c.name}（{c.command}）</MenuItem>)}
        </TextField>
        <TextField label="模型（留空用 CLI 默认）" value={model} onChange={e => setModel(e.target.value)} placeholder="如 anthropic/claude-sonnet-4-5" />
        <TextField select label="推理强度" value={effort} onChange={e => setEffort(e.target.value)}>
          {EFFORTS.map(x => <MenuItem key={x} value={x}>{x}</MenuItem>)}
        </TextField>
        <TextField label="系统提示词（角色/working agreement）" value={systemPrompt} onChange={e => setSystemPrompt(e.target.value)} multiline minRows={3} />
        <TextField select label="工具审批策略" value={policy} onChange={e => setPolicy(e.target.value)}>
          {POLICIES.map(p => <MenuItem key={p.v} value={p.v}>{p.label}</MenuItem>)}
        </TextField>
        <TextField label="最大并发任务" type="number" value={maxConcurrent} onChange={e => setMaxConcurrent(Number(e.target.value))} />
        <TextField label="匹配标签（逗号分隔，空=匹配所有任务）" value={matchTags} onChange={e => setMatchTags(e.target.value)} />
        <TextField label="质量阀覆盖（JSON，可空）" value={gates} onChange={e => setGates(e.target.value)} multiline minRows={2}
          placeholder='{"loopWindow":5,"silenceSecs":120,"maxTurns":200,"maxWallSecs":3600}' />
        <Box>
          <Typography variant="subtitle2">SKILL 授权</Typography>
          <FormGroup row>
            {skills.map(s => <FormControlLabel key={s.id} control={<Checkbox size="small" checked={skillIds.includes(s.id)}
              onChange={e => setSkillIds(ids => e.target.checked ? [...ids, s.id] : ids.filter(x => x !== s.id))} />} label={s.name} />)}
            {skills.length === 0 && <Typography variant="caption" color="text.secondary">暂无 SKILL</Typography>}
          </FormGroup>
        </Box>
        <Box>
          <Typography variant="subtitle2">MCP 授权</Typography>
          <FormGroup row>
            {mcps.map(m => <FormControlLabel key={m.id} control={<Checkbox size="small" checked={mcpIds.includes(m.id)}
              onChange={e => setMcpIds(ids => e.target.checked ? [...ids, m.id] : ids.filter(x => x !== m.id))} />} label={m.name} />)}
            {mcps.length === 0 && <Typography variant="caption" color="text.secondary">暂无 MCP 服务</Typography>}
          </FormGroup>
        </Box>
        {err && <Alert severity="error">{err}</Alert>}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>取消</Button>
        <Button variant="contained" onClick={save} disabled={!name.trim() || !cliEntryId}>保存</Button>
      </DialogActions>
    </Dialog>
  )
}