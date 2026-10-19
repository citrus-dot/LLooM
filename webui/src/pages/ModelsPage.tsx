import { useEffect, useState } from 'react';
import { Alert, Table, Button, Space, Tag, Modal, Form, Input, InputNumber, Select, message, Popconfirm } from 'antd';
import { PlusOutlined, ReloadOutlined, EditOutlined } from '@ant-design/icons';
import { getModels, addModel, updateModel, removeModel, probeModel } from '../api';
import type { Model, ModelCreatePayload, ModelPatchPayload, ModelProbeResult } from '../api';

const CLOUD_PROVIDERS = [
  { value: 'dashscope', label: '阿里云百炼 (DashScope)' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'custom', label: '自定义' },
];

const LOCAL_COMPTS = [
  { value: 'ollama', label: 'Ollama 原生' },
  { value: 'openai', label: 'OpenAI 兼容 (LM Studio / vLLM)' },
];

const LOCAL_BASE_HINT: Record<string, string> = {
  ollama: '缺省 http://localhost:11434',
  openai: '缺省 http://localhost:1234/v1',
};

const TASK_TYPES = [
  { value: '', label: '不分配' },
  { value: 'simple_qa', label: '简单问答' },
  { value: 'general', label: '日常对话' },
  { value: 'coding', label: '代码生成' },
  { value: 'math_logic', label: '数学推理' },
  { value: 'complex_reasoning', label: '复杂分析' },
];

const HEALTH_STATES: Record<string, { label: string; color: string }> = {
  up: { label: '正常', color: 'green' },
  healthy: { label: '正常', color: 'green' },
  degraded: { label: '降级', color: 'orange' },
  down: { label: '不可用', color: 'red' },
  unknown: { label: '未检测', color: 'default' },
};

