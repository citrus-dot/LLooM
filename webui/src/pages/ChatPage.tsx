import { useEffect, useState } from 'react';
import { Avatar, Button, Input, Layout, List, Spin, Tag, Tooltip } from 'antd';
import { PlusOutlined, SendOutlined, DeleteOutlined, EditOutlined, RobotOutlined, UserOutlined } from '@ant-design/icons';
import {
  useChat,
  send,
  selectConv,
  newConv,
  deleteConv,
  renameConv,
  setInput,
  refreshConvs,
  DisplayMsg,
  PlanView,
} from '../store/chatStore';
import Markdown from '../components/Markdown';
import { cacheFeedback, cacheThresholdGet } from '../api';

const { Sider, Content } = Layout;

const CHAT_MAX = 'min(94vw, 1080px)';
const BUBBLE_MAX = 'min(80vw, 860px)';

// Lightweight inline question that drives threshold self-tuning. No 👍/👎 system:
// - on a cache HIT: "did this cached answer solve it?" (labels correct/incorrect hits)
// - on a near-threshold MISS (gray zone): "was this actually a duplicate?" (labels
//   false negatives). Only the gray zone is asked, so it stays low-volume.
function CacheFeedback({
  sim,
  isHit,
  threshold,
}: {
  sim?: number;
  isHit: boolean;
  threshold: number;
}) {
  const [answered, setAnswered] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const send = async (decision: 'hit' | 'miss', correct: boolean) => {
    setBusy(true);
    try {
      await cacheFeedback(sim ?? 0, decision, correct);
    } catch {
      /* best-effort */
    }
    setBusy(false);
    setAnswered(correct);
  };
  if (answered !== null) {
    return (
      <div style={{ marginTop: 4, fontSize: 12, color: '#999' }}>
        {answered ? '已记录，感谢反馈' : '已记录，会据此优化缓存'}
      </div>
    );
  }
  // Only ask on hits, or on misses whose similarity sits in the gray zone just
  // below the current threshold (those are the ambiguous near-duplicates).
  if (!isHit && (sim == null || sim < threshold - 0.06)) return null;
  const prompt = isHit
    ? '这条缓存回答解决了你的问题吗？'
    : '这个问题与之前问过的相似吗？';
  return (
    <div
      style={{
        marginTop: 4,
        fontSize: 12,
        color: '#666',
        display: 'flex',
        gap: 8,
        alignItems: 'center',
        flexWrap: 'wrap',
      }}
    >
      <span>{prompt}</span>
      <Button size="small" loading={busy} onClick={() => send(isHit ? 'hit' : 'miss', true)}>
        是
      </Button>
      <Button size="small" loading={busy} onClick={() => send(isHit ? 'hit' : 'miss', false)}>
        否
      </Button>
    </div>
  );
}

function PlanCard({ plan }: { plan: PlanView }) {
  const running = plan.sub_tasks.find((t) => t.status === 'running');
  return (
    <div className="plan-card">
      <div className="plan-head">
        <span className="plan-title">智能编排计划</span>
        {plan.models_used && plan.models_used.length > 0 && (
          <span className="plan-models">
            {plan.models_used.map((m) => (
              <Tag key={m} color="geekblue" style={{ marginInlineEnd: 4 }}>
                {m}
              </Tag>
            ))}
          </span>
        )}
      </div>
      <ol className="plan-list">
        {plan.sub_tasks.map((t) => (
          <li key={t.id} className="plan-item">
            <span className={`plan-dot ${t.status ?? 'done'}`} />
            <span className="plan-desc">{t.description}</span>
            {t.model && (
              <Tooltip title={`子任务 #${t.id} 执行模型`}>
                <Tag
                  color={t.status === 'running' ? 'processing' : t.status === 'failed' ? 'error' : 'blue'}
                  className="plan-model"
                >
                  {t.model}
                </Tag>
              </Tooltip>
            )}
            {t.status === 'running' && <span className="plan-running">进行中…</span>}
            {t.status === 'failed' && t.error && (
              <Tooltip title={t.error}>
                <span className="plan-error">失败</span>
              </Tooltip>
            )}
          </li>
        ))}
      </ol>
      {plan.aggregator && (
        <div className="plan-foot">
          汇总模型：<Tag color="purple">{plan.aggregator}</Tag>
        </div>
      )}
      {running && <div className="plan-active">正在执行：{running.model}</div>}
    </div>
  );
}

