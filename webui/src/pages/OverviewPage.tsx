import { useEffect, useState } from 'react';
import { Row, Col, Card, Statistic, Table, Button, Tag, Space, message, Descriptions, Modal, Collapse } from 'antd';
import {
  PlayCircleOutlined,
  StopOutlined,
  ReloadOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  FileTextOutlined,
  PoweroffOutlined,
  SafetyCertificateOutlined,
} from '@ant-design/icons';
import {
  getServicesStatus,
  getStats,
  getServiceLogs,
  startService,
  stopService,
  restartService,
  shutdownAll,
  getRoutingReview,
  refreshRoutingReview,
  adoptRoutingSuggestion,
  ServiceStatus,
  UsageStats,
  RoutingReview,
  RoutingSuggestion,
} from '../api';

const SERVICE_KEY: Record<string, string> = {
  Ollama: 'ollama',
  'AI Service': 'ai',
};

export default function OverviewPage() {
  const [services, setServices] = useState<ServiceStatus[]>([]);
  const [stats, setStats] = useState<UsageStats | null>(null);
  const [loading, setLoading] = useState(false);
  const [logModal, setLogModal] = useState<{ name: string; content: string } | null>(null);
  const [review, setReview] = useState<RoutingReview | null>(null);
  const [reviewBusy, setReviewBusy] = useState(false);

  const refresh = async () => {
    setLoading(true);
    try {
      const [s, st] = await Promise.all([getServicesStatus(), getStats()]);
      setServices(s.services);
      setStats(st);
    } catch (e) {
      message.error(`加载失败: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  const refreshReview = async () => {
    try {
      setReview(await getRoutingReview());
    } catch {
      // 体检报告加载失败不阻塞页面其余部分
    }
  };

  const handleReviewRefresh = async () => {
    setReviewBusy(true);
    try {
      const r = await refreshRoutingReview();
      if (r.ok) {
        message.success('体检报告已生成');
      } else {
        message.warning(r.error ?? '生成失败');
      }
      await refreshReview();
    } catch (e) {
      message.error(`体检失败: ${e}`);
    } finally {
      setReviewBusy(false);
    }
  };

  const handleAdopt = async (taskType?: string) => {
    setReviewBusy(true);
    try {
      const r = await adoptRoutingSuggestion(taskType);
      if (r.ok) {
        message.success(taskType ? `已采纳 ${taskType} 的建议权重` : '已采纳全部建议权重');
      }
    } catch (e) {
      message.error(`采纳失败: ${e}`);
    } finally {
      setReviewBusy(false);
    }
  };

  useEffect(() => {
    refresh();
    refreshReview();
    const t = setInterval(refresh, 30000);
    return () => clearInterval(t);
  }, []);

  const healthyCount = services.filter((s) => s.healthy).length;

  const handleStart = async () => {
    await startService('ai');
    await startService('ollama');
    message.success('服务启动中...');
    setTimeout(refresh, 2000);
  };

  const handleStop = async () => {
    await stopService('ai');
    await stopService('ollama');
    message.info('服务已停止');
    setTimeout(refresh, 2000);
  };

  // Shut down everything (AI service + Ollama + the core server itself) so no
  // stale processes hold the ports. Used when the user is done and wants a
  // clean state for the next launch.
  const handleShutdownAll = () => {
    Modal.confirm({
      title: '关闭全部服务',
      content: '将关闭 AI 服务、Ollama 和主服务进程，页面将无法继续访问。确认关闭？',
      okText: '关闭全部',
      okType: 'danger',
      cancelText: '取消',
      onOk: async () => {
        try {
          await shutdownAll();
          message.info('服务正在关闭，本页面即将不可用...');
        } catch {
          // server may die before responding — that's expected
          message.info('服务已关闭');
        }
      },
    });
  };

  const handleRestart = async (name: string) => {
    await restartService(name);
    message.info(`${name} 重启中...`);
    setTimeout(refresh, 2000);
  };

  const handleStopOne = async (name: string, displayName: string) => {
    await stopService(name);
    message.success(`${displayName} 已停止`);
    setTimeout(refresh, 1000);
  };

  const handleLogs = async (name: string, displayName: string) => {
    try {
      const r = await getServiceLogs(name);
      setLogModal({ name: displayName, content: r.logs || '(暂无日志)' });
    } catch (e) {
      message.error(`获取日志失败: ${e}`);
    }
  };

  const columns = [
    {
      title: '服务名',
      dataIndex: 'name',
      key: 'name',
      render: (n: string) => <b>{n}</b>,
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      render: (s: string, r: ServiceStatus) => (
        <Space>
          {r.healthy ? (
            <CheckCircleOutlined style={{ color: '#52c41a' }} />
          ) : (
            <CloseCircleOutlined style={{ color: '#ff4d4f' }} />
          )}
          <span>{s}</span>
        </Space>
      ),
    },
    {
      title: '健康',
      key: 'healthy',
      render: (_: unknown, r: ServiceStatus) => (
        <Space direction="vertical" size={0}>
          <div>{r.healthy ? <Tag color="success">健康</Tag> : <Tag color="error">异常</Tag>}</div>
          {r.detail && <div style={{ color: '#faad14', fontSize: 12, maxWidth: 420 }}>{r.detail}</div>}
        </Space>
      ),
    },
    {
      title: '操作',
      key: 'action',
      render: (_: unknown, r: ServiceStatus) => {
        if (r.name === 'Core Server') return null;
        const key = SERVICE_KEY[r.name];
        if (!key) return null;
        return (
          <Space>
            <Button size="small" icon={<FileTextOutlined />} onClick={() => handleLogs(key, r.name)}>
              日志
            </Button>
            <Button size="small" icon={<ReloadOutlined />} onClick={() => handleRestart(key)}>
              重启
            </Button>
            {r.healthy ? (
              <Button size="small" danger icon={<StopOutlined />} onClick={() => handleStopOne(key, r.name)}>
                停止
              </Button>
            ) : (
              <Button size="small" type="primary" icon={<PlayCircleOutlined />} onClick={() => handleRestart(key)}>
                启动
              </Button>
            )}
          </Space>
        );
      },
    },
  ];

  const suggestionColumns = [
    { title: '任务', dataIndex: 'task_type', key: 'task_type', render: (t: string) => <Tag color="blue">{t}</Tag> },
    {
      title: '当前策略',
      key: 'current',
      render: (_: unknown, r: RoutingSuggestion) => (
        <div style={{ fontSize: 12 }}>
          <div>{r.current.model}（质量 {r.current.quality.toFixed(2)}）</div>
          <div style={{ color: '#999' }}>
            权重 cost {r.current.cost_weight} / quality {r.current.quality_weight}
          </div>
        </div>
      ),
    },
    {
      title: '建议改为',
      key: 'suggested',
      render: (_: unknown, r: RoutingSuggestion) => (
        <div style={{ fontSize: 12 }}>
          <div>
            {r.suggested.model}（质量 {r.suggested.quality.toFixed(2)}）
          </div>
          <div style={{ color: '#999' }}>
            权重 cost {r.suggested.cost_weight} / quality {r.suggested.quality_weight}
          </div>
        </div>
      ),
    },
    {
      title: '预计成本',
      key: 'cost',
      render: (_: unknown, r: RoutingSuggestion) => {
        const pct = r.current.est_cost > 0 ? ((r.suggested.est_cost - r.current.est_cost) / r.current.est_cost) * 100 : 0;
        return pct < 0 ? <Tag color="green">省 {(-pct).toFixed(1)}%</Tag> : <Tag color="orange">增 {pct.toFixed(1)}%</Tag>;
      },
    },
    {
      title: '操作',
      key: 'action',
      render: (_: unknown, r: RoutingSuggestion) => (
        <Button size="small" type="primary" loading={reviewBusy} onClick={() => handleAdopt(r.task_type)}>
          采纳
        </Button>
      ),
    },
  ];

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Row gutter={16}>
        <Col span={6}>
          <Card>
            <Statistic title="服务健康" value={healthyCount} suffix={`/ ${services.length}`} />
          </Card>
        </Col>
        <Col span={6}>
          <Card>
            <Statistic title="核心服务" value={services.some((s) => s.name === 'Core Server' && s.healthy) ? '运行中' : '异常'} />
          </Card>
        </Col>
        <Col span={6}>
          <Card>
            <Statistic
              title="Ollama"
              value={services.find((s) => s.name === 'Ollama')?.healthy ? '运行中' : '未运行'}
              valueStyle={{ color: services.find((s) => s.name === 'Ollama')?.healthy ? '#52c41a' : '#ff4d4f' }}
            />
          </Card>
        </Col>
        <Col span={6}>
          <Card>
            <Statistic title="累计花费" value={stats?.total_spend ?? 0} precision={6} prefix="$" />
          </Card>
        </Col>
      </Row>

      <Card
        title="服务列表"
        extra={
          <Space>
            <Button size="small" type="primary" icon={<PlayCircleOutlined />} onClick={handleStart}>
              启动
            </Button>
            <Button size="small" danger icon={<StopOutlined />} onClick={handleStop}>
              停止
            </Button>
            <Button size="small" danger icon={<PoweroffOutlined />} onClick={handleShutdownAll}>
              关闭全部服务
            </Button>
            <Button size="small" icon={<ReloadOutlined />} onClick={refresh}>
              刷新
            </Button>
          </Space>
        }
      >
        <Table rowKey="name" size="small" loading={loading} columns={columns} dataSource={services} pagination={false} />
      </Card>

      <Card title="智能路由统计" loading={loading}>
        <Descriptions column={4} size="small">
          <Descriptions.Item label="请求路由">
            <b>{Object.entries(stats?.routing_stats ?? {}).filter(([k]) => k.startsWith('route:')).reduce((a, [, v]) => a + v, 0)}</b>
          </Descriptions.Item>
          <Descriptions.Item label="正则分类">
            {Object.entries(stats?.routing_stats ?? {}).filter(([k]) => k.startsWith('rule:')).reduce((a, [, v]) => a + v, 0)}
          </Descriptions.Item>
          <Descriptions.Item label="LLM 分类">
            {Object.entries(stats?.routing_stats ?? {}).filter(([k]) => k.startsWith('llm:')).reduce((a, [, v]) => a + v, 0)}
          </Descriptions.Item>
          <Descriptions.Item label="语义缓存">
            {stats?.cache_enabled ? <Tag color="success">启用</Tag> : <Tag>未启用</Tag>}
          </Descriptions.Item>
        </Descriptions>
      </Card>

      <Card
        title={
          <Space>
            <SafetyCertificateOutlined />
            路由体检
            {review?.ok && (
              <span style={{ fontSize: 12, color: '#999' }}>
                （{review.created_at} · 样本 {review.samples}）
              </span>
            )}
          </Space>
        }
        extra={
          <Space>
            {(review?.suggestions?.length ?? 0) > 0 && (
              <Button size="small" loading={reviewBusy} onClick={() => handleAdopt()}>
                全部采纳
              </Button>
            )}
            <Button size="small" icon={<ReloadOutlined />} loading={reviewBusy} onClick={handleReviewRefresh}>
              立即体检
            </Button>
          </Space>
        }
      >
        <Collapse
          size="small"
          items={[
            {
              key: 'review',
              label: (
                <Space size={16}>
                  {review?.ok ? (
                    <>
                      <span>
                        AIQ <b>{(review.aiq ?? 0).toFixed(3)}</b>
                      </span>
                      <span>相对全强节省 {(review.saved_pct ?? 0).toFixed(1)}%</span>
                      <span style={{ color: '#999' }}>{review.conclusion}</span>
                    </>
                  ) : (
                    <span style={{ color: '#faad14' }}>{review?.error ?? '暂无体检报告'}</span>
                  )}
                </Space>
              ),
              children: review?.ok ? (
                <Space direction="vertical" size={12} style={{ width: '100%' }}>
                  <Descriptions column={3} size="small" bordered>
                    <Descriptions.Item label="全弱基线">
                      ${review.weak?.cost.toFixed(6) ?? '-'} · 质量 {review.weak?.quality.toFixed(3) ?? '-'}
                    </Descriptions.Item>
                    <Descriptions.Item label="当前策略">
                      ${review.current?.cost.toFixed(6) ?? '-'} · 质量 {review.current?.quality.toFixed(3) ?? '-'}
                    </Descriptions.Item>
                    <Descriptions.Item label="全强基线">
                      ${review.strong?.cost.toFixed(6) ?? '-'} · 质量 {review.strong?.quality.toFixed(3) ?? '-'}
                    </Descriptions.Item>
                  </Descriptions>
                  <div>
                    预算档触发分布（近 7 天）：
                    {Object.entries(review.budget_tiers ?? {}).length === 0 ? (
                      <span style={{ color: '#999' }}> 暂无数据</span>
                    ) : (
                      Object.entries(review.budget_tiers ?? {}).map(([tier, n]) => (
                        <Tag key={tier} style={{ marginLeft: 8 }}>
                          {tier}: {n}
                        </Tag>
                      ))
                    )}
                  </div>
                  {(review.suggestions?.length ?? 0) > 0 ? (
                    <>
                      <div style={{ fontWeight: 500 }}>权重建议（人工确认后生效，下一请求即用新权重）</div>
                      <Table
                        rowKey="task_type"
                        size="small"
                        columns={suggestionColumns}
                        dataSource={review.suggestions}
                        pagination={false}
                      />
                    </>
                  ) : (
                    <div style={{ color: '#999' }}>当前无权重调整建议（策略已无明显可优化空间，或影子样本不足）。</div>
                  )}
                </Space>
              ) : (
                <div style={{ color: '#999' }}>
                  影子评测自动按比例采样；也可到 Models 页手动双跑采集（POST /api/routing/shadow），积累样本后点「立即体检」。
                </div>
              ),
            },
          ]}
        />
      </Card>

      <Modal
        title={`${logModal?.name ?? ''} 日志`}
        open={logModal !== null}
        onCancel={() => setLogModal(null)}
        footer={
          <Button
            type="primary"
            onClick={() => {
              const key = SERVICE_KEY[logModal?.name ?? ''];
              if (key) handleLogs(key, logModal!.name);
            }}
          >
            刷新
          </Button>
        }
        width={720}
      >
        <pre
          style={{
            maxHeight: 480,
            overflow: 'auto',
            background: '#1e2030',
            color: '#c8d3f5',
            padding: 12,
            borderRadius: 6,
            fontSize: 12,
            whiteSpace: 'pre-wrap',
          }}
        >
          {logModal?.content}
        </pre>
      </Modal>
    </Space>
  );
}
