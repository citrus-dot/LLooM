import { useEffect, useMemo, useState } from 'react';
import {
  Table,
  Button,
  Space,
  Tag,
  Modal,
  Form,
  InputNumber,
  Switch,
  Popconfirm,
  message,
  Card,
  Statistic,
  Alert,
  Tooltip,
} from 'antd';
import { ReloadOutlined } from '@ant-design/icons';
import {
  PriceSpec,
  CalibrationRow,
  ProbeStats,
  listPriceSpecs,
  updatePriceSpec,
  acceptPriceSpec,
  refreshPricing,
  listPriceCalibration,
  getProbeStats,
  setProbeBudget,
} from '../api';

// price_source → 徽标语义（PRICING-PLAN §3.3 来源分级：manual>overlay>litellm_remote>packaged>heuristic）
const SOURCE_META: Record<string, { color: string; label: string }> = {
  manual: { color: 'green', label: 'manual' },
  overlay: { color: 'purple', label: 'overlay' },
  litellm_remote: { color: 'blue', label: 'litellm_remote' },
  litellm_packaged: { color: 'cyan', label: 'litellm_packaged' },
  heuristic: { color: 'orange', label: 'heuristic' },
};
const fallbackMeta = { color: 'default', label: 'unknown' };

function usdPerTokToDollarsPer1k(v: number): string {
  return (v * 1000).toFixed(6);
}

/** Tooltip text for a stale price. Explains the trigger; shows the stored reason. */
function staleTooltip(s: PriceSpec): string {
  if (s.price_stale) {
    const reason =
      s.stale_reason === 'calibration_drift'
        ? '对账偏差：连续3天 真实成本/估算成本 超出 [0.8, 1.2]'
        : (s.stale_reason ?? '已标记过期');
    return `${reason}。建议核对官方价后「改价」或「采纳」`;
  }
  return '';
}

