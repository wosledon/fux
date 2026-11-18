import { useMemo, useState } from 'react'
import { useNavigate, useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { DndContext, DragOverlay, PointerSensor, useSensor, useSensors, useDraggable, useDroppable, type DragEndEvent, type DragOverEvent, type DragStartEvent } from '@dnd-kit/core'
import { Box, Typography, Chip, Paper, Button, IconButton, Dialog, DialogTitle, DialogContent, DialogActions, TextField, MenuItem, Tooltip, InputAdornment, Divider } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import SearchIcon from '@mui/icons-material/Search'
import DragIndicatorIcon from '@mui/icons-material/DragIndicator'
import FlagIcon from '@mui/icons-material/Flag'
import MergeIcon from '@mui/icons-material/Merge'
import ErrorOutlineIcon from '@mui/icons-material/ErrorOutline'
import SmartToyIcon from '@mui/icons-material/SmartToy'
import InboxIcon from '@mui/icons-material/Inbox'
import RocketLaunchIcon from '@mui/icons-material/RocketLaunch'
import RateReviewIcon from '@mui/icons-material/RateReview'
import CheckCircleOutlineIcon from '@mui/icons-material/CheckCircleOutline'
import ArchiveIcon from '@mui/icons-material/Archive'
import { get, post, patch, parseJson, type Task, type Lane, type Board, type Project, type Agent, type Team, type TaskType } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { LANE_ACCENT } from '../theme'
import { PersonAvatar, useToast } from '../ui'

const LANE_HINT: Record<string, { icon: React.ReactNode; text: string }> = {
  planning: { icon: <InboxIcon />, text: '新任务在这里诞生，规划后拖入待办' },
  ready: { icon: <RocketLaunchIcon />, text: '智能体将自动认领这里的任务' },
  work: { icon: <SmartToyIcon />, text: '智能体正在执行，过程实时可见' },
  review: { icon: <RateReviewIcon />, text: '等待你审核 PR/MR 并批准' },
  blocked: { icon: <ErrorOutlineIcon />, text: '需要介入：评论解答或解除阻塞' },
  done: { icon: <CheckCircleOutlineIcon />, text: '已交付，可归档' },
  archived: { icon: <ArchiveIcon />, text: '近 7 天归档' },
}

type DropPlan = { ok: true; kind: 'unblock' } | { ok: true; kind: 'transition'; to: string } | { ok: true; kind: 'lane' } | { ok: false; reason: string }

function dropPlan(task: Task, lane: Lane): DropPlan {
  if (task.laneId === lane.id) return { ok: false, reason: '' }
  if (task.status === 'blocked' && ['ready', 'work', 'review'].includes(lane.kind)) return { ok: true, kind: 'unblock' }
  if (lane.kind === 'ready' && ['planning', 'review', 'done'].includes(task.status)) return { ok: true, kind: 'transition', to: 'ready' }
  if (lane.kind === 'done' && task.status === 'review') return { ok: true, kind: 'transition', to: 'done' }
  if (lane.kind === 'archived' && task.status === 'done') return { ok: true, kind: 'transition', to: 'archived' }
  if (lane.kind === 'work' && task.status === 'in_progress') return { ok: true, kind: 'lane' }
  const reasons: Record<string, string> = {
    work: '「进行中」由智能体自动认领进入', review: '「审核中」由智能体完成时进入',
    blocked: '「已阻塞」由智能体/系统标记', planning: '任务不能移回规划',
  }
  return { ok: false, reason: reasons[lane.kind] || '该状态转移不被允许' }
}

function CardBody({ task, agents, teams }: { task: Task; agents: Agent[]; teams: { team: Team }[] }) {
  const assignee = task.assigneeAgentId ? agents.find(a => a.id === task.assigneeAgentId)?.name
    : task.assigneeTeamId ? teams.find(t => t.team.id === task.assigneeTeamId)?.team.name : undefined
  const tags = parseJson<string[]>(task.tags, [])
  return (
    <>
      <Box sx={{ display: 'flex', gap: 0.5, alignItems: 'flex-start' }}>
        <DragIndicatorIcon sx={{ fontSize: 16, color: 'text.secondary', opacity: 0.4, mt: 0.25, flexShrink: 0 }} />
        <Typography variant="body2" fontWeight={600} sx={{ wordBreak: 'break-word', lineHeight: 1.45 }}>{task.title}</Typography>
      </Box>
      {(task.priority > 0 || task.prState || task.retryCount > 0 || tags.length > 0) && (
        <Box sx={{ display: 'flex', gap: 0.5, flexWrap: 'wrap', mt: 1, alignItems: 'center' }}>
          {task.priority > 0 && <Chip size="small" icon={<FlagIcon sx={{ fontSize: 12 }} />} label={`P${task.priority}`} color="warning" variant="outlined" />}
          {task.prState && <Chip size="small" icon={<MergeIcon sx={{ fontSize: 12 }} />}
            label={task.prState === 'merged' ? '已合并' : task.prState === 'local' ? '本地交付' : 'PR 待合并'}
            color={task.prState === 'merged' ? 'success' : 'default'} variant="outlined" />}
          {task.retryCount > 0 && <Chip size="small" label={`重试 ${task.retryCount}`} variant="outlined" />}
          {tags.map(t => <Chip key={t} size="small" label={t} variant="outlined" />)}
        </Box>
      )}
      {task.status === 'blocked' && task.blockReason && (
        <Box sx={{ mt: 1, px: 1, py: 0.5, borderRadius: 1, bgcolor: 'rgba(248,113,113,0.1)', border: '1px solid rgba(248,113,113,0.25)' }}>
          <Typography variant="caption" sx={{ color: 'error.light', display: 'block', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {task.blockReason}
          </Typography>
        </Box>
      )}
      {assignee && (
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75, mt: 1.25 }}>
          <PersonAvatar name={assignee} size={20} />
          <Typography variant="caption" color="text.secondary">{assignee}</Typography>
        </Box>
      )}
    </>
  )
}

function TaskCard({ task, agents, teams, onOpen }: { task: Task; agents: Agent[]; teams: { team: Team }[]; onOpen: () => void }) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({ id: task.id })
  return (
    <Paper ref={setNodeRef} {...listeners} {...attributes} onClick={onOpen}
      sx={{
        p: 1.75, mb: 1.25, cursor: 'grab', opacity: isDragging ? 0.35 : 1,
        bgcolor: '#171c27', border: '1px solid', borderColor: 'rgba(148,163,184,0.12)', borderRadius: 3,
        transition: 'border-color .15s, transform .15s, box-shadow .15s',
        '&:hover': { borderColor: 'rgba(139,157,255,0.55)', transform: 'translateY(-2px)', boxShadow: '0 10px 26px rgba(0,0,0,0.2)' },
        '&:active': { cursor: 'grabbing' },
      }}>
      <CardBody task={task} agents={agents} teams={teams} />
    </Paper>
  )
}

