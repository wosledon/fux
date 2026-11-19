//! Fux API 客户端与类型（camelCase，与后端 serde 输出一致）。
export const API = '/api/v1'

let token = localStorage.getItem('fux_token') || ''
export const getToken = () => token
export function setToken(t: string) {
  token = t
  if (t) localStorage.setItem('fux_token', t)
  else localStorage.removeItem('fux_token')
}

export async function req<T = any>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(API + path, {
    method,
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  if (res.status === 401) {
    setToken('')
    if (!location.pathname.startsWith('/login')) location.href = '/login'
    throw new Error('未登录或登录已过期')
  }
  const data = await res.json().catch(() => null)
  if (!res.ok) throw new Error((data as any)?.error || `请求失败 (${res.status})`)
  return data as T
}

export const get = <T = any>(p: string) => req<T>('GET', p)
export const post = <T = any>(p: string, b?: unknown) => req<T>('POST', p, b)
export const patch = <T = any>(p: string, b?: unknown) => req<T>('PATCH', p, b)
export const put = <T = any>(p: string, b?: unknown) => req<T>('PUT', p, b)
export const del = <T = any>(p: string) => req<T>('DELETE', p)

export const taskEventsUrl = (taskId: string, after = 0) =>
  `${API}/tasks/${taskId}/events?token=${encodeURIComponent(token)}&after=${after}`
export const workspaceEventsUrl = (wsId: string) =>
  `${API}/workspaces/${wsId}/events?token=${encodeURIComponent(token)}`

// ---------- 类型 ----------
export interface User { id: string; username: string; displayName: string; createdAt: string }
export interface Workspace { id: string; name: string; settings: string; createdBy: string; createdAt: string }
export interface Project { id: string; workspaceId: string; name: string; kind: 'git' | 'local'; remoteUrl?: string; localPath?: string; defaultBranch: string }
export interface Board { id: string; workspaceId: string; config: string }
export type LaneKind = 'planning' | 'ready' | 'work' | 'review' | 'blocked' | 'done' | 'archived'
export interface Lane { id: string; boardId: string; kind: LaneKind; name: string; position: number; wipLimit?: number; stageRole?: string }
export type TaskStatus = 'planning' | 'ready' | 'in_progress' | 'review' | 'blocked' | 'done' | 'archived' | 'cancelled'
export interface Task {
  id: string; boardId: string; workspaceId: string; typeId?: string; projectId?: string
  title: string; content: string; status: TaskStatus; blockedFrom?: string; blockReason?: string
  laneId?: string; priority: number; tags: string; assigneeAgentId?: string; workflowId?: string
  branch?: string; prUrl?: string; prState?: string; mergeCommit?: string
  retryCount: number; version: number; createdBy?: string; createdAt: string; updatedAt: string
  workflowName?: string; stageTotal?: number; stageDone?: number
  commentCount?: number; runCount?: number; lastRunStatus?: string
}
export interface TaskType { id: string; workspaceId: string; name: string; icon: string; titleTemplate: string; contentTemplate: string; variablesSchema: string; defaults: string }
export interface CliEntry { id: string; workspaceId?: string; name: string; kind: string; command: string; args: string; envTemplate: string; isDefault: number }
export interface Agent {
  id: string; workspaceId: string; name: string; cliEntryId: string; model: string; reasoningEffort: string
  systemPrompt: string; policy: string; gates: string; maxConcurrent: number; paused: number; matchTags: string
  skillIds?: string[]; mcpServers?: { id: string; toolWhitelist: string[] }[]
}
export interface WorkflowStage {
  key: string; name: string; agentId: string; projectId?: string
  promptAppend?: string; dependsOn: string[]; when?: string; x?: number; y?: number
}
export interface Workflow {
  id: string; workspaceId: string; name: string; description: string
  stages: string; createdAt: string; updatedAt: string
}
export interface Delivery {
  id: string; taskId: string; projectId: string; nodeKey: string
  branch?: string; prUrl?: string; prState?: string; mergeCommit?: string; createdAt: string
}
export interface Skill { id: string; workspaceId: string; name: string; description: string; source: string; version: string; content: string }
export interface McpServer { id: string; workspaceId: string; name: string; transport: 'stdio' | 'sse' | 'http'; command?: string; args: string; url?: string; envJson: string; status: string }
export interface Run { id: string; taskId: string; agentId: string; nodeId: string; stage: number; stageKey?: string; projectId?: string; branch?: string; headSha?: string; retryCount: number; status: string; stopReason?: string; turnCount: number; usageTokens: number; startedAt: string; endedAt?: string }
export interface NodeInfo { id: string; name: string; mode: string; approved: number; connStatus: string; capabilities: string; lastHeartbeat?: string; createdAt: string }
export interface TaskEvent { id: number; taskId: string; seq: number; kind: string; actor: string; runId?: string; payload: string; createdAt: string }
export interface Envelope { workspaceId: string; taskId: string; seq: number; kind: string; actor: string; runId?: string; payload: any; createdAt: string }
export interface Member { userId: string; username: string; displayName: string; role: string; joinedAt: string }
export interface Invitation { code: string; workspaceId: string; role: string; expiresAt?: string; oneTime: number; status: string; createdAt: string }
export interface Approval { id: string; runId: string; taskId: string; request: string; status: string; createdAt: string }
export interface Notification {
  id: string; workspaceId: string; kind: string; taskId?: string
  message: string; readBy: string; createdAt: string
}
export interface NotificationItem { notification: Notification; read: boolean }
export interface AssistantSession {
  id: string; title: string; createdAt: string; updatedAt: string; messageCount: number; preview?: string
}
export interface AssistantMessage {
  id: number; role: 'user' | 'assistant' | 'process'; content: string; createdAt: string
}

export const STATUS_LABEL: Record<TaskStatus, string> = {
  planning: '待规划', ready: '待办', in_progress: '进行中', review: '审核中',
  blocked: '已阻塞', done: '已完成', archived: '已归档', cancelled: '已取消',
}
export const STATUS_COLOR: Record<TaskStatus, 'default' | 'primary' | 'secondary' | 'warning' | 'error' | 'success' | 'info'> = {
  planning: 'default', ready: 'info', in_progress: 'primary', review: 'secondary',
  blocked: 'error', done: 'success', archived: 'default', cancelled: 'default',
}
export const STAGE_LABEL: Record<string, string> = {
  pending: '待执行', running: '执行中', succeeded: '已完成', failed: '失败', cancelled: '已取消', skipped: '已跳过',
}
export const STAGE_COLOR: Record<string, 'default' | 'primary' | 'success' | 'error' | 'warning' | 'info'> = {
  pending: 'default', running: 'primary', succeeded: 'success', failed: 'error', cancelled: 'warning', skipped: 'default',
}
export const parseJson = <T = any>(s: string | undefined | null, fallback: T): T => {
  if (!s) return fallback
  try { return JSON.parse(s) as T } catch { return fallback }
}