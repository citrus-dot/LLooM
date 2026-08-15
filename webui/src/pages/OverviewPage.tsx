import { useEffect, useState } from 'react';
import { Row, Col, Card, Statistic, Table, Button, Tag, Space, message, Descriptions, Modal } from 'antd';
import {
  PlayCircleOutlined,
  StopOutlined,
  ReloadOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
  FileTextOutlined,
} from '@ant-design/icons';
import {
  getServicesStatus,
  getStats,
  getServiceLogs,
  startService,
  stopService,
  restartService,
  ServiceStatus,
  UsageStats,
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

  useEffect(() => {
    refresh();
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
      render: (_: unknown, r: ServiceStatus) =>
        r.healthy ? <Tag color="success">健康</Tag> : <Tag color="error">异常</Tag>,
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
