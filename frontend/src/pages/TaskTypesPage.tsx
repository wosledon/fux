import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Table, TableHead, TableRow, TableCell, TableBody, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Alert, IconButton, Tooltip } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import EditIcon from '@mui/icons-material/Edit'
import DeleteIcon from '@mui/icons-material/Delete'
import { get, post, patch, del, type TaskType } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton } from '../ui'

export default function TaskTypesPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const qc = useQueryClient()
  const isAdmin = role === 'owner' || role === 'admin'
  const { data: types = [] } = useQuery({ queryKey: ['types', wsId], queryFn: () => get<TaskType[]>(`/workspaces/${wsId}/task-types`) })
  const [editing, setEditing] = useState<TaskType | 'new' | null>(null)
  const invalidate = () => qc.invalidateQueries({ queryKey: ['types', wsId] })

  return (
    <Box sx={{ p: { xs: 2, md: 3.5 }, overflowY: 'auto', height: '100%', width: '100%', maxWidth: 1500, mx: 'auto' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', mb: 2 }}>
        <Typography variant="h6" fontWeight={700} sx={{ flexGrow: 1 }}>任务类型（模板）</Typography>
        {isAdmin && <Button variant="contained" startIcon={<AddIcon />} onClick={() => setEditing('new')}>新建类型</Button>}
      </Box>
      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 2 }}>
        模板语法 {'{{变量名}}'}；建卡时填变量一键生成。在看板「新建任务」中选择类型即可快速建卡。
      </Typography>
      <Table size="small">
        <TableHead><TableRow><TableCell>图标</TableCell><TableCell>名称</TableCell><TableCell>标题模板</TableCell><TableCell>变量</TableCell><TableCell /></TableRow></TableHead>
        <TableBody>
          {types.map(t => {
            let vars: string[] = []
            try { vars = (JSON.parse(t.variablesSchema) as any[]).map(v => v.name) } catch { /* ignore */ }
            return (
              <TableRow key={t.id} hover>
                <TableCell>{t.icon}</TableCell>
                <TableCell>{t.name}</TableCell>
                <TableCell><code>{t.titleTemplate}</code></TableCell>
                <TableCell>{vars.join(', ') || '—'}</TableCell>
                <TableCell align="right">
                  {isAdmin && <>
                    <Tooltip title="编辑"><IconButton size="small" onClick={() => setEditing(t)}><EditIcon fontSize="small" /></IconButton></Tooltip>
                    <DeleteButton title="删除类型" onDelete={() => del(`/task-types/${t.id}`)} />
                  </>}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>
      {editing && <TypeDialog t={editing === 'new' ? null : editing} wsId={wsId} onClose={() => setEditing(null)} onSaved={() => { invalidate(); setEditing(null) }} />}
    </Box>
  )
}

function TypeDialog({ t, wsId, onClose, onSaved }: { t: TaskType | null; wsId: string; onClose: () => void; onSaved: () => void }) {
  const [name, setName] = useState(t?.name || '')
  const [icon, setIcon] = useState(t?.icon || '📋')
  const [titleTemplate, setTitleTemplate] = useState(t?.titleTemplate || '{{title}}')
  const [contentTemplate, setContentTemplate] = useState(t?.contentTemplate || '')
  const [schema, setSchema] = useState(t?.variablesSchema || '[{"name":"title","label":"标题","required":true}]')
  const [defaults, setDefaults] = useState(t?.defaults || '{}')
  const [err, setErr] = useState('')

  const save = async () => {
    setErr('')
    try {
      const body = { name, icon, titleTemplate, contentTemplate, variablesSchema: JSON.parse(schema), defaults: JSON.parse(defaults) }
      if (t) await patch(`/task-types/${t.id}`, body)
      else await post(`/workspaces/${wsId}/task-types`, body)
      onSaved()
    } catch (e: any) { setErr(e.message) }
  }

  return (
    <Dialog open onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>{t ? '编辑任务类型' : '新建任务类型'}</DialogTitle>
      <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
        <Box sx={{ display: 'flex', gap: 2 }}>
          <TextField label="图标" value={icon} onChange={e => setIcon(e.target.value)} sx={{ width: 90 }} />
          <TextField label="名称 *" value={name} onChange={e => setName(e.target.value)} sx={{ flexGrow: 1 }} />
        </Box>
        <TextField label="标题模板" value={titleTemplate} onChange={e => setTitleTemplate(e.target.value)} />
        <TextField label="内容（提示词）模板" value={contentTemplate} onChange={e => setContentTemplate(e.target.value)} multiline minRows={5} />
        <TextField label="变量定义（JSON 数组：name/label/required/default）" value={schema} onChange={e => setSchema(e.target.value)} multiline minRows={3} />
        <TextField label="默认属性（JSON：projectId/priority 等）" value={defaults} onChange={e => setDefaults(e.target.value)} multiline minRows={2} />
        {err && <Alert severity="error">{err}</Alert>}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>取消</Button>
        <Button variant="contained" onClick={save} disabled={!name.trim()}>保存</Button>
      </DialogActions>
    </Dialog>
  )
}