function LaneColumn({ lane, tasks, agents, teams, onOpen, onCreate, dropState }: {
  lane: Lane; tasks: Task[]; agents: Agent[]; teams: { team: Team }[]
  onOpen: (id: string) => void; onCreate: () => void; dropState: 'ok' | 'bad' | null
}) {
  const { setNodeRef } = useDroppable({ id: lane.id })
  const accent = LANE_ACCENT[lane.kind] || '#8b93a7'
  const hint = LANE_HINT[lane.kind]
  const overLimit = lane.wipLimit != null && tasks.length > lane.wipLimit
  return (
    <Box ref={setNodeRef} sx={{
      width: 312, minWidth: 312, display: 'flex', flexDirection: 'column', height: '100%',
      bgcolor: 'rgba(18,23,34,0.82)', borderRadius: 4, border: '1px solid',
      borderColor: dropState === 'ok' ? 'success.main' : dropState === 'bad' ? 'error.main' : 'divider',
      boxShadow: dropState === 'ok' ? '0 0 0 2px rgba(94,214,160,0.2)' : dropState === 'bad' ? '0 0 0 2px rgba(251,132,144,0.18)' : 'none',
      transition: 'border-color .15s, box-shadow .15s', overflow: 'hidden',
    }}>
      <Box sx={{ height: 3, bgcolor: accent, flexShrink: 0, opacity: 0.85 }} />
      <Box sx={{ px: 1.75, py: 1.5, display: 'flex', alignItems: 'center', gap: 1, borderBottom: '1px solid', borderColor: 'divider' }}>
        <Typography variant="subtitle2" sx={{ flexGrow: 1 }}>{lane.name}</Typography>
        {lane.stageRole && <Tooltip title={`阶段角色：${lane.stageRole}`}><Chip size="small" label={lane.stageRole} variant="outlined" /></Tooltip>}
        {lane.wipLimit != null && (
          <Tooltip title={`WIP 上限 ${lane.wipLimit}`}><Chip size="small" label={`${tasks.length}/${lane.wipLimit}`} color={overLimit ? 'error' : 'default'} variant="outlined" /></Tooltip>
        )}
        <Chip size="small" label={tasks.length} sx={{ bgcolor: `${accent}22`, color: accent, fontWeight: 700 }} />
        {['planning', 'ready'].includes(lane.kind) && (
          <Tooltip title="在此新建任务"><IconButton size="small" onClick={onCreate}><AddIcon fontSize="small" /></IconButton></Tooltip>
        )}
      </Box>
      <Box sx={{ p: 1, overflowY: 'auto', flexGrow: 1 }}>
        {tasks.map(t => <TaskCard key={t.id} task={t} agents={agents} teams={teams} onOpen={() => onOpen(t.id)} />)}
        {tasks.length === 0 && (
          <Box sx={{ textAlign: 'center', py: 4, px: 2, color: 'text.secondary' }}>
            <Box sx={{ opacity: 0.4, mb: 1 }}>{hint?.icon}</Box>
            <Typography variant="caption" display="block">{hint?.text || '空'}</Typography>
          </Box>
        )}
      </Box>
    </Box>
  )
}

