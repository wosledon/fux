import { useMemo, useRef, useState } from 'react'
import { Box, Button, Chip, Paper, Stack, Typography } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import AutoFixHighIcon from '@mui/icons-material/AutoFixHigh'
import LinkIcon from '@mui/icons-material/Link'
import type { Agent, Project, WorkflowStage } from '../api'

export const NODE_W = 216
export const NODE_H = 104
const GAP_X = 96
const GAP_Y = 40

export function stagePos(s: WorkflowStage, index: number): { x: number; y: number } {
  return {
    x: s.x ?? 48 + (index % 3) * (NODE_W + GAP_X),
    y: s.y ?? 48 + Math.floor(index / 3) * (NODE_H + GAP_Y),
  }
}

/** 保存前补齐坐标，保证重开画布时布局稳定。 */
export function normalizeStages(stages: WorkflowStage[]): WorkflowStage[] {
  return stages.map((s, i) => ({ ...s, ...stagePos(s, i) }))
}

/** 按依赖层级自动布局。 */
export function autoLayout(stages: WorkflowStage[]): WorkflowStage[] {
  const byKey = new Map(stages.map(s => [s.key, s]))
  const levelOf = (s: WorkflowStage, depth = 0): number => {
    if (s.dependsOn.length === 0 || depth > 50) return 0
    return 1 + Math.max(...s.dependsOn.map(k => { const d = byKey.get(k); return d ? levelOf(d, depth + 1) : 0 }))
  }
  const counters = new Map<number, number>()
  return stages.map(s => {
    const lv = levelOf(s)
    const idx = counters.get(lv) || 0
    counters.set(lv, idx + 1)
    return { ...s, x: 48 + lv * (NODE_W + GAP_X), y: 48 + idx * (NODE_H + GAP_Y) }
  })
}

/** 判断 from 是否（直接或间接）依赖 target，用于阻止成环。 */
export function reaches(stages: WorkflowStage[], from: string, target: string, seen = new Set<string>()): boolean {
  if (from === target) return true
  if (seen.has(from)) return false
  seen.add(from)
  const s = stages.find(x => x.key === from)
  return !!s && s.dependsOn.some(d => reaches(stages, d, target, seen))
}

type Props = {
  stages: WorkflowStage[]
  agents: Agent[]
  projects: Project[]
  selected: string | null
  onChange: (next: WorkflowStage[]) => void
  onSelect: (key: string | null) => void
  /** 只读：用于任务详情的流程视图 */
  readonly?: boolean
  statusOf?: (key: string) => string
  metaOf?: (key: string) => string | undefined
}

