import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useNavigate, useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { DndContext, DragOverlay, PointerSensor, useSensor, useSensors, useDraggable, useDroppable, type DragEndEvent, type DragOverEvent, type DragStartEvent } from '@dnd-kit/core'
import { Box, Typography, Chip, Paper, Button, IconButton, Dialog, DialogTitle, DialogContent, DialogActions, TextField, MenuItem, Tooltip, InputAdornment, Divider } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import SearchIcon from '@mui/icons-material/Search'
import ChevronLeftIcon from '@mui/icons-material/ChevronLeft'
import ChevronRightIcon from '@mui/icons-material/ChevronRight'
import FlagIcon from '@mui/icons-material/Flag'
import MergeIcon from '@mui/icons-material/Merge'
import ErrorOutlineIcon from '@mui/icons-material/ErrorOutline'
import SmartToyIcon from '@mui/icons-material/SmartToy'
import ChatBubbleOutlineIcon from '@mui/icons-material/ChatBubbleOutline'
import HistoryIcon from '@mui/icons-material/History'
import FolderOutlinedIcon from '@mui/icons-material/FolderOutlined'
import AccountTreeIcon from '@mui/icons-material/AccountTree'
import CategoryIcon from '@mui/icons-material/Category'
import InboxIcon from '@mui/icons-material/Inbox'
import RocketLaunchIcon from '@mui/icons-material/RocketLaunch'
import RateReviewIcon from '@mui/icons-material/RateReview'
import CheckCircleOutlineIcon from '@mui/icons-material/CheckCircleOutline'
import ArchiveIcon from '@mui/icons-material/Archive'
import { get, post, patch, parseJson, type Task, type Lane, type Board, type Project, type Agent, type Workflow, type TaskType } from '../api'
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