export default function ChatPage() {
  const state = useChat();
  const convs = state.convs;
  const activeId = state.activeId;
  const cur = activeId === null ? state.draft : (state.convMap[activeId] ?? { messages: [], loading: false, loaded: false, input: '' });

  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  // Current semantic-cache threshold, used to decide the gray-zone miss prompt.
  const [curThr, setCurThr] = useState(0.8);

  useEffect(() => {
    refreshConvs();
    cacheThresholdGet()
      .then((r) => setCurThr(r.threshold))
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const commitRename = async (id: string) => {
    const t = editValue.trim();
    setEditingId(null);
    setEditValue('');
    if (t) await renameConv(id, t);
  };

  return (
    <Layout style={{ background: 'transparent', height: 'calc(100vh - 130px)' }}>
      <Sider width={260} theme="light" style={{ borderRight: '1px solid #f0f0f0', padding: 12 }}>
        <Button type="primary" block icon={<PlusOutlined />} onClick={newConv}>
          新建对话
        </Button>
        <div style={{ marginTop: 12, maxHeight: 'calc(100vh - 220px)', overflow: 'auto' }}>
          <List
            size="small"
            dataSource={convs}
            renderItem={(c) => {
              const isEditing = editingId === c.id;
              return (
                <List.Item
                  key={c.id}
                  style={{
                    cursor: 'pointer',
                    padding: '8px 12px',
                    borderRadius: 6,
                    background: c.id === activeId ? '#e6f4ff' : 'transparent',
                  }}
                  onClick={() => !isEditing && selectConv(c.id)}
                  onMouseEnter={() => setHoveredId(c.id)}
                  onMouseLeave={() => setHoveredId(null)}
                  actions={
                    isEditing
                      ? []
                      : [
                          <EditOutlined
                            key="edit"
                            title="重命名"
                            style={{ opacity: hoveredId === c.id ? 1 : 0, transition: 'opacity 0.15s' }}
                            onClick={(e) => {
                              e.stopPropagation();
                              setEditValue(c.title || '');
                              setEditingId(c.id);
                            }}
                          />,
                          <DeleteOutlined
                            key="del"
                            onClick={(e) => {
                              e.stopPropagation();
                              deleteConv(c.id);
                            }}
                          />,
                        ]
                  }
                >
                  {isEditing ? (
                    <Input
                      autoFocus
                      size="small"
                      value={editValue}
                      onChange={(e) => setEditValue(e.target.value)}
                      onClick={(e) => e.stopPropagation()}
                      onPressEnter={() => commitRename(c.id)}
                      onBlur={() => commitRename(c.id)}
                      style={{ fontSize: 13 }}
                    />
                  ) : (
                    <List.Item.Meta
                      title={<span style={{ fontSize: 13 }}>{c.title || '新对话'}</span>}
                      description={<span style={{ fontSize: 12, color: '#999' }}>{c.message_count} 条</span>}
                    />
                  )}
                </List.Item>
              );
            }}
          />
          {convs.length === 0 && (
            <div style={{ color: '#999', textAlign: 'center', padding: 24 }}>
              暂无对话
              <br />
              输入消息自动新建
            </div>
          )}
        </div>
      </Sider>

      <Content style={{ padding: '0 16px', display: 'flex', flexDirection: 'column' }}>
        <div
          style={{
            flex: 1,
            overflow: 'auto',
            background: '#fff',
            border: '1px solid #f0f0f0',
            borderRadius: 8,
            padding: 16,
            marginBottom: 12,
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
          }}
        >
          <div style={{ width: '100%', maxWidth: CHAT_MAX }}>
            {cur.messages.length === 0 && !cur.loading && (
              <div style={{ color: '#999', textAlign: 'center', padding: 40 }}>
                你好！我是智能编排助手。复杂任务会自动分解为子任务，选择最优模型执行。
              </div>
            )}
            {cur.messages.map((m: DisplayMsg, i) =>
              m.role === 'user' ? (
                <div key={i} style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 12 }}>
                  <div style={{ maxWidth: BUBBLE_MAX, display: 'flex', gap: 8, alignItems: 'flex-start' }}>
                    <div
                      style={{
                        background: '#e6f4ff',
                        padding: '8px 12px',
                        borderRadius: 8,
                        whiteSpace: 'pre-wrap',
                        wordBreak: 'break-word',
                      }}
                    >
                      {m.content}
                    </div>
                    <Avatar size="small" icon={<UserOutlined />} style={{ width: 24, height: 24, flexShrink: 0 }} />
                  </div>
                </div>
              ) : (
                <div key={i} style={{ display: 'flex', justifyContent: 'flex-start', marginBottom: 12 }}>
                  <div style={{ maxWidth: BUBBLE_MAX, display: 'flex', gap: 8, alignItems: 'flex-start' }}>
                    <Avatar
                      size="small"
                      icon={<RobotOutlined />}
                      style={{ background: '#0984e3', width: 24, height: 24, flexShrink: 0 }}
                    />
                    <div style={{ minWidth: 0, width: '100%' }}>
                      {m.status === 'interrupted' && (
                        <div style={{ marginBottom: 4 }}>
                          <Tag color="warning">回答中断（服务关闭前未完成）</Tag>
                        </div>
                      )}
                      {m.plan && (
                        <div style={{ marginBottom: 8 }}>
                          <PlanCard plan={m.plan} />
                        </div>
                      )}
                      <div style={{ background: '#f6f8fa', padding: '8px 12px', borderRadius: 8 }}>
                        {m.content ? (
                          <Markdown content={m.content} />
                        ) : cur.loading ? (
                          <span style={{ color: '#999' }}>
                            <Spin size="small" /> 思考中…
                          </span>
                        ) : null}
                      </div>
                      {m.detail && (
                        <div style={{ marginTop: 4 }}>
                          <Tag color="blue">{m.detail}</Tag>
                        </div>
                      )}
                      {m.cacheHit !== undefined && (
                        <CacheFeedback sim={m.cacheSim} isHit={!!m.cacheHit} threshold={curThr} />
                      )}
                    </div>
                  </div>
                </div>
              ),
            )}
          </div>
        </div>

        <div style={{ display: 'flex', gap: 8, maxWidth: CHAT_MAX, width: '100%', margin: '0 auto' }}>
          <Input
            value={cur.input}
            onChange={(e) => setInput(e.target.value)}
            onPressEnter={send}
            placeholder="输入消息... (Enter 发送)"
            disabled={cur.loading}
          />
          <Button type="primary" icon={<SendOutlined />} onClick={send} loading={cur.loading}>
            发送
          </Button>
        </div>
      </Content>
    </Layout>
  );
}
