import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Box, Button, Chip, IconButton, Paper, Stack, Tooltip, Typography } from '@mui/material'
import { useTheme } from '@mui/material/styles'
import AddIcon from '@mui/icons-material/Add'
import AutoFixHighIcon from '@mui/icons-material/AutoFixHigh'
import AutorenewIcon from '@mui/icons-material/Autorenew'
import FitScreenIcon from '@mui/icons-material/FitScreen'
import LinkIcon from '@mui/icons-material/Link'
import ZoomInIcon from '@mui/icons-material/ZoomIn'
import ZoomOutIcon from '@mui/icons-material/ZoomOut'
import type { Agent, Project, WorkflowStage } from '../api'
import { NODE_H, NODE_W, autoLayout, reaches, stagePos } from './WorkflowCanvas'

type Props = {
  stages: WorkflowStage[]
  agents: Agent[]
  projects: Project[]
  selected: string | null
  onChange: (next: WorkflowStage[]) => void
  onSelect: (key: string | null) => void
}

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v))
const MIN_ZOOM = 0.35
const MAX_ZOOM = 2.2

export default function WorkflowCanvasEditor({ stages, agents, projects, selected, onChange, onSelect }: Props) {
  const theme = useTheme()
  const edgeColor = theme.palette.text.secondary
  const canvasRef = useRef<HTMLDivElement>(null)
  const dragRef = useRef<{ key: string; dx: number; dy: number; moved: boolean } | null>(null)
  const suppressNodeClickRef = useRef(false)
  const panRef = useRef<{ sx: number; sy: number; ox: number; oy: number; moved: boolean } | null>(null)
  const suppressCanvasClickRef = useRef(false)
  const fittedRef = useRef(false)
  const [view, setView] = useState({ x: 0, y: 0, zoom: 1 })
  const [connectMode, setConnectMode] = useState(false)
  const [connectFrom, setConnectFrom] = useState<string | null>(null)
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null)
  const [hint, setHint] = useState('')

  const positions = useMemo(() => {
    const map = new Map<string, { x: number; y: number }>()
    stages.forEach((s, i) => map.set(s.key, stagePos(s, i)))
    return map
  }, [stages])

  const worldW = Math.max(1600, ...stages.map(s => (positions.get(s.key)?.x || 0) + NODE_W + 480))
  const worldH = Math.max(1000, ...stages.map(s => (positions.get(s.key)?.y || 0) + NODE_H + 480))

  const toWorld = (clientX: number, clientY: number) => {
    const rect = canvasRef.current?.getBoundingClientRect()
    return {
      x: ((clientX - (rect?.left || 0)) - view.x) / view.zoom,
      y: ((clientY - (rect?.top || 0)) - view.y) / view.zoom,
    }
  }

  const zoomAt = (factor: number, mx?: number, my?: number) => {
    const rect = canvasRef.current?.getBoundingClientRect()
    const px = mx ?? (rect ? rect.width / 2 : 0)
    const py = my ?? (rect ? rect.height / 2 : 0)
    setView(v => {
      const zoom = clamp(v.zoom * factor, MIN_ZOOM, MAX_ZOOM)
      return { zoom, x: px - (px - v.x) * (zoom / v.zoom), y: py - (py - v.y) * (zoom / v.zoom) }
    })
  }

  const fitTo = useCallback((list: WorkflowStage[]) => {
    const el = canvasRef.current
    if (!el || list.length === 0) return
    const rect = el.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0) return
    const pts = list.map((s, i) => stagePos(s, i))
    const minX = Math.min(...pts.map(p => p.x))
    const maxX = Math.max(...pts.map(p => p.x + NODE_W))
    const minY = Math.min(...pts.map(p => p.y))
    const maxY = Math.max(...pts.map(p => p.y + NODE_H))
    const pad = 64
    const zoom = clamp(
      Math.min((rect.width - pad * 2) / Math.max(1, maxX - minX), (rect.height - pad * 2) / Math.max(1, maxY - minY)),
      MIN_ZOOM,
      1.3,
    )
    setView({
      zoom,
      x: (rect.width - (maxX - minX) * zoom) / 2 - minX * zoom,
      y: (rect.height - (maxY - minY) * zoom) / 2 - minY * zoom,
    })
  }, [])

  // 首次拿到节点后自动适应画布
  useEffect(() => {
    if (fittedRef.current || stages.length === 0) return
    fittedRef.current = true
    requestAnimationFrame(() => fitTo(stages))
  }, [stages, fitTo])

  // 滚轮缩放（原生监听以支持 preventDefault）
  useEffect(() => {
    const el = canvasRef.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      const rect = el.getBoundingClientRect()
      const mx = e.clientX - rect.left
      const my = e.clientY - rect.top
      setView(v => {
        const zoom = clamp(v.zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1), MIN_ZOOM, MAX_ZOOM)
        return { zoom, x: mx - (mx - v.x) * (zoom / v.zoom), y: my - (my - v.y) * (zoom / v.zoom) }
      })
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [])

  const onNodeDown = (e: React.PointerEvent, s: WorkflowStage) => {
    if (connectMode || e.button !== 0) return
    e.stopPropagation()
    suppressNodeClickRef.current = false
    const p = positions.get(s.key)
    if (!p) return
    const w = toWorld(e.clientX, e.clientY)
    dragRef.current = { key: s.key, dx: w.x - p.x, dy: w.y - p.y, moved: false }
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  }

  const onNodeMove = (e: React.PointerEvent) => {
    const drag = dragRef.current
    if (!drag) return
    const w = toWorld(e.clientX, e.clientY)
    const x = Math.max(0, Math.round((w.x - drag.dx) / 8) * 8)
    const y = Math.max(0, Math.round((w.y - drag.dy) / 8) * 8)
    const cur = positions.get(drag.key)
    if (!cur || (cur.x === x && cur.y === y)) return
    drag.moved = true
    onChange(stages.map(s => s.key === drag.key ? { ...s, x, y } : s))
  }

  const onNodeUp = (e: React.PointerEvent) => {
    if (dragRef.current?.moved) suppressNodeClickRef.current = true
    dragRef.current = null
    try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId) } catch { /* ignore */ }
  }

  const onNodeClick = (s: WorkflowStage) => {
    if (suppressNodeClickRef.current) {
      suppressNodeClickRef.current = false
      return
    }
    if (!connectMode) {
      onSelect(s.key)
      return
    }
    if (!connectFrom) {
      setConnectFrom(s.key)
      setHint('再点一个节点作为下游（依赖方向：先 → 后）')
      return
    }
    if (connectFrom === s.key) {
      setConnectFrom(null)
      setHint('')
      return
    }
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

  const onCanvasDown = (e: React.PointerEvent) => {
    if (connectMode || e.button !== 0) return
    panRef.current = { sx: e.clientX, sy: e.clientY, ox: view.x, oy: view.y, moved: false }
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  }

  const onCanvasMove = (e: React.PointerEvent) => {
    if (connectFrom) {
      setCursor(toWorld(e.clientX, e.clientY))
      return
    }
    const p = panRef.current
    if (!p) return
    const dx = e.clientX - p.sx
    const dy = e.clientY - p.sy
    if (Math.abs(dx) + Math.abs(dy) > 3) p.moved = true
    setView(v => ({ ...v, x: p.ox + dx, y: p.oy + dy }))
  }

  const onCanvasUp = (e: React.PointerEvent) => {
    if (panRef.current?.moved) suppressCanvasClickRef.current = true
    panRef.current = null
    try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId) } catch { /* ignore */ }
  }

  const onCanvasClick = () => {
    if (suppressCanvasClickRef.current) {
      suppressCanvasClickRef.current = false
      return
    }
    if (!connectMode) onSelect(null)
  }

  const addNode = () => {
    const key = nextKey(stages)
    const rect = canvasRef.current?.getBoundingClientRect()
    const cx = rect ? (rect.width / 2 - view.x) / view.zoom : 360
    const cy = rect ? (rect.height / 2 - view.y) / view.zoom : 220
    onChange([...stages, {
      key,
      name: `节点 ${stages.length + 1}`,
      agentId: agents[0]?.id || '',
      dependsOn: selected ? [selected] : [],
      maxRetries: 2,
      x: Math.max(0, Math.round(cx / 8) * 8 - NODE_W / 2),
      y: Math.max(0, Math.round(cy / 8) * 8 - NODE_H / 2),
    }])
    onSelect(key)
  }

  const edges: { from: string; to: string }[] = []
  for (const s of stages) for (const d of s.dependsOn) if (positions.has(d)) edges.push({ from: d, to: s.key })

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 460 }}>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, px: 2, py: 1, borderBottom: '1px solid', borderColor: 'divider', bgcolor: 'background.paper', flexWrap: 'wrap' }}>
        <Button size="small" startIcon={<AddIcon />} onClick={addNode}>添加节点</Button>
        <Button size="small" variant={connectMode ? 'contained' : 'outlined'} color={connectMode ? 'primary' : 'inherit'}
          startIcon={<LinkIcon />}
          onClick={() => { setConnectMode(v => !v); setConnectFrom(null); setHint('') }}>
          {connectMode ? '退出连线' : '连线'}
        </Button>
        <Button size="small" startIcon={<AutoFixHighIcon />} onClick={() => {
          const next = autoLayout(stages)
          onChange(next)
          requestAnimationFrame(() => fitTo(next))
        }}>自动布局</Button>
        <Box sx={{ flexGrow: 1 }} />
        <Typography variant="caption" color="text.secondary">
          {hint || (connectMode ? '点击起点节点，再点击下游节点' : '拖动节点 · 拖动画布平移 · 滚轮缩放 · 依赖满足即并行执行')}
        </Typography>
      </Box>

      <Box
        ref={canvasRef}
        onPointerDown={onCanvasDown}
        onPointerMove={onCanvasMove}
        onPointerUp={onCanvasUp}
        onClick={onCanvasClick}
        sx={{
          position: 'relative', flexGrow: 1, overflow: 'hidden', touchAction: 'none',
          backgroundImage: theme => `radial-gradient(circle, ${theme.palette.divider} 1px, transparent 1px)`,
          backgroundSize: `${18 * view.zoom}px ${18 * view.zoom}px`,
          backgroundPosition: `${view.x}px ${view.y}px`,
          bgcolor: 'background.default',
          cursor: connectMode ? 'crosshair' : 'grab',
          '&:active': { cursor: connectMode ? 'crosshair' : 'grabbing' },
        }}
      >
        <Box sx={{
          position: 'absolute', left: 0, top: 0, width: worldW, height: worldH,
          transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})`,
          transformOrigin: '0 0',
        }}>
          <svg width={worldW} height={worldH} style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}>
            <defs>
              <marker id="wf-edit-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M 0 0 L 10 5 L 0 10 z" fill={edgeColor} />
              </marker>
            </defs>
            {edges.map((e, i) => {
              const a = positions.get(e.from)!
              const b = positions.get(e.to)!
              const x1 = a.x + NODE_W
              const y1 = a.y + NODE_H / 2
              const x2 = b.x
              const y2 = b.y + NODE_H / 2
              const mid = Math.max(40, Math.abs(x2 - x1) / 2)
              const d = `M ${x1} ${y1} C ${x1 + mid} ${y1}, ${x2 - mid} ${y2}, ${x2} ${y2}`
              return (
                <g key={i}>
                  <path d={d} stroke={edgeColor} strokeWidth={1.8} fill="none" markerEnd="url(#wf-edit-arrow)"
                    vectorEffect="non-scaling-stroke" />
                  <path d={d} stroke="transparent" strokeWidth={16} fill="none"
                    vectorEffect="non-scaling-stroke"
                    style={{ pointerEvents: 'stroke', cursor: 'pointer' }}
                    onClick={ev => {
                      ev.stopPropagation()
                      onChange(stages.map(x => x.key === e.to ? { ...x, dependsOn: x.dependsOn.filter(dep => dep !== e.from) } : x))
                    }}>
                    <title>点击删除依赖 {e.from} → {e.to}</title>
                  </path>
                </g>
              )
            })}
            {connectFrom && cursor && (() => {
              const a = positions.get(connectFrom)
              if (!a) return null
              return <line x1={a.x + NODE_W} y1={a.y + NODE_H / 2} x2={cursor.x} y2={cursor.y}
                stroke={theme.palette.primary.main} strokeWidth={1.5} strokeDasharray="5 4" vectorEffect="non-scaling-stroke" />
            })()}
          </svg>

          {stages.map((s, i) => {
            const p = stagePos(s, i)
            const agentName = agents.find(a => a.id === s.agentId)?.name || '未绑定'
            const projectName = projects.find(pp => pp.id === s.projectId)?.name || '主项目'
            const isSelected = selected === s.key
            const isSource = connectFrom === s.key
            return (
              <Paper key={s.key} elevation={0} data-node={s.key}
                onPointerDown={e => onNodeDown(e, s)}
                onPointerMove={onNodeMove}
                onPointerUp={onNodeUp}
                onClick={e => { e.stopPropagation(); onNodeClick(s) }}
                sx={{
                  position: 'absolute', left: p.x, top: p.y, width: NODE_W, height: NODE_H,
                  p: 1.5, borderRadius: 1.5, userSelect: 'none', touchAction: 'none',
                  cursor: connectMode ? 'crosshair' : 'grab',
                  border: '2px solid',
                  borderColor: isSource ? 'warning.main' : isSelected ? 'primary.main' : 'divider',
                  boxShadow: isSelected ? '0 6px 18px rgba(37,99,235,0.14)' : 'none',
                  bgcolor: 'background.paper',
                  '&:active': { cursor: connectMode ? 'crosshair' : 'grabbing' },
                }}>
                <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
                  <Typography variant="body2" fontWeight={700} noWrap sx={{ flexGrow: 1 }}>{s.name || s.key}</Typography>
                  {isSource && <Chip size="small" label="起点" color="warning" sx={{ height: 18, fontSize: 10 }} />}
                </Box>
                <Typography variant="caption" color="text.secondary" display="block" noWrap sx={{ fontFamily: 'monospace' }}>{s.key}</Typography>
                <Stack direction="row" spacing={0.5} sx={{ mt: 1, flexWrap: 'wrap', gap: 0.5 }}>
                  <Chip size="small" label={agentName} variant="outlined" sx={{ height: 20, fontSize: 10 }} />
                  <Chip size="small" label={projectName} variant="outlined" sx={{ height: 20, fontSize: 10 }} />
                  {s.dependsOn.length === 0 && <Chip size="small" label="入口" color="primary" sx={{ height: 20, fontSize: 10 }} />}
                  {s.when && <Chip size="small" label={s.when} color="warning" variant="outlined" sx={{ height: 20, fontSize: 10 }} />}
                  {s.maxRetries !== undefined && s.maxRetries !== 2 && <Chip size="small" label={`重试 ${s.maxRetries}`} variant="outlined" sx={{ height: 20, fontSize: 10 }} />}
                </Stack>
              </Paper>
            )
          })}
        </Box>

        {stages.length === 0 && (
          <Box sx={{ position: 'absolute', inset: 0, display: 'grid', placeItems: 'center', pointerEvents: 'none' }}>
            <Typography color="text.secondary">点击「添加节点」开始绘制工作流</Typography>
          </Box>
        )}

        <Box
          onPointerDown={e => e.stopPropagation()}
          onClick={e => e.stopPropagation()}
          sx={{
            position: 'absolute', right: 12, bottom: 12, display: 'flex', alignItems: 'center', gap: 0.25,
            bgcolor: 'rgba(255,255,255,0.94)', border: '1px solid', borderColor: 'divider',
            borderRadius: 1.5, px: 0.5, py: 0.25, boxShadow: '0 2px 8px rgba(29,36,58,0.08)',
          }}
        >
          <Tooltip title="缩小"><IconButton size="small" onClick={() => zoomAt(1 / 1.2)}><ZoomOutIcon sx={{ fontSize: 16 }} /></IconButton></Tooltip>
          <Typography variant="caption" sx={{ minWidth: 38, textAlign: 'center', fontVariantNumeric: 'tabular-nums' }}>
            {Math.round(view.zoom * 100)}%
          </Typography>
          <Tooltip title="放大"><IconButton size="small" onClick={() => zoomAt(1.2)}><ZoomInIcon sx={{ fontSize: 16 }} /></IconButton></Tooltip>
          <Tooltip title="适应画布"><IconButton size="small" onClick={() => fitTo(stages)}><FitScreenIcon sx={{ fontSize: 16 }} /></IconButton></Tooltip>
        </Box>
      </Box>
    </Box>
  )
}

function nextKey(stages: WorkflowStage[]): string {
  for (let i = 1; i < 999; i++) {
    const key = `node-${i}`
    if (!stages.some(s => s.key === key)) return key
  }
  return `node-${Date.now()}`
}