function CardBody({ task, agents, workflows, projects, types }: {
  task: Task; agents: Agent[]; workflows: Workflow[]; projects: Project[]; types: TaskType[]
}) {
  const assignee = task.assigneeAgentId ? agents.find(a => a.id === task.assigneeAgentId)?.name : undefined
  const workflowName = task.workflowId ? workflows.find(w => w.id === task.workflowId)?.name : undefined
  const workflowLabel = task.workflowId
    ? `${workflowName || '工作流'}${task.stageTotal ? ` · ${task.stageDone || 0}/${task.stageTotal}` : ''}`
    : undefined
  const projectName = task.projectId ? projects.find(p => p.id === task.projectId)?.name : undefined
  const type = task.typeId ? types.find(t => t.id === task.typeId) : undefined
  const tags = parseJson<string[]>(task.tags, [])
  const running = task.status === 'in_progress' && task.lastRunStatus === 'running'
  return (
    <>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5, mb: 0.6 }}>
        <Typography variant="caption" color="text.disabled" sx={{ fontFamily: 'monospace', fontSize: 11 }}>#{task.id.slice(0, 8)}</Typography>
        <Box sx={{ flexGrow: 1 }} />
        {type && (
          <Tooltip title={`任务类型：${type.name}`}>
            <Chip size="small" icon={<CategoryIcon sx={{ fontSize: 12 }} />} label={type.name} variant="outlined"
              sx={{ maxWidth: 124, '& .MuiChip-label': { overflow: 'hidden', textOverflow: 'ellipsis' } }} />
          </Tooltip>
        )}
        {task.priority > 0 && <Chip size="small" icon={<FlagIcon sx={{ fontSize: 12 }} />} label={`P${task.priority}`} color="warning" variant="outlined" />}
      </Box>

      <Typography variant="body2" fontWeight={650} sx={{ wordBreak: 'break-word', lineHeight: 1.5 }}>{task.title}</Typography>

      {task.content && (
        <Typography variant="caption" color="text.secondary" sx={{
          mt: 0.5, display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical',
          overflow: 'hidden', lineHeight: 1.55, wordBreak: 'break-word',
        }}>{task.content}</Typography>
      )}

      {(running || workflowLabel || task.prState || task.retryCount > 0) && (
        <Box sx={{ display: 'flex', gap: 0.5, flexWrap: 'wrap', mt: 1, alignItems: 'center' }}>
          {running && (
            <Chip size="small" color="primary" variant="filled" label="运行中" icon={
              <Box sx={{ width: 6, height: 6, borderRadius: '50%', bgcolor: 'primary.main', ml: 1, animation: 'pulse 1.4s infinite', '@keyframes pulse': { '50%': { opacity: 0.3 } } }} />
            } />
          )}
          {workflowLabel && <Chip size="small" icon={<AccountTreeIcon sx={{ fontSize: 12 }} />} label={workflowLabel} color="primary" variant="outlined" />}
          {task.prState && <Chip size="small" icon={<MergeIcon sx={{ fontSize: 12 }} />}
            label={task.prState === 'merged' ? '已合并' : task.prState === 'local' ? '本地交付' : 'PR 待合并'}
            color={task.prState === 'merged' ? 'success' : 'default'} variant="outlined" />}
          {task.retryCount > 0 && <Chip size="small" label={`重试 ${task.retryCount}`} variant="outlined" />}
        </Box>
      )}

      {tags.length > 0 && (
        <Box sx={{ display: 'flex', gap: 0.5, flexWrap: 'wrap', mt: 1 }}>
          {tags.map(t => <Chip key={t} size="small" label={t} variant="outlined" />)}
        </Box>
      )}

      {task.status === 'blocked' && task.blockReason && (
        <Box sx={{ mt: 1.25, px: 1.25, py: 0.75, borderRadius: 1.25, bgcolor: theme => theme.palette.mode === 'dark' ? 'rgba(220,38,38,0.12)' : '#fef2f2' }}>
          <Typography variant="caption" sx={{
            color: 'error.main', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical',
            overflow: 'hidden', lineHeight: 1.45,
          }}>{task.blockReason}</Typography>
        </Box>
      )}

      <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.75, minWidth: 0, mt: 1.25, pt: 1.25, borderTop: '1px solid', borderColor: 'divider' }}>
        {projectName && (
          <Tooltip title={`项目：${projectName}`}>
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5, minWidth: 0, maxWidth: 130, color: 'text.secondary' }}>
              <FolderOutlinedIcon sx={{ fontSize: 14, flexShrink: 0 }} />
              <Typography variant="caption" noWrap>{projectName}</Typography>
            </Box>
          </Tooltip>
        )}
        <Box sx={{ flexGrow: 1 }} />
        {(task.commentCount || 0) > 0 && (
          <Tooltip title={`${task.commentCount} 条评论`}>
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.3, color: 'text.secondary' }}>
              <ChatBubbleOutlineIcon sx={{ fontSize: 13 }} />
              <Typography variant="caption" sx={{ fontSize: 11 }}>{task.commentCount}</Typography>
            </Box>
          </Tooltip>
        )}
        {(task.runCount || 0) > 0 && (
          <Tooltip title={`${task.runCount} 次运行`}>
            <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.3, color: 'text.secondary' }}>
              <HistoryIcon sx={{ fontSize: 13 }} />
              <Typography variant="caption" sx={{ fontSize: 11 }}>{task.runCount}</Typography>
            </Box>
          </Tooltip>
        )}
        {assignee && <Tooltip title={assignee}><PersonAvatar name={assignee} size={20} /></Tooltip>}
      </Box>
      <Typography variant="caption" color="text.disabled" sx={{ display: 'block', mt: 0.5, fontSize: 10.5 }}>
        更新于 {task.updatedAt?.replace('T', ' ').slice(5, 16)}
      </Typography>
    </>
  )
}