export default function PricingPage() {
  const [specs, setSpecs] = useState<PriceSpec[]>([]);
  const [loading, setLoading] = useState(false);
  const [staleOnly, setStaleOnly] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  const [calRows, setCalRows] = useState<CalibrationRow[]>([]);
  const [probe, setProbe] = useState<ProbeStats | null>(null);

  const [editOpen, setEditOpen] = useState(false);
  const [editing, setEditing] = useState<PriceSpec | null>(null);
  const [form] = Form.useForm();

  const refresh = async () => {
    setLoading(true);
    try {
      const [s, c, p] = await Promise.all([
        listPriceSpecs(staleOnly),
        listPriceCalibration(30),
        getProbeStats(),
      ]);
      setSpecs(s);
      setCalRows(c);
      setProbe(p);
    } catch (e) {
      message.error(`加载失败: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleRefreshRemote = async () => {
    setRefreshing(true);
    try {
      const r = await refreshPricing();
      message.success(`刷新完成：更新 ${r.updated} 条，远端 ${r.remote_total} 条，保留 manual ${r.manual_kept} 条`);
      refresh();
    } catch (e) {
      message.error(`刷新失败（断网/镜像不可达，本地价保留）: ${e}`);
    } finally {
      setRefreshing(false);
    }
  };

  const openEdit = (s: PriceSpec) => {
    setEditing(s);
    form.setFieldsValue({
      input_price: s.input_cost * 1000,
      output_price: s.output_cost * 1000,
      cache_read_price: s.cache_read_cost == null ? undefined : s.cache_read_cost * 1000,
    });
    setEditOpen(true);
  };

  const handleSave = async () => {
    const v = await form.validateFields();
    if (!editing) return;
    try {
      // 录入口以 $/1K token 输入，转回 per-token；改价强制 source=manual
      await updatePriceSpec(editing.provider, editing.model, {
        input_cost: v.input_price / 1000,
        output_cost: v.output_price / 1000,
        cache_read_cost: v.cache_read_price == null ? null : v.cache_read_price / 1000,
      });
      message.success('价格已更新并标记为 manual（刷新不再覆盖）');
      setEditOpen(false);
      refresh();
    } catch (e) {
      message.error(`保存失败: ${e}`);
    }
  };

  const handleAccept = async (s: PriceSpec) => {
    try {
      await acceptPriceSpec(s.provider, s.model);
      message.success(`${s.provider}/${s.model} 已采纳为 manual`);
      refresh();
    } catch (e) {
      message.error(`采纳失败: ${e}`);
    }
  };

  const onBudgetSave = async (v: number) => {
    try {
      const r = await setProbeBudget(v);
      message.success(`探针月预算已设为 $${r.monthly_limit_usd.toFixed(4)}`);
      refresh();
    } catch (e) {
      message.error(`预算更新失败: ${e}`);
    }
  };

  // 校准视图：每个 (provider,model) 取最近一天
  const latestCal = useMemo(() => {
    const map = new Map<string, CalibrationRow>();
    for (const r of calRows) {
      const prev = map.get(`${r.provider}/${r.model}`);
      if (!prev || r.as_of > prev.as_of) map.set(`${r.provider}/${r.model}`, r);
    }
    return Array.from(map.values());
  }, [calRows]);

  const staleCount = specs.filter((s) => s.price_stale).length;

  const columns = [
    {
      title: '通道',
      key: 'chan',
      render: (_: unknown, s: PriceSpec) => (
        <b>
          {s.provider}/{s.model}
        </b>
      ),
    },
    {
      title: '输入 $/1K',
      key: 'in',
      render: (_: unknown, s: PriceSpec) => usdPerTokToDollarsPer1k(s.input_cost),
    },
    {
      title: '输出 $/1K',
      key: 'out',
      render: (_: unknown, s: PriceSpec) => usdPerTokToDollarsPer1k(s.output_cost),
    },
    {
      title: '缓存读 $/1K',
      key: 'cread',
      render: (_: unknown, s: PriceSpec) =>
        s.cache_read_cost == null ? '-' : usdPerTokToDollarsPer1k(s.cache_read_cost),
    },
    {
      title: '来源',
      key: 'source',
      render: (_: unknown, s: PriceSpec) => {
        const m = SOURCE_META[s.price_source] ?? fallbackMeta;
        return (
          <Tag color={m.color}>
            {m.label}
            {s.price_stale
              ? (
                  <Tooltip title={staleTooltip(s)}>
                    <span style={{ color: '#cf1322' }}>●</span>
                  </Tooltip>
                )
              : null}
          </Tag>
        );
      },
    },
    {
      title: '生效日',
      dataIndex: 'effective_from',
      key: 'eff',
      render: (v: string | null) => v ?? '-',
    },
    {
      title: '操作',
      key: 'action',
      render: (_: unknown, s: PriceSpec) => (
        <Space size={4}>
          <Button size="small" onClick={() => openEdit(s)}>
            改价
          </Button>
          {s.price_source !== 'manual' && (
            <Popconfirm
              title="采纳为 manual？此后刷新不再覆盖，该价成为固定锚点。"
              onConfirm={() => handleAccept(s)}
            >
              <Button size="small" type="primary" ghost>
                采纳
              </Button>
            </Popconfirm>
          )}
        </Space>
      ),
    },
  ];

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <Space size={16}>
          <span style={{ color: '#999' }}>
            共 {specs.length} 条
            {staleCount > 0 ? <b style={{ color: '#cf1322' }}>（{staleCount} 条价格过期）</b> : null}
          </span>
          <span>
            仅看过期
            <Switch size="small" style={{ marginLeft: 6 }} checked={staleOnly} onChange={(v) => {
              setStaleOnly(v);
              setTimeout(refresh, 0);
            }} />
          </span>
        </Space>
        <Space>
          <Button icon={<ReloadOutlined />} loading={refreshing} onClick={handleRefreshRemote}>
            刷新远端价
          </Button>
          <Button onClick={refresh}>刷新视图</Button>
        </Space>
      </div>

      <Alert
        type="info"
        showIcon
        message="价格口径说明"
        description="价格按「倒排来源优先级」维护：manual > overlay > litellm_remote > litellm_packaged > heuristic。manual 为人工锚定，刷新 job 永不覆盖；标红色 ● 表示该价已过期（对账比值连续 3 天超出 [0.8, 1.2] 且当日调用 ≥50 才触发，日常看不到属正常），悬停红点可见原因，需人工核对后「采纳」或「改价」。"
      />

      <Table
        rowKey={(s) => `${s.provider}/${s.model}`}
        loading={loading}
        columns={columns}
        dataSource={specs}
        pagination={{ pageSize: 12 }}
        size="small"
      />

      <Card size="small" title="探针（响应性测试）与对账校准">
        {probe ? (
          <>
            <Space size={24} wrap>
              <Statistic title="本月消耗" value={probe.monthly_limit_cny ? probe.spend_usd / probe.monthly_limit_usd * probe.monthly_limit_cny : 0} prefix="¥" precision={2} suffix={`/ ¥${probe.monthly_limit_cny.toFixed(0)}`} />
              <Statistic title="探测轮数" value={probe.rounds} />
              <Statistic title="命中验证" value={probe.hit_verifications} />
              <Statistic title="命中失败" value={probe.hit_failures} valueStyle={{ color: probe.hit_failures ? '#cf1322' : undefined }} />
              <Statistic title="异常数" value={probe.failures} valueStyle={{ color: probe.failures ? '#cf1322' : undefined }} />
            </Space>
            <div style={{ marginTop: 16, display: 'flex', alignItems: 'center', gap: 8 }}>
              <span>月预算（¥，0=关闭探针）：</span>
              <InputNumber
                style={{ width: 120 }}
                min={0}
                step={1}
                defaultValue={probe.monthly_limit_cny}
                onPressEnter={(e) => onBudgetSave(Number((e.target as HTMLInputElement).value))}
                onBlur={(e) => onBudgetSave(Number(e.target.value))}
              />
            </div>
          </>
        ) : (
          <span style={{ color: '#999' }}>加载中…</span>
        )}
      </Card>

      <Card size="small" title={`近 30 天对账（耗时/估算偏差 · 命中率）`}>
        {latestCal.length === 0 ? (
          <span style={{ color: '#999' }}>暂无校准数据——用量样本 ≥50 后按日聚合（PRICING-PLAN §6.2）。</span>
        ) : (
          <Table
            rowKey={(r) => `${r.provider}/${r.model}/${r.as_of}`}
            dataSource={latestCal}
            pagination={{ pageSize: 8 }}
            size="small"
            columns={[
              { title: '通道', key: 'c', render: (_: unknown, r: CalibrationRow) => <b>{r.provider}/{r.model}</b> },
              { title: '日期', dataIndex: 'as_of', key: 'd' },
              { title: '调用', dataIndex: 'calls', key: 'calls' },
              {
                title: '对账偏差', key: 'ratio', render: (_: unknown, r: CalibrationRow) => {
                  const v = r.input_side_ratio;
                  const color = v > 1.2 || v < 0.8 ? '#cf1322' : '#389e0d';
                  return <span style={{ color, fontWeight: 600 }}>{v.toFixed(3)}</span>;
                },
              },
              {
                title: '缓存命中率', key: 'hit', render: (_: unknown, r: CalibrationRow) => `${(r.cache_hit_rate * 100).toFixed(0)}%`,
              },
            ]}
          />
        )}
      </Card>

      <Modal
        title={`改价 ${editing ? `${editing.provider}/${editing.model}` : ''}`}
        open={editOpen}
        onOk={handleSave}
        onCancel={() => setEditOpen(false)}
        destroyOnClose
        okText="保存"
      >
        <Form form={form} layout="vertical" style={{ marginTop: 16 }}>
          <Space size={12} style={{ display: 'flex' }}>
            <Form.Item
              name="input_price"
              label="输入价 ($/1K token)"
              rules={[{ required: true, message: '必填' }]}
            >
              <InputNumber min={1e-6} step={0.0001} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item
              name="output_price"
              label="输出价 ($/1K token)"
              rules={[{ required: true, message: '必填' }]}
            >
              <InputNumber min={1e-6} step={0.0001} style={{ width: '100%' }} />
            </Form.Item>
          </Space>
          <Form.Item name="cache_read_price" label="缓存读价 ($/1K token，留空表示无缓存区分)">
            <InputNumber min={0} step={0.0001} style={{ width: '100%' }} />
          </Form.Item>
          <span style={{ color: '#999', fontSize: 12 }}>保存后该规格将转为 manual 来源，刷新不再自动覆盖。</span>
        </Form>
      </Modal>
    </Space>
  );
}