import { useEffect, useState } from 'react';
import { Row, Col, Card, Statistic, Table, Progress, Tag, Space, message, Button, Modal, Form, Input, InputNumber, Tooltip } from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import { getStats, getUsage, getBudgets, setBudget, checkBudget, getModels, getReconcileSummary, ReconcileSummary, UsageRow, Budget, Model } from '../api';

const CNY_PER_USD = 7.2;

export default function UsagePage() {
  const [stats, setStats] = useState<any>(null);
  const [usage, setUsage] = useState<UsageRow[]>([]);
  const [totalCacheSaved, setTotalCacheSaved] = useState(0);
  const [budgets, setBudgets] = useState<Budget[]>([]);
  const [budgetSpent, setBudgetSpent] = useState<Record<string, number>>({});
  const [models, setModels] = useState<Model[]>([]);
  const [reconcile, setReconcile] = useState<ReconcileSummary | null>(null);
  const [loading, setLoading] = useState(false);
  const [budgetModal, setBudgetModal] = useState(false);
  const [form] = Form.useForm();

  const refresh = async () => {
    setLoading(true);
    try {
      const [st, u, b, m, rc] = await Promise.all([
        getStats(), getUsage(), getBudgets(), getModels(),
        // 对账报告是脚本离线产物，端点永不应 5xx；兜底 null 防御旧后端
        getReconcileSummary().catch(() => null),
      ]);
      setStats(st);
      setUsage(u.usage);
      setTotalCacheSaved(u.total_cache_saved ?? 0);
      setBudgets(b.budgets);
      setModels(m.models);
      setReconcile(rc);
      // Real spend per budget via check API.
      const spent: Record<string, number> = {};
      for (const budget of b.budgets) {
        try {
          const r = await checkBudget(budget.scope, budget.scope_id);
          spent[`${budget.scope}/${budget.scope_id}`] = r.spent;
        } catch {}
      }
      setBudgetSpent(spent);
    } catch (e) {
      message.error(`加载失败: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  const handleAddBudget = async () => {
    try {
      const v = await form.validateFields();
      await setBudget(v.scope, v.scopeId, v.maxBudget, v.duration || '30d');
      message.success('预算已设置');
      setBudgetModal(false);
      form.resetFields();
      refresh();
    } catch (e) {
      message.error(`设置失败: ${e}`);
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

  // B2 账单对账徽标：报告来自 scripts/bill_reconcile.py --save（离线产物）
  const reconcileBadge = reconcile?.reconciled ? (
    <Tooltip title={`百炼账单对账一致（${reconcile.models_ok}/${reconcile.models_matched} 模型） · ${reconcile.generated_at}`}>
      <Tag color="green" style={{ marginLeft: 8, verticalAlign: 'middle' }}>已对账</Tag>
    </Tooltip>
  ) : reconcile?.verdict ? (
    <Tooltip title={`对账偏差 ${reconcile.dev_pct ?? 'n/a'}%（${reconcile.models_ok}/${reconcile.models_matched} 模型对平） · ${reconcile.generated_at}`}>
      <Tag color="orange" style={{ marginLeft: 8, verticalAlign: 'middle' }}>对账偏差</Tag>
    </Tooltip>
  ) : null;

  const columns = [
    { title: '模型', dataIndex: 'model_name', key: 'model_name' },
    { title: '输入 tokens', dataIndex: 'total_input_tokens', key: 'in' },
    { title: '输出 tokens', dataIndex: 'total_output_tokens', key: 'out' },
    { title: '请求数', dataIndex: 'request_count', key: 'req' },
    { title: '缓存命中', dataIndex: 'cache_hits', key: 'cache', render: (v: number) => (v ? <Tag color="green">{v}</Tag> : '-') },
    {
      title: '缓存节省',
      key: 'saved',
      render: (_: unknown, r: UsageRow) => (r.cache_saved ? `¥${(r.cache_saved * CNY_PER_USD).toFixed(2)}` : '-'),
    },
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

      <Card
        size="small"
        style={{ borderColor: '#a0d911', background: '#fcffe6' }}
      >
        <Statistic
          title={<span>缓存为您节省{reconcileBadge}</span>}
          value={totalCacheSaved * CNY_PER_USD}
          precision={2}
          prefix="¥"
          suffix="（语义缓存命中省下的费用）"
          valueStyle={{ color: '#7cb305' }}
        />
      </Card>

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

      <Card
        title="配额管理"
        loading={loading}
        extra={
          <Button size="small" type="primary" icon={<PlusOutlined />} onClick={() => setBudgetModal(true)}>
            设置预算
          </Button>
        }
      >
        {budgets.length === 0 && <span style={{ color: '#999' }}>未设置预算（点右上角「设置预算」或 CLI: lloom-cli budgets set）</span>}
        {budgets.map((b) => {
          const spent = budgetSpent[`${b.scope}/${b.scope_id}`] ?? 0;
          const pct = b.max_budget > 0 ? Math.min((spent / b.max_budget) * 100, 100) : 0;
          return (
            <div key={b.id} style={{ marginBottom: 12 }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 13 }}>
                <span>
                  {b.scope}/{b.scope_id}
                </span>
                <span>
                  ${spent.toFixed(4)} / ${b.max_budget.toFixed(2)}
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

      <Modal
        title="设置预算"
        open={budgetModal}
        onCancel={() => setBudgetModal(false)}
        onOk={handleAddBudget}
        okText="保存"
      >
        <Form form={form} layout="vertical">
          <Form.Item name="scope" label="范围" rules={[{ required: true }]}>
            <Input placeholder="user / model" />
          </Form.Item>
          <Form.Item name="scopeId" label="范围 ID" rules={[{ required: true }]}>
            <Input placeholder="如 default 或模型名" />
          </Form.Item>
          <Form.Item name="maxBudget" label="上限 ($)" rules={[{ required: true }]}>
            <InputNumber min={0} step={0.01} style={{ width: '100%' }} />
          </Form.Item>
          <Form.Item name="duration" label="周期" initialValue="30d">
            <Input placeholder="30d / 7d / 1d" />
          </Form.Item>
        </Form>
      </Modal>
    </Space>
  );
}