function TaskCardInner({ task, agents, workflows, projects, types, onOpen }: {
  task: Task; agents: Agent[]; workflows: Workflow[]; projects: Project[]; types: TaskType[]; onOpen: (id: string) => void
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({ id: task.id })
  const running = task.status === 'in_progress'
  return (
    <Paper ref={setNodeRef} {...listeners} {...attributes} onClick={() => onOpen(task.id)}
      onKeyDown={e => e.key === 'Enter' && onOpen(task.id)} role="button" tabIndex={0}
      sx={{
        p: 1.75, mb: 1.25, cursor: 'grab', opacity: isDragging ? 0.35 : 1,
        bgcolor: 'background.paper', border: '1px solid', borderColor: running ? 'primary.light' : 'divider', borderRadius: 2,
        boxShadow: running ? theme => `inset 3px 0 0 ${theme.palette.primary.main}` : 'none',
        transition: 'transform .16s, border-color .16s, box-shadow .16s',
        '&:hover': { transform: 'translateY(-1px)', borderColor: running ? 'primary.main' : '#aab4c3', boxShadow: '0 5px 16px rgba(24,34,48,0.07)' },
        '&:focus-visible': { outline: '2px solid', outlineColor: 'primary.main', outlineOffset: 2 },
        '&:active': { cursor: 'grabbing' },
      }}>
      <CardBody task={task} agents={agents} workflows={workflows} projects={projects} types={types} />
    </Paper>
  )
}

const TaskCard = memo(TaskCardInner)

function LaneColumnInner({ lane, tasks, agents, workflows, projects, types, onOpen, onCreate, dropState }: {
  lane: Lane; tasks: Task[]; agents: Agent[]; workflows: Workflow[]; projects: Project[]; types: TaskType[]
  onOpen: (id: string) => void; onCreate: () => void; dropState: 'ok' | 'bad' | null
}) {
  const { setNodeRef } = useDroppable({ id: lane.id })
  const accent = LANE_ACCENT[lane.kind] || '#8b93a7'
  const hint = LANE_HINT[lane.kind]
  const overLimit = lane.wipLimit != null && tasks.length > lane.wipLimit
  return (
    <Box ref={setNodeRef} sx={{
      width: 300, minWidth: 300, display: 'flex', flexDirection: 'column', height: '100%',
      bgcolor: theme => theme.palette.mode === 'dark' ? 'rgba(255,255,255,0.025)' : '#f0f2f5',
      borderRadius: 2, border: '1px solid',
      borderColor: dropState === 'ok' ? 'success.main' : dropState === 'bad' ? 'error.main' : 'transparent',
      transition: 'border-color .15s, background-color .15s', overflow: 'hidden',
    }}>
      <Box sx={{ px: 1.75, py: 1.25, display: 'flex', alignItems: 'center', gap: 1 }}>
        <Box sx={{ width: 7, height: 7, borderRadius: '50%', bgcolor: accent, flexShrink: 0 }} />
        <Typography variant="subtitle2" sx={{ flexGrow: 1 }}>{lane.name}</Typography>
        {lane.stageRole && <Tooltip title={`阶段角色：${lane.stageRole}`}><Chip size="small" label={lane.stageRole} variant="outlined" /></Tooltip>}
        {lane.wipLimit != null && (
          <Tooltip title={`WIP 上限 ${lane.wipLimit}`}><Chip size="small" label={`${tasks.length}/${lane.wipLimit}`} color={overLimit ? 'error' : 'default'} variant="outlined" /></Tooltip>
        )}
        <Typography variant="caption" sx={{ minWidth: 24, py: 0.25, textAlign: 'center', borderRadius: 1, color: 'text.secondary', bgcolor: 'action.hover', fontWeight: 650 }}>{tasks.length}</Typography>
        {['planning', 'ready'].includes(lane.kind) && (
          <Tooltip title="在此新建任务"><IconButton size="small" onClick={onCreate}><AddIcon fontSize="small" /></IconButton></Tooltip>
        )}
      </Box>
      <Box sx={{ p: 1, overflowY: 'auto', flexGrow: 1 }}>
        {tasks.map(t => <TaskCard key={t.id} task={t} agents={agents} workflows={workflows} projects={projects} types={types} onOpen={onOpen} />)}
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

const LaneColumn = memo(LaneColumnInner)

// 泳道横向滚动区：滚动状态只在本组件内更新，翻页按钮显隐不再重渲染整个看板
function BoardScroller({ children, contentKey }: { children: ReactNode; contentKey: string }) {
  const ref = useRef<HTMLDivElement>(null)
  const raf = useRef(0)
  const [state, setState] = useState({ left: false, right: false })

  const update = useCallback(() => {
    if (raf.current) return
    raf.current = requestAnimationFrame(() => {
      raf.current = 0
      const el = ref.current
      if (!el) return
      const left = el.scrollLeft > 4
      const right = el.scrollLeft + el.clientWidth < el.scrollWidth - 4
      setState(s => s.left === left && s.right === right ? s : { left, right })
    })
  }, [])

  useEffect(() => {
    const el = ref.current
    if (!el) return
    update()
    el.addEventListener('scroll', update, { passive: true })
    const ro = new ResizeObserver(update)
    ro.observe(el)
    return () => {
      el.removeEventListener('scroll', update)
      ro.disconnect()
      if (raf.current) {
        cancelAnimationFrame(raf.current)
        raf.current = 0
      }
    }
  }, [update, contentKey])

  const scroll = (dir: -1 | 1) => {
    const el = ref.current
    if (!el) return
    // 一次切换一屏左右的距离，保持平滑滚动但不过长
    el.scrollBy({ left: dir * Math.max(280, el.clientWidth * 0.6), behavior: 'smooth' })
  }

  return (
    <Box sx={{ position: 'relative', display: 'flex', flexGrow: 1, minHeight: 0 }}>
      <Box ref={ref} sx={{ display: 'flex', gap: 2, px: { xs: 2, md: 3 }, pt: 2.5, pb: 3, overflowX: 'auto', flexGrow: 1, minHeight: 0 }}>
        {children}
      </Box>
      {state.left && (
        <IconButton aria-label="向左查看更多泳道" onClick={() => scroll(-1)} sx={scrollButtonSx('left')}>
          <ChevronLeftIcon fontSize="small" />
        </IconButton>
      )}
      {state.right && (
        <IconButton aria-label="向右查看更多泳道" onClick={() => scroll(1)} sx={scrollButtonSx('right')}>
          <ChevronRightIcon fontSize="small" />
        </IconButton>
      )}
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
  const { data: workflows = [] } = useQuery({ queryKey: ['workflows', wsId], queryFn: () => get<Workflow[]>(`/workspaces/${wsId}/workflows`) })
  const { data: types = [] } = useQuery({ queryKey: ['types', wsId], queryFn: () => get<TaskType[]>(`/workspaces/${wsId}/task-types`) })

  const [createOpen, setCreateOpen] = useState(false)
  const [filterProject, setFilterProject] = useState('')
  const [search, setSearch] = useState('')
  const [activeTask, setActiveTask] = useState<Task | null>(null)
  const [hoverLane, setHoverLane] = useState<{ id: string; ok: boolean } | null>(null)
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 6 } }))

  const lanes = useMemo(() => (boardData?.lanes || []).slice().sort((a, b) => a.position - b.position), [boardData])
  const filtered = useMemo(() => tasks.filter(t =>
    (!filterProject || t.projectId === filterProject) &&
    (!search || t.title.toLowerCase().includes(search.toLowerCase()))), [tasks, filterProject, search])
  const laneTasks = useMemo(() => {
    const map = new Map<string, Task[]>()
    for (const lane of lanes) {
      map.set(lane.id, filtered.filter(t =>
        t.laneId === lane.id || (!t.laneId && lane.kind === (t.status === 'in_progress' ? 'work' : t.status))))
    }
    return map
  }, [lanes, filtered])
  const openTask = useCallback((id: string) => nav(`/ws/${wsId}/tasks/${id}`), [nav, wsId])
  const openCreate = useCallback(() => setCreateOpen(true), [])
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
      <Box sx={{ px: { xs: 2, md: 3 }, pt: 3, flexShrink: 0, bgcolor: 'background.paper', borderBottom: '1px solid', borderColor: 'divider' }}>
        <Box sx={{ display: 'flex', alignItems: 'flex-start', gap: 2, mb: 2.5 }}>
          <Box sx={{ flexGrow: 1, minWidth: 0 }}>
            <Typography variant="h5">工作看板</Typography>
            <Typography variant="body2" color="text.secondary" sx={{ mt: 0.75 }}>规划、执行、审核与交付，一屏掌握团队进展</Typography>
          </Box>
          <Button variant="contained" startIcon={<AddIcon />} onClick={() => setCreateOpen(true)} sx={{ flexShrink: 0 }}>新建任务</Button>
        </Box>
        <Box sx={{ display: 'flex', gap: 2, alignItems: 'center', flexWrap: 'wrap', py: 2, borderTop: '1px solid', borderColor: 'divider' }}>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: { xs: 2, md: 3 }, width: { xs: '100%', lg: 'auto' }, flexWrap: 'wrap' }}>
            {[
              { label: '全部任务', value: tasks.length, color: '#697386' },
              { label: '进行中', value: stats.doing, color: '#635bff' },
              { label: '待审核', value: stats.review, color: '#875bf7' },
              { label: '阻塞', value: stats.blocked, color: stats.blocked ? '#d85160' : '#a0a7b5' },
            ].map(s => (
              <Box key={s.label} sx={{ display: 'flex', alignItems: 'center', gap: 0.75 }}>
                <Box sx={{ width: 6, height: 6, borderRadius: '50%', bgcolor: s.color }} />
                <Typography variant="body2" color="text.secondary">{s.label}</Typography>
                <Typography variant="body2" fontWeight={750} sx={{ color: s.color }}>{s.value}</Typography>
              </Box>
            ))}
          </Box>
          <Box sx={{ flexGrow: 1 }} />
          <Box sx={{ display: 'flex', gap: 1, width: { xs: '100%', sm: 'auto' } }}>
            <TextField select size="small" label="项目" value={filterProject} onChange={e => setFilterProject(e.target.value)} sx={{ width: { xs: '42%', sm: 160 } }}>
              <MenuItem value="">全部项目</MenuItem>
              {projects.map(p => <MenuItem key={p.id} value={p.id}>{p.name}</MenuItem>)}
            </TextField>
            <TextField size="small" placeholder="搜索任务…" value={search} onChange={e => setSearch(e.target.value)} sx={{ width: { xs: '58%', sm: 220 } }}
              slotProps={{ input: { startAdornment: <InputAdornment position="start"><SearchIcon fontSize="small" /></InputAdornment> } }} />
          </Box>
        </Box>
      </Box>
      <DndContext sensors={sensors} onDragStart={onDragStart} onDragOver={onDragOver} onDragEnd={onDragEnd} onDragCancel={() => { setActiveTask(null); setHoverLane(null) }}>
        <BoardScroller contentKey={`${lanes.length}:${filtered.length}`}>
          {lanes.map(lane => (
            <LaneColumn key={lane.id} lane={lane} agents={agents} workflows={workflows} projects={projects} types={types}
              tasks={laneTasks.get(lane.id) || []}
              onOpen={openTask} onCreate={openCreate}
              dropState={hoverLane?.id === lane.id ? (hoverLane.ok ? 'ok' : 'bad') : null} />
          ))}
        </BoardScroller>
        <DragOverlay dropAnimation={null}>
          {activeTask && (
            <Paper sx={{ p: 1.75, width: 288, transform: 'rotate(1.5deg)', boxShadow: '0 18px 48px rgba(29,36,58,0.22)', borderRadius: 1.5, cursor: 'grabbing' }}>
              <CardBody task={activeTask} agents={agents} workflows={workflows} projects={projects} types={types} />
            </Paper>
          )}
        </DragOverlay>
      </DndContext>

      <CreateTaskDialog open={createOpen} onClose={() => setCreateOpen(false)} wsId={wsId} boardId={boardId || ''}
        projects={projects} agents={agents} workflows={workflows} types={types}
        onCreated={() => { invalidate(); toast('任务已创建（待规划）', 'success') }} />
    </Box>
  )
}

