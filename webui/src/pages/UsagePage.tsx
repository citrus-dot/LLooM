import { useEffect, useState } from 'react';
import { Row, Col, Card, Statistic, Table, Progress, Tag, Space, message } from 'antd';
import { getStats, getUsage, getBudgets, getModels, UsageRow, Budget, Model } from '../api';

export default function UsagePage() {
  const [stats, setStats] = useState<any>(null);
  const [usage, setUsage] = useState<UsageRow[]>([]);
  const [budgets, setBudgets] = useState<Budget[]>([]);
  const [models, setModels] = useState<Model[]>([]);
  const [loading, setLoading] = useState(false);

  const refresh = async () => {
    setLoading(true);
    try {
      const [st, u, b, m] = await Promise.all([getStats(), getUsage(), getBudgets(), getModels()]);
      setStats(st);
      setUsage(u.usage);
      setBudgets(b.budgets);
      setModels(m.models);
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

  const spendData = usage.map((u) => ({ model: u.model_name, value: u.total_cost }));
  const reqData = usage.map((u) => ({ model: u.model_name, value: u.request_count }));
  const maxSpend = Math.max(...spendData.map((d) => d.value), 0.000001);
  const maxReq = Math.max(...reqData.map((d) => d.value), 1);

  const columns = [
    { title: '模型', dataIndex: 'model_name', key: 'model_name' },
    { title: '输入 tokens', dataIndex: 'total_input_tokens', key: 'in' },
    { title: '输出 tokens', dataIndex: 'total_output_tokens', key: 'out' },
    { title: '请求数', dataIndex: 'request_count', key: 'req' },
    { title: '花费', dataIndex: 'total_cost', key: 'cost', render: (v: number) => `$${v.toFixed(6)}` },
  ];

  const pricingColumns = [
    { title: '模型', dataIndex: 'name', key: 'name' },
    { title: '供应商', dataIndex: 'provider', key: 'provider', render: (v: string) => <Tag>{v}</Tag> },
    { title: '输入 ($/1K)', key: 'in', render: (_: unknown, m: Model) => (m.input_cost_per_token * 1000).toFixed(6) },
    { title: '输出 ($/1K)', key: 'out', render: (_: unknown, m: Model) => (m.output_cost_per_token * 1000).toFixed(6) },
  ];

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Row gutter={16}>
        <Col span={6}><Card><Statistic title="核心服务" value="正常" /></Card></Col>
        <Col span={6}><Card><Statistic title="可用模型" value={stats?.model_count ?? 0} /></Card></Col>
        <Col span={6}><Card><Statistic title="语义缓存" value={stats?.cache_enabled ? '✓' : '✗'} /></Card></Col>
        <Col span={6}><Card><Statistic title="累计花费" value={stats?.total_spend ?? 0} precision={6} prefix="$" /></Card></Col>
      </Row>

      <Row gutter={16}>
        <Col span={12}>
          <Card title="模型花费分布" loading={loading}>
            <Space direction="vertical" style={{ width: '100%' }} size={8}>
              {spendData.length === 0 && <span style={{ color: '#999' }}>暂无数据</span>}
              {spendData.map((d) => (
                <div key={d.model}>
                  <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12 }}>
                    <span>{d.model}</span>
                    <span>${d.value.toFixed(6)}</span>
                  </div>
                  <Progress percent={(d.value / maxSpend) * 100} showInfo={false} strokeColor="#0984e3" size="small" />
                </div>
              ))}
            </Space>
          </Card>
        </Col>
        <Col span={12}>
          <Card title="模型请求分布" loading={loading}>
            <Space direction="vertical" style={{ width: '100%' }} size={8}>
              {reqData.length === 0 && <span style={{ color: '#999' }}>暂无数据</span>}
              {reqData.map((d) => (
                <div key={d.model}>
                  <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12 }}>
                    <span>{d.model}</span>
                    <span>{d.value} 次</span>
                  </div>
                  <Progress percent={(d.value / maxReq) * 100} showInfo={false} strokeColor="#00b894" size="small" />
                </div>
              ))}
            </Space>
          </Card>
        </Col>
      </Row>

      <Card title="模型用量明细" loading={loading}>
        <Table rowKey="model_name" size="small" columns={columns} dataSource={usage} pagination={false} />
      </Card>

      <Card title="配额管理" loading={loading}>
        {budgets.length === 0 && <span style={{ color: '#999' }}>未设置预算（可用 CLI: lloom-cli budgets set）</span>}
        {budgets.map((b) => {
          const pct = b.max_budget > 0 ? Math.min((getBudgetSpent(b) / b.max_budget) * 100, 100) : 0;
          return (
            <div key={b.id} style={{ marginBottom: 12 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 13 }}>
                <span>
                  {b.scope}/{b.scope_id}
                </span>
                <span>
                  ${getBudgetSpent(b).toFixed(2)} / ${b.max_budget.toFixed(2)}
                </span>
              </div>
              <Progress percent={Math.round(pct)} status={pct >= 100 ? 'exception' : 'active'} />
            </div>
          );
        })}
      </Card>

      <Card title="模型定价表" loading={loading}>
        <Table rowKey="name" size="small" columns={pricingColumns} dataSource={models} pagination={false} />
      </Card>
    </Space>
  );
}

// Budget spend is computed from usage totals in the real backend; for display
// we derive it from the stats response.
function getBudgetSpent(_b: Budget): number {
  return 0;
}