export default function BoardPage() {
  const { wsId } = useOutletContext<WsCtx>()
  const nav = useNavigate()
  const qc = useQueryClient()
  const toast = useToast()
  const { data: boardData } = useQuery({ queryKey: ['board', wsId], queryFn: () => get<{ board: Board; lanes: Lane[] }>(`/workspaces/${wsId}/board`) })
  const boardId = boardData?.board.id
  const { data: tasks = [] } = useQuery({ queryKey: ['tasks', boardId], queryFn: () => get<Task[]>(`/tasks?boardId=${boardId}`), enabled: !!boardId })
  const { data: projects = [] } = useQuery({ queryKey: ['projects', wsId], queryFn: () => get<Project[]>(`/workspaces/${wsId}/projects`) })
  const { data: agents = [] } = useQuery({ queryKey: ['agents', wsId], queryFn: () => get<Agent[]>(`/workspaces/${wsId}/agents`) })
  const { data: teams = [] } = useQuery({ queryKey: ['teams', wsId], queryFn: () => get<{ team: Team; members: any[] }[]>(`/workspaces/${wsId}/teams`) })
  const { data: types = [] } = useQuery({ queryKey: ['types', wsId], queryFn: () => get<TaskType[]>(`/workspaces/${wsId}/task-types`) })

  const [createOpen, setCreateOpen] = useState(false)
  const [filterProject, setFilterProject] = useState('')
  const [search, setSearch] = useState('')
  const [activeTask, setActiveTask] = useState<Task | null>(null)
  const [hoverLane, setHoverLane] = useState<{ id: string; ok: boolean } | null>(null)
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 6 } }))

  const lanes = useMemo(() => (boardData?.lanes || []).slice().sort((a, b) => a.position - b.position), [boardData])
  const filtered = tasks.filter(t =>
    (!filterProject || t.projectId === filterProject) &&
    (!search || t.title.toLowerCase().includes(search.toLowerCase())))
  const stats = useMemo(() => ({
    doing: tasks.filter(t => t.status === 'in_progress').length,
    review: tasks.filter(t => t.status === 'review').length,
    blocked: tasks.filter(t => t.status === 'blocked').length,
  }), [tasks])

  const invalidate = () => { qc.invalidateQueries({ queryKey: ['tasks', boardId] }) }

  const onDragStart = (e: DragStartEvent) => setActiveTask(tasks.find(t => t.id === String(e.active.id)) || null)
  const onDragOver = (e: DragOverEvent) => {
    const laneId = e.over?.id ? String(e.over.id) : null
    if (!laneId || !activeTask) { setHoverLane(null); return }
    const lane = lanes.find(l => l.id === laneId)
    if (!lane) { setHoverLane(null); return }
    const plan = dropPlan(activeTask, lane)
    setHoverLane({ id: laneId, ok: plan.ok })
  }
  const onDragEnd = async (e: DragEndEvent) => {
    const task = activeTask
    setActiveTask(null); setHoverLane(null)
    const laneId = e.over?.id ? String(e.over.id) : null
    if (!task || !laneId) return
    const lane = lanes.find(l => l.id === laneId)
    if (!lane) return
    const plan = dropPlan(task, lane)
    if (!plan.ok) { if (plan.reason) toast(plan.reason, 'warning'); return }
    try {
      if (plan.kind === 'unblock') await post(`/tasks/${task.id}/unblock`)
      else if (plan.kind === 'transition') await post(`/tasks/${task.id}/transition`, { to: plan.to, expectedVersion: task.version })
      else await patch(`/tasks/${task.id}`, { laneId: lane.id })
      invalidate()
    } catch (err: any) {
      toast(err.message, 'error')
      invalidate()
    }
  }

  return (
    <Box sx={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <Box sx={{ px: { xs: 2, md: 3.5 }, pt: 3.5, pb: 2.5, flexShrink: 0 }}>
        <Box sx={{ display: 'flex', alignItems: 'flex-start', gap: 2, mb: 2.5 }}>
          <Box sx={{ flexGrow: 1 }}>
            <Typography variant="h5">工作看板</Typography>
            <Typography variant="body2" color="text.secondary" sx={{ mt: 0.75 }}>跟随任务从规划到交付，让每一步进展都清晰可见</Typography>
          </Box>
          <Button variant="contained" startIcon={<AddIcon />} onClick={() => setCreateOpen(true)}>新建任务</Button>
        </Box>
        <Paper sx={{ p: 1.25, display: 'flex', gap: 1.5, alignItems: 'center', flexWrap: 'wrap', border: '1px solid', borderColor: 'divider', borderRadius: 3 }}>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 2, px: 1 }}>
            <Typography variant="body2" color="text.secondary">共 <Box component="span" sx={{ color: 'text.primary', fontWeight: 700 }}>{tasks.length}</Box> 项</Typography>
            <Divider orientation="vertical" flexItem />
            <Chip size="small" label={`进行中 ${stats.doing}`} sx={{ bgcolor: 'rgba(139,157,255,0.12)', color: 'primary.light' }} />
            <Chip size="small" label={`待审核 ${stats.review}`} sx={{ bgcolor: 'rgba(184,169,247,0.12)', color: 'secondary.main' }} />
            <Chip size="small" label={`阻塞 ${stats.blocked}`} color={stats.blocked > 0 ? 'error' : 'default'} variant={stats.blocked > 0 ? 'filled' : 'outlined'} />
          </Box>
          <Box sx={{ flexGrow: 1 }} />
          <TextField select size="small" label="项目" value={filterProject} onChange={e => setFilterProject(e.target.value)} sx={{ width: 160 }}>
            <MenuItem value="">全部项目</MenuItem>
            {projects.map(p => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
          </TextField>
          <TextField size="small" placeholder="搜索任务…" value={search} onChange={e => setSearch(e.target.value)} sx={{ width: 220 }}
            InputProps={{ startAdornment: <InputAdornment position="start"><SearchIcon fontSize="small" /></InputAdornment> }} />
        </Paper>
      </Box>
      <DndContext sensors={sensors} onDragStart={onDragStart} onDragOver={onDragOver} onDragEnd={onDragEnd} onDragCancel={() => { setActiveTask(null); setHoverLane(null) }}>
        <Box sx={{ display: 'flex', gap: 2, px: { xs: 2, md: 3.5 }, pb: 3.5, overflowX: 'auto', flexGrow: 1, minHeight: 0 }}>
          {lanes.map(lane => (
            <LaneColumn key={lane.id} lane={lane} agents={agents} teams={teams}
              tasks={filtered.filter(t => t.laneId === lane.id || (!t.laneId && lane.kind === (t.status === 'in_progress' ? 'work' : t.status)))}
              onOpen={id => nav(`/ws/${wsId}/tasks/${id}`)} onCreate={() => setCreateOpen(true)}
              dropState={hoverLane?.id === lane.id ? (hoverLane.ok ? 'ok' : 'bad') : null} />
          ))}
        </Box>
        <DragOverlay dropAnimation={null}>
          {activeTask && (
            <Paper sx={{ p: 1.75, width: 296, transform: 'rotate(1.5deg)', boxShadow: '0 18px 48px rgba(0,0,0,0.45)', border: '1px solid', borderColor: 'primary.main', borderRadius: 3, cursor: 'grabbing' }}>
              <CardBody task={activeTask} agents={agents} teams={teams} />
            </Paper>
          )}
        </DragOverlay>
      </DndContext>

      <CreateTaskDialog open={createOpen} onClose={() => setCreateOpen(false)} wsId={wsId} boardId={boardId || ''}
        projects={projects} agents={agents} teams={teams} types={types}
        onCreated={() => { invalidate(); toast('任务已创建（待规划）', 'success') }} />
    </Box>
  )
}

