import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Card, CardContent, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Alert, IconButton } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import EditIcon from '@mui/icons-material/Edit'
import DeleteIcon from '@mui/icons-material/Delete'
import { get, post, patch, del, type Skill } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton } from '../ui'

export default function SkillsPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const qc = useQueryClient()
  const isAdmin = role === 'owner' || role === 'admin'
  const { data: skills = [] } = useQuery({ queryKey: ['skills', wsId], queryFn: () => get<Skill[]>(`/workspaces/${wsId}/skills`) })
  const [editing, setEditing] = useState<Skill | 'new' | null>(null)
  const invalidate = () => qc.invalidateQueries({ queryKey: ['skills', wsId] })

  return (
    <Box sx={{ p: 3, overflowY: 'auto', height: '100%' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', mb: 2 }}>
        <Typography variant="h6" fontWeight={700} sx={{ flexGrow: 1 }}>SKILL 库</Typography>
        {isAdmin && <Button variant="contained" startIcon={<AddIcon />} onClick={() => setEditing('new')}>导入 SKILL</Button>}
      </Box>
      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 2 }}>
        以 SKILL.md 为入口的能力包；在智能体上授权后，运行时写入会话工作目录 .fux/skills/ 并在提示词中引用。
      </Typography>
      <Box sx={{ display: 'grid', gap: 2, gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))' }}>
        {skills.map(s => (
          <Card key={s.id}>
            <CardContent>
              <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
                <Typography fontWeight={700} sx={{ flexGrow: 1 }}>{s.name}</Typography>
                <Chip label={`v${s.version}`} variant="outlined" />
                {isAdmin && <>
                  <IconButton size="small" onClick={() => setEditing(s)}><EditIcon fontSize="small" /></IconButton>
                  <DeleteButton title="删除 SKILL" onDelete={() => del(`/skills/${s.id}`)} />
                </>}
              </Box>
              <Typography variant="body2" color="text.secondary" sx={{ mt: 1 }}>{s.description || '（无描述）'}</Typography>
              {s.source && <Typography variant="caption" color="text.secondary">来源：{s.source}</Typography>}
            </CardContent>
          </Card>
        ))}
        {skills.length === 0 && <Typography color="text.secondary">暂无 SKILL</Typography>}
      </Box>
      {editing && <SkillDialog s={editing === 'new' ? null : editing} wsId={wsId} onClose={() => setEditing(null)} onSaved={() => { invalidate(); setEditing(null) }} />}
    </Box>
  )
}

function SkillDialog({ s, wsId, onClose, onSaved }: { s: Skill | null; wsId: string; onClose: () => void; onSaved: () => void }) {
  const [name, setName] = useState(s?.name || '')
  const [description, setDescription] = useState(s?.description || '')
  const [source, setSource] = useState(s?.source || '')
  const [version, setVersion] = useState(s?.version || '1.0.0')
  const [content, setContent] = useState(s?.content || '---\nname: \ndescription: \n---\n\n# SKILL\n\n在此编写能力说明与步骤…')
  const [err, setErr] = useState('')

  const save = async () => {
    setErr('')
    try {
      const body = { name, description, source, version, content }
      if (s) await patch(`/skills/${s.id}`, body)
      else await post(`/workspaces/${wsId}/skills`, body)
      onSaved()
    } catch (e: any) { setErr(e.message) }
  }

  return (
    <Dialog open onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle>{s ? '编辑 SKILL' : '导入 SKILL'}</DialogTitle>
      <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
        <TextField label="名称 *" value={name} onChange={e => setName(e.target.value)} />
        <TextField label="描述" value={description} onChange={e => setDescription(e.target.value)} />
        <Box sx={{ display: 'flex', gap: 2 }}>
          <TextField label="来源（目录/Git/上传）" value={source} onChange={e => setSource(e.target.value)} sx={{ flexGrow: 1 }} />
          <TextField label="版本" value={version} onChange={e => setVersion(e.target.value)} sx={{ width: 110 }} />
        </Box>
        <TextField label="SKILL.md 内容" value={content} onChange={e => setContent(e.target.value)} multiline minRows={10} />
        {err && <Alert severity="error">{err}</Alert>}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>取消</Button>
        <Button variant="contained" onClick={save} disabled={!name.trim()}>保存</Button>
      </DialogActions>
    </Dialog>
  )
}