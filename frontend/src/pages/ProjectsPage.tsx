import { useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Box, Typography, Button, Table, TableHead, TableRow, TableCell, TableBody, Chip, Dialog, DialogTitle, DialogContent, DialogActions, TextField, MenuItem, Alert, IconButton, Tooltip } from '@mui/material'
import AddIcon from '@mui/icons-material/Add'
import DeleteIcon from '@mui/icons-material/Delete'
import NetworkCheckIcon from '@mui/icons-material/NetworkCheck'
import { get, post, del, type Project } from '../api'
import type { WsCtx } from './WorkspaceLayout'
import { DeleteButton } from '../ui'

export default function ProjectsPage() {
  const { wsId, role } = useOutletContext<WsCtx>()
  const qc = useQueryClient()
  const isAdmin = role === 'owner' || role === 'admin'
  const { data: projects = [] } = useQuery({ queryKey: ['projects', wsId], queryFn: () => get<Project[]>(`/workspaces/${wsId}/projects`) })
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const [kind, setKind] = useState<'git' | 'local'>('git')
  const [remoteUrl, setRemoteUrl] = useState('')
  const [localPath, setLocalPath] = useState('')
  const [defaultBranch, setDefaultBranch] = useState('main')
  const [err, setErr] = useState('')
  const [testMsg, setTestMsg] = useState<Record<string, string>>({})
  const invalidate = () => qc.invalidateQueries({ queryKey: ['projects', wsId] })

  const create = async () => {
    setErr('')
    try {
      await post(`/workspaces/${wsId}/projects`, { name, kind, remoteUrl: remoteUrl || undefined, localPath: localPath || undefined, defaultBranch })
      invalidate(); setOpen(false); setName(''); setRemoteUrl(''); setLocalPath('')
    } catch (e: any) { setErr(e.message) }
  }
  const test = async (id: string) => {
    setTestMsg(m => ({ ...m, [id]: '测试中…' }))
    const r = await post<{ ok: boolean; message?: string; defaultBranch?: string; isGitRepo?: boolean }>(`/projects/${id}/test-connection`)
    setTestMsg(m => ({ ...m, [id]: r.ok ? `连接正常${r.defaultBranch ? `（默认分支 ${r.defaultBranch}）` : ''}${r.isGitRepo === false ? '（非 git 目录，将用工作副本）' : ''}` : `失败：${r.message}` }))
  }

  return (
    <Box sx={{ p: 3, overflowY: 'auto', height: '100%' }}>
      <Box sx={{ display: 'flex', alignItems: 'center', mb: 2 }}>
        <Typography variant="h6" fontWeight={700} sx={{ flexGrow: 1 }}>项目</Typography>
        {isAdmin && <Button variant="contained" startIcon={<AddIcon />} onClick={() => setOpen(true)}>添加项目</Button>}
      </Box>
      <Typography variant="caption" color="text.secondary" display="block" sx={{ mb: 2 }}>
        支持 git 仓库（任意 remote）与本地目录。git 认证/代理由用户在执行节点本地配置，Fux 只调用 git CLI。任务在独立分支 fux/task-&lt;id&gt; 的 worktree 中执行。
      </Typography>
      <Table size="small">
        <TableHead><TableRow><TableCell>名称</TableCell><TableCell>类型</TableCell><TableCell>地址</TableCell><TableCell>默认分支</TableCell><TableCell>连接</TableCell><TableCell /></TableRow></TableHead>
        <TableBody>
          {projects.map(p => (
            <TableRow key={p.id} hover>
              <TableCell>{p.name}</TableCell>
              <TableCell><Chip label={p.kind} variant="outlined" /></TableCell>
              <TableCell><code>{p.remoteUrl || p.localPath}</code></TableCell>
              <TableCell>{p.defaultBranch}</TableCell>
              <TableCell>
                <Button size="small" startIcon={<NetworkCheckIcon />} onClick={() => test(p.id)}>测试</Button>
                {testMsg[p.id] && <Typography variant="caption" display="block" color={testMsg[p.id].startsWith('连接') ? 'success.main' : testMsg[p.id] === '测试中…' ? 'text.secondary' : 'error.main'}>{testMsg[p.id]}</Typography>}
              </TableCell>
              <TableCell align="right">
                {isAdmin && <DeleteButton title="删除项目" onDelete={() => del(`/projects/${p.id}`)} />}
              </TableCell>
            </TableRow>
          ))}
          {projects.length === 0 && <TableRow><TableCell colSpan={6}><Typography color="text.secondary">暂无项目</Typography></TableCell></TableRow>}
        </TableBody>
      </Table>

      <Dialog open={open} onClose={() => setOpen(false)} fullWidth maxWidth="xs">
        <DialogTitle>添加项目</DialogTitle>
        <DialogContent sx={{ display: 'grid', gap: 2, pt: '12px !important' }}>
          <TextField label="名称 *" value={name} onChange={e => setName(e.target.value)} />
          <TextField select label="类型" value={kind} onChange={e => setKind(e.target.value as any)}>
            <MenuItem value="git">git 仓库（remote URL）</MenuItem>
            <MenuItem value="local">本地目录</MenuItem>
          </TextField>
          {kind === 'git'
            ? <TextField label="Remote URL *" value={remoteUrl} onChange={e => setRemoteUrl(e.target.value)} placeholder="https://github.com/o/r.git 或 git@…" />
            : <TextField label="本地路径 *" value={localPath} onChange={e => setLocalPath(e.target.value)} placeholder="E:\repos\my-project" />}
          <TextField label="默认分支" value={defaultBranch} onChange={e => setDefaultBranch(e.target.value)} />
          {err && <Alert severity="error">{err}</Alert>}
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setOpen(false)}>取消</Button>
          <Button variant="contained" onClick={create} disabled={!name.trim() || (kind === 'git' ? !remoteUrl.trim() : !localPath.trim())}>添加</Button>
        </DialogActions>
      </Dialog>
    </Box>
  )
}