export default function WorkflowCanvas({ stages, agents, projects, selected, onChange, onSelect, readonly = false, statusOf, metaOf }: Props) {
  const canvasRef = useRef<HTMLDivElement>(null)
  const dragRef = useRef<{ key: string; dx: number; dy: number } | null>(null)
  const [connectMode, setConnectMode] = useState(false)
  const [connectFrom, setConnectFrom] = useState<string | null>(null)
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null)
  const [hint, setHint] = useState('')

  const positions = useMemo(() => {
    const map = new Map<string, { x: number; y: number }>()
    stages.forEach((s, i) => map.set(s.key, stagePos(s, i)))
    return map
  }, [stages])

  const maxX = Math.max(760, ...stages.map(s => (positions.get(s.key)?.x || 0) + NODE_W + 80))
  const maxY = Math.max(readonly ? 240 : 460, ...stages.map(s => (positions.get(s.key)?.y || 0) + NODE_H + 80))

  const point = (e: React.PointerEvent): { x: number; y: number } => {
    const rect = canvasRef.current?.getBoundingClientRect()
    return { x: e.clientX - (rect?.left || 0) + (canvasRef.current?.scrollLeft || 0), y: e.clientY - (rect?.top || 0) + (canvasRef.current?.scrollTop || 0) }
  }

  const onNodeDown = (e: React.PointerEvent, s: WorkflowStage) => {
    if (readonly || connectMode) return
    e.stopPropagation()
    const p = positions.get(s.key)!
    const pt = point(e)
    dragRef.current = { key: s.key, dx: pt.x - p.x, dy: pt.y - p.y }
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  }

  const onNodeClick = (s: WorkflowStage) => {
    if (readonly) { onSelect(s.key); return }
    if (!connectMode) { onSelect(s.key); return }
    if (!connectFrom) {
      setConnectFrom(s.key)
      setHint('再点一个节点作为下游（依赖方向：先 → 后）')
      return
    }
    if (connectFrom === s.key) { setConnectFrom(null); setHint(''); return }
    if (reaches(stages, connectFrom, s.key)) {
      setHint('不能这样连：会形成循环依赖')
      return
    }
    if (!s.dependsOn.includes(connectFrom)) {
      onChange(stages.map(x => x.key === s.key ? { ...x, dependsOn: [...x.dependsOn, connectFrom] } : x))
    }
    setConnectFrom(null)
    setHint('')
  }

  const onPointerMove = (e: React.PointerEvent) => {
    const pt = point(e)
    if (connectFrom) {
      setCursor(pt)
      return
    }
    const drag = dragRef.current
    if (!drag) return
    const x = Math.max(0, Math.round((pt.x - drag.dx) / 8) * 8)
    const y = Math.max(0, Math.round((pt.y - drag.dy) / 8) * 8)
    onChange(stages.map(s => s.key === drag.key ? { ...s, x, y } : s))
  }

  const addNode = () => {
    const index = stages.length
    const key = nextKey(stages)
    const anchor = selected ? stages.find(s => s.key === selected) : stages[stages.length - 1]
    const pos = anchor ? stagePos(anchor, Math.max(0, stages.indexOf(anchor))) : { x: 48, y: 48 }
    onChange([...stages, {
      key, name: `节点 ${index + 1}`, agentId: agents[0]?.id || '', dependsOn: anchor ? [anchor.key] : [],
      x: pos.x + NODE_W + GAP_X, y: pos.y,
    }])
    onSelect(key)
  }

  const edges: { from: string; to: string }[] = []
  for (const s of stages) for (const d of s.dependsOn) if (positions.has(d)) edges.push({ from: d, to: s.key })

  return (
      <Box sx={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: readonly ? 240 : 460 }}>
      {!readonly && (
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, px: 2, py: 1.25, borderBottom: '1px solid', borderColor: 'divider', bgcolor: '#fafbfc' }}>
        <Button size="small" startIcon={<AddIcon />} onClick={addNode}>添加节点</Button>
        <Button size="small" variant={connectMode ? 'contained' : 'outlined'} color={connectMode ? 'primary' : 'inherit'}
          startIcon={<LinkIcon />}
          onClick={() => { setConnectMode(v => !v); setConnectFrom(null); setHint('') }}>
          {connectMode ? '退出连线' : '连线'}
        </Button>
        <Button size="small" startIcon={<AutoFixHighIcon />} onClick={() => onChange(autoLayout(stages))}>自动布局</Button>
        <Box sx={{ flexGrow: 1 }} />
        <Typography variant="caption" color="text.secondary">{hint || (connectMode ? '点击起点节点，再点击下游节点' : '拖动节点摆放位置 · 依赖满足即并行执行')}</Typography>
      </Box>
      )}

      <Box ref={canvasRef} onPointerMove={onPointerMove}
        onPointerUp={() => { dragRef.current = null; setCursor(null) }}
        onClick={() => { if (!connectMode) onSelect(null) }}
        sx={{
          position: 'relative', flexGrow: 1, overflow: 'auto',
          backgroundImage: 'radial-gradient(circle, #d7dbe4 1px, transparent 1px)',
          backgroundSize: '18px 18px', bgcolor: '#f7f8fb',
          cursor: connectMode ? 'crosshair' : 'default',
        }}>
        <Box sx={{ position: 'relative', width: maxX, height: maxY }}>
          <svg width={maxX} height={maxY} style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}>
            <defs>
              <marker id="wf-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M 0 0 L 10 5 L 0 10 z" fill="#8b93a7" />
              </marker>
            </defs>
            {edges.map((e, i) => {
              const a = positions.get(e.from)!
              const b = positions.get(e.to)!
              const x1 = a.x + NODE_W, y1 = a.y + NODE_H / 2
              const x2 = b.x, y2 = b.y + NODE_H / 2
              const mid = Math.max(40, Math.abs(x2 - x1) / 2)
              const d = `M ${x1} ${y1} C ${x1 + mid} ${y1}, ${x2 - mid} ${y2}, ${x2} ${y2}`
              const st = statusOf?.(e.to)
              const color = st === 'succeeded' || st === 'finished' ? '#16a34a'
                : st === 'running' ? '#635bff'
                : st === 'failed' || st === 'cancelled' ? '#d85160'
                : st === 'skipped' ? '#a0a7b5' : '#8b93a7'
              return (
                <g key={i}>
                  <path d={d} stroke={color} strokeWidth={1.8} fill="none" markerEnd="url(#wf-arrow)" />
                  {!readonly && (
                    <path d={d} stroke="transparent" strokeWidth={14} fill="none" style={{ pointerEvents: 'stroke', cursor: 'pointer' }}
                      onClick={ev => {
                        ev.stopPropagation()
                        onChange(stages.map(x => x.key === e.to ? { ...x, dependsOn: x.dependsOn.filter(dep => dep !== e.from) } : x))
                      }}>
                      <title>点击删除依赖 {e.from} → {e.to}</title>
                    </path>
                  )}
                </g>
              )
            })}
            {connectFrom && cursor && (() => {
              const a = positions.get(connectFrom)
              if (!a) return null
              return <line x1={a.x + NODE_W} y1={a.y + NODE_H / 2} x2={cursor.x} y2={cursor.y}
                stroke="#635bff" strokeWidth={1.5} strokeDasharray="5 4" />
            })()}
          </svg>

          {stages.map((s, i) => {
            const p = stagePos(s, i)
            const agentName = agents.find(a => a.id === s.agentId)?.name || '未绑定'
            const projectName = projects.find(pp => pp.id === s.projectId)?.name || '主项目'
            const isSelected = selected === s.key
            const isSource = connectFrom === s.key
            const status = statusOf?.(s.key)
            const statusBorder = status === 'running' ? 'primary.main'
              : status === 'succeeded' || status === 'finished' ? 'success.main'
              : status === 'failed' || status === 'cancelled' ? 'error.main'
              : undefined
            return (
              <Paper key={s.key} elevation={0}
                onPointerDown={e => onNodeDown(e, s)}
                onClick={e => { e.stopPropagation(); onNodeClick(s) }}
                sx={{
                  position: 'absolute', left: p.x, top: p.y, width: NODE_W, height: NODE_H,
                  p: 1.5, borderRadius: 1.5, cursor: readonly ? 'pointer' : connectMode ? 'crosshair' : 'grab', userSelect: 'none',
                  border: '2px solid',
                  borderColor: isSource ? 'warning.main' : isSelected ? 'primary.main' : (readonly && statusBorder ? statusBorder : 'divider'),
                  boxShadow: isSelected ? '0 6px 18px rgba(99,91,255,0.16)' : '0 1px 3px rgba(29,36,58,0.06)',
                  bgcolor: '#fff',
                }}>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
                  <Typography variant="body2" fontWeight={700} noWrap sx={{ flexGrow: 1 }}>{s.name || s.key}</Typography>
                  {status && <Chip size="small" label={statusLabel(status)} color={statusChipColor(status)} sx={{ height: 18, fontSize: 10 }} />}
                </Box>
                <Typography variant="caption" color="text.secondary" display="block" noWrap sx={{ fontFamily: 'monospace' }}>{s.key}</Typography>
                <Stack direction="row" spacing={0.5} sx={{ mt: 1, flexWrap: 'wrap', gap: 0.5 }}>
                  <Chip size="small" label={agentName} variant="outlined" sx={{ height: 20, fontSize: 10 }} />
                  <Chip size="small" label={projectName} variant="outlined" sx={{ height: 20, fontSize: 10 }} />
                  {s.dependsOn.length === 0 && <Chip size="small" label="入口" color="primary" sx={{ height: 20, fontSize: 10 }} />}
                  {s.when && <Chip size="small" label={s.when} color="warning" variant="outlined" sx={{ height: 20, fontSize: 10 }} />}
                </Stack>
                {metaOf?.(s.key) && <Typography variant="caption" color="text.secondary" display="block" noWrap sx={{ mt: 0.5 }}>{metaOf(s.key)}</Typography>}
              </Paper>
            )
          })}

          {stages.length === 0 && (
            <Box sx={{ position: 'absolute', inset: 0, display: 'grid', placeItems: 'center', pointerEvents: 'none' }}>
              <Typography color="text.secondary">点击「添加节点」开始绘制工作流</Typography>
            </Box>
          )}
        </Box>
      </Box>
    </Box>
  )
}

function statusLabel(status: string): string {
  return ({ pending: '待执行', running: '执行中', succeeded: '已完成', finished: '已完成', failed: '失败', cancelled: '已取消', skipped: '已跳过', retrying: '重试中' } as Record<string, string>)[status] || status
}
function statusChipColor(status: string): 'default' | 'primary' | 'success' | 'error' | 'warning' {
  if (status === 'running') return 'primary'
  if (status === 'succeeded' || status === 'finished') return 'success'
  if (status === 'failed') return 'error'
  if (status === 'cancelled' || status === 'retrying') return 'warning'
  return 'default'
}

function nextKey(stages: WorkflowStage[]): string {
  for (let i = 1; i < 999; i++) {
    const key = `node-${i}`
    if (!stages.some(s => s.key === key)) return key
  }
  return `node-${Date.now()}`
}