export default function ModelsPage() {
  const [models, setModels] = useState<Model[]>([]);
  const [loading, setLoading] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<Model | null>(null);
  const [probing, setProbing] = useState<string | null>(null);
  const [probeResults, setProbeResults] = useState<Record<string, ModelProbeResult>>({});
  const [form] = Form.useForm();
  const kind = Form.useWatch('kind', form) ?? 'cloud';
  const compat = Form.useWatch('compat', form);

  const refresh = async () => {
    setLoading(true);
    try {
      const { models } = await getModels(false);
      setModels(models);
    } catch (e) {
      message.error(`加载失败: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const handleAdd = async () => {
    const v = await form.validateFields();
    try {
      if (editing) {
        const patch: ModelPatchPayload = {
          kind: v.kind,
          task_type: v.task_type ?? '',
          input_cost_per_token: v.input_cost ?? 0,
          output_cost_per_token: v.output_cost ?? 0,
        };
        if (v.provider_model) patch.provider_model = v.provider_model;
        if (v.kind === 'local') {
          patch.compat = v.compat ?? 'ollama';
          patch.api_base = v.api_base ?? '';
        } else {
          patch.provider = v.provider;
          patch.api_base = v.api_base ?? '';
          patch.api_key = v.api_key ?? '';
        }
        await updateModel(editing.name, patch);
        message.success('模型已更新');
      } else {
        const existing = models.find((m) => m.name === v.name);
        if (existing?.is_active) {
          message.error('模型名称已存在');
          return;
        }
        const body: ModelCreatePayload = {
          name: v.name,
          kind: v.kind,
          task_type: v.task_type ?? '',
          input_cost_per_token: v.input_cost ?? 0,
          output_cost_per_token: v.output_cost ?? 0,
          rpm: v.rpm ?? 60,
        };
        if (v.provider_model) body.provider_model = v.provider_model;
        if (v.kind === 'local') {
          body.compat = v.compat ?? 'ollama';
          body.api_base = v.api_base || undefined;
        } else {
          body.provider = v.provider;
          body.api_base = v.api_base || undefined;
          body.api_key = v.api_key ?? '';
        }
        await addModel(body);
        message.success('模型添加成功');
      }
      setModalOpen(false);
      setEditing(null);
      form.resetFields();
      refresh();
    } catch (e) {
      message.error(`保存失败: ${e}`);
    }
  };

  const openAdd = () => {
    setEditing(null);
    form.resetFields();
    setModalOpen(true);
  };

  const openEdit = (m: Model) => {
    setEditing(m);
    form.resetFields();
    form.setFieldsValue({
      name: m.name,
      kind: m.kind,
      compat: m.compat ?? 'ollama',
      provider: m.provider ?? 'dashscope',
      provider_model: m.provider_model,
      api_base: m.api_base ?? '',
      api_key: m.api_key ?? '',
      input_cost: m.input_cost_per_token,
      output_cost: m.output_cost_per_token,
      task_type: m.task_type,
    });
    setModalOpen(true);
  };

  const handleRemove = async (name: string) => {
    try {
      await removeModel(name);
      setProbeResults((prev) => {
        const next = { ...prev };
        delete next[name];
        return next;
      });
      message.success('模型已停用');
      refresh();
    } catch (e) {
      message.error(`删除失败: ${e}`);
    }
  };

  const handleRestore = async (name: string) => {
    try {
      await updateModel(name, { is_active: 1 });
      setProbeResults((prev) => {
        const next = { ...prev };
        delete next[name];
        return next;
      });
      message.success('模型已恢复');
      refresh();
    } catch (e) {
      message.error(`恢复失败: ${e}`);
    }
  };

  const handleProbe = async (name: string) => {
    setProbing(name);
    try {
      const result = await probeModel(name);
      setProbeResults((prev) => ({ ...prev, [name]: result }));
      if (result.ok) message.success(`${name} 调用成功（${result.latency_ms} ms）`);
      else message.error(`${name} 测试失败：${result.detail}`);
      await refresh();
    } catch (e) {
      message.error(`测活失败: ${e}`);
    } finally {
      setProbing(null);
    }
  };

  const columns = [
    { title: '模型名称', dataIndex: 'name', key: 'name', render: (n: string) => <b>{n}</b> },
    {
      title: '类型',
      key: 'kind',
      render: (_: unknown, m: Model) =>
        m.kind === 'local' ? (
          <Tag color="green">本地 · {m.compat ?? 'ollama'}</Tag>
        ) : (
          <Tag color="blue">云端 · {m.provider}</Tag>
        ),
    },
    { title: '供应商模型', dataIndex: 'provider_model', key: 'provider_model' },
    {
      title: '状态',
      key: 'status',
      render: (_: unknown, m: Model) => (
        <Space>
          <Tag color={m.is_active ? 'green' : 'default'}>{m.is_active ? '已启用' : '已停用'}</Tag>
          <Tag color={HEALTH_STATES[m.health_state]?.color ?? 'default'}>
            健康：{HEALTH_STATES[m.health_state]?.label ?? m.health_state ?? '未检测'}
          </Tag>
          {m.health_checked_at && <span title="上次健康状态记录时间">记录于 {m.health_checked_at}</span>}
          {probeResults[m.name] && (
            <Tag color={probeResults[m.name].ok ? 'green' : 'red'}>
              本次检测：{probeResults[m.name].ok ? `${probeResults[m.name].latency_ms} ms` : '失败'}
            </Tag>
          )}
        </Space>
      ),
    },
    {
      title: '输入 ($/1K)',
      key: 'in',
      render: (_: unknown, m: Model) => (m.input_cost_per_token * 1000).toFixed(6),
    },
    {
      title: '输出 ($/1K)',
      key: 'out',
      render: (_: unknown, m: Model) => (m.output_cost_per_token * 1000).toFixed(6),
    },
    {
      title: '任务',
      dataIndex: 'task_type',
      key: 'task_type',
      render: (t: string) => (t ? <Tag>{t}</Tag> : '-'),
    },
    {
      title: '操作',
      key: 'action',
      render: (_: unknown, m: Model) => (
        <Space>
          <Button size="small" icon={<EditOutlined />} onClick={() => openEdit(m)}>
            编辑
          </Button>
          {m.is_active && (
            <Popconfirm title="发起最小模型请求？云端模型可能产生少量费用" onConfirm={() => handleProbe(m.name)}>
              <Button size="small" loading={probing === m.name} disabled={probing !== null && probing !== m.name}>
                立即测活
              </Button>
            </Popconfirm>
          )}
          {m.is_active ? (
            <Popconfirm title={`确认停用 ${m.name}？`} onConfirm={() => handleRemove(m.name)}>
              <Button size="small" danger>停用</Button>
            </Popconfirm>
          ) : (
            <Button size="small" onClick={() => handleRestore(m.name)}>恢复</Button>
          )}
        </Space>
      ),
    },
  ];

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between' }}>
        <span style={{ color: '#999' }}>共 {models.filter((m) => m.is_active).length} 个启用模型，{models.filter((m) => !m.is_active).length} 个已停用</span>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={refresh}>
            刷新
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={openAdd}>
            添加模型
          </Button>
        </Space>
      </div>

      <Alert type="info" showIcon message="云模型需要可靠定价才能参与自动路由"
        description="在「定价」页新增与模型名称完全一致的供应商/模型价格；未配置价格时仍可查看模型，但自动路由不会把未知成本当作免费。" />

      <Table rowKey="name" loading={loading} columns={columns} dataSource={models} pagination={false} />

      <Modal
        title={editing ? `编辑模型 ${editing.name}` : '添加模型'}
        open={modalOpen}
        onOk={handleAdd}
        onCancel={() => {
          setModalOpen(false);
          setEditing(null);
        }}
        destroyOnClose
      >
        <Form
          form={form}
          layout="vertical"
          style={{ marginTop: 16 }}
          onValuesChange={(changed) => {
            if ('kind' in changed) {
              // 切换类型时清掉另一侧的残留值，避免把本地 base 带进云端配置
              form.setFieldsValue({ api_base: '', api_key: '' });
            }
          }}
        >
          <Form.Item name="name" label="模型名称" rules={[{ required: true, message: '请输入名称' }]}>
            <Input placeholder="如 my-gpt-4o" disabled={!!editing} />
          </Form.Item>
          <Form.Item name="kind" label="模型类型" initialValue="cloud">
            <Select
              options={[
                { value: 'cloud', label: '云端模型（API Key 调用）' },
                { value: 'local', label: '本地模型（Ollama / OpenAI 兼容端点）' },
              ]}
            />
          </Form.Item>
          {kind === 'local' ? (
            <>
              <Form.Item name="compat" label="本地协议" initialValue="ollama">
                <Select options={LOCAL_COMPTS} />
              </Form.Item>
              <Form.Item name="api_base" label="API Base" extra={LOCAL_BASE_HINT[compat] ?? LOCAL_BASE_HINT.ollama}>
                <Input placeholder="留空使用缺省地址" />
              </Form.Item>
            </>
          ) : (
            <>
              <Form.Item name="provider" label="云端供应商" rules={[{ required: true, message: '请选择供应商' }]}>
                <Select options={CLOUD_PROVIDERS} />
              </Form.Item>
              <Form.Item name="api_base" label="API Base">
                <Input placeholder="可选，留空走供应商默认" />
              </Form.Item>
              <Form.Item
                name="api_key"
                label="API Key"
                extra="sk-... 字面密钥或环境变量名（如 DASHSCOPE_API_KEY）；编辑时原样提交掩码即保持原值，清空则移除"
              >
                <Input.Password placeholder="可选" autoComplete="new-password" />
              </Form.Item>
            </>
          )}
          <Form.Item name="provider_model" label="供应商模型 ID">
            <Input placeholder="留空自动生成，如 openai/my-model" />
          </Form.Item>
          <Space size={12} style={{ display: 'flex' }}>
            <Form.Item name="input_cost" label="输入价格 ($/token)" initialValue={0}>
              <InputNumber min={0} step={0.000001} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="output_cost" label="输出价格 ($/token)" initialValue={0}>
              <InputNumber min={0} step={0.000001} style={{ width: '100%' }} />
            </Form.Item>
          </Space>
          <Form.Item name="task_type" label="任务路由" initialValue="">
            <Select options={TASK_TYPES} />
          </Form.Item>
        </Form>
      </Modal>
    </Space>
  );
}