const scrollButtonSx = (side: 'left' | 'right') => ({
  position: 'absolute' as const,
  ...(side === 'left' ? { left: { xs: 6, md: 10 } } : { right: { xs: 6, md: 10 } }),
  top: '46%',
  transform: 'translateY(-50%)',
  zIndex: 3,
  width: 36,
  height: 36,
  bgcolor: 'background.paper',
  border: '1px solid',
  borderColor: 'divider',
  boxShadow: '0 6px 18px rgba(24,34,48,0.16)',
  color: 'text.secondary',
  '&:hover': { bgcolor: 'background.paper', color: 'primary.main' },
})

function renderTemplate(tpl: string, vars: Record<string, string>): string {
  return tpl.replace(/\{\{(\w+)\}\}/g, (_, k) => vars[k] ?? `{{${k}}}`)
}

function CreateTaskDialog({ open, onClose, wsId, boardId, projects, agents, workflows, types, onCreated }: {
  open: boolean; onClose: () => void; wsId: string; boardId: string
  projects: Project[]; agents: Agent[]; workflows: Workflow[]; types: TaskType[]; onCreated: () => void
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
        })
      } else {
        await post('/tasks', {
          boardId, title, content, projectId: projectId || undefined, priority,
          assigneeAgentId: kind === 'agent' ? id : undefined,
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
          {types.map(t => <MenuItem key={t.id} value={t.id}><CategoryIcon fontSize="small" sx={{ mr: 1, color: 'text.secondary' }} />{t.name}</MenuItem>)}
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
          {agents.map(a => <MenuItem key={a.id} value={`agent:${a.id}`}><SmartToyIcon fontSize="small" sx={{ mr: 1, color: 'text.secondary' }} />{a.name}</MenuItem>)}
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