function renderTemplate(tpl: string, vars: Record<string, string>): string {
  return tpl.replace(/\{\{(\w+)\}\}/g, (_, k) => vars[k] ?? `{{${k}}}`)
}

function CreateTaskDialog({ open, onClose, wsId, boardId, projects, agents, teams, types, onCreated }: {
  open: boolean; onClose: () => void; wsId: string; boardId: string
  projects: Project[]; agents: Agent[]; teams: { team: Team }[]; types: TaskType[]; onCreated: () => void
}) {
  const [title, setTitle] = useState('')
  const [content, setContent] = useState('')
  const [typeId, setTypeId] = useState('')
  const [projectId, setProjectId] = useState('')
  const [priority, setPriority] = useState(0)
  const [assignee, setAssignee] = useState('')
  const [vars, setVars] = useState<Record<string, string>>({})
  const [err, setErr] = useState('')
  const type = types.find(t => t.id === typeId)
  const schema = parseJson<{ name: string; label?: string; required?: boolean; default?: string }[]>(type?.variablesSchema, [])
  const previewTitle = type ? renderTemplate(type.titleTemplate, vars) : title
  const previewContent = type ? renderTemplate(type.contentTemplate, vars) : content

  const submit = async () => {
    setErr('')
    try {
      const [kind, id] = assignee ? assignee.split(':') : ['', '']
      if (typeId) {
        await post(`/task-types/${typeId}/instantiate`, {
          variables: vars, projectId: projectId || undefined, priority,
          assigneeAgentId: kind === 'agent' ? id : undefined,
          assigneeTeamId: kind === 'team' ? id : undefined,
        })
      } else {
        await post('/tasks', {
          boardId, title, content, projectId: projectId || undefined, priority,
          assigneeAgentId: kind === 'agent' ? id : undefined,
          assigneeTeamId: kind === 'team' ? id : undefined,
        })
      }
      onCreated(); onClose()
      setTitle(''); setContent(''); setTypeId(''); setVars({}); setAssignee(''); setPriority(0)
    } catch (e: any) { setErr(e.message) }
  }

  return (
    <Dialog open={open} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>新建任务</DialogTitle>
      <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
        <TextField select label="任务类型（模板）" value={typeId} onChange={e => { setTypeId(e.target.value); setVars({}) }}>
          <MenuItem value="">自由创建</MenuItem>
          {types.map(t => <MenuItem key={t.id} value={t.id}>{t.icon} {t.name}</MenuItem>)}
        </TextField>
        {type ? (
          <>
            {schema.map(v => (
              <TextField key={v.name} label={`${v.label || v.name}${v.required ? ' *' : ''}`} value={vars[v.name] ?? v.default ?? ''}
                onChange={e => setVars(s => ({ ...s, [v.name]: e.target.value }))} multiline={v.name !== 'title'} minRows={v.name === 'title' ? 1 : 2} />
            ))}
            {schema.length === 0 && <TextField label="title" value={vars.title || ''} onChange={e => setVars(s => ({ ...s, title: e.target.value }))} />}
            <Paper variant="outlined" sx={{ p: 1.5, bgcolor: 'background.default', borderColor: 'divider' }}>
              <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 0.5 }}>实时预览</Typography>
              <Typography fontWeight={700}>{previewTitle || '（标题）'}</Typography>
              {previewContent && <Typography variant="body2" color="text.secondary" sx={{ whiteSpace: 'pre-wrap', mt: 0.5, maxHeight: 120, overflow: 'auto' }}>{previewContent}</Typography>}
            </Paper>
          </>
        ) : (
          <>
            <TextField label="标题 *" value={title} onChange={e => setTitle(e.target.value)} />
            <TextField label="内容（提示词）" value={content} onChange={e => setContent(e.target.value)} multiline minRows={4} />
          </>
        )}
        <Divider />
        <Box sx={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 2 }}>
          <TextField select label="项目" value={projectId} onChange={e => setProjectId(e.target.value)}>
            <MenuItem value="">不关联项目</MenuItem>
            {projects.map(p => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
          </TextField>
          <TextField select label="优先级" value={priority} onChange={e => setPriority(Number(e.target.value))}>
            {[0, 1, 2, 3].map(p => <MenuItem key={p} value={p}>{p === 0 ? '普通' : `P${p}`}</MenuItem>)}
          </TextField>
        </Box>
        <TextField select label="指派（留空自动匹配）" value={assignee} onChange={e => setAssignee(e.target.value)}>
          <MenuItem value="">自动匹配</MenuItem>
          {agents.map(a => <MenuItem key={a.id} value={`agent:${a.id}`}>🤖 {a.name}</MenuItem>)}
          {teams.map(t => <MenuItem key={t.team.id} value={`team:${t.team.id}`}>👥 {t.team.name}</MenuItem>)}
        </TextField>
        {err && <Typography color="error">{err}</Typography>}
      </DialogContent>
      <DialogActions sx={{ px: 3, pb: 2 }}>
        <Button onClick={onClose}>取消</Button>
        <Button variant="contained" onClick={submit} disabled={!typeId && !title.trim()}>创建任务</Button>
      </DialogActions>
    </Dialog>
  )
}