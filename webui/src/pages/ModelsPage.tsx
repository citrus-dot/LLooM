import { useEffect, useState } from 'react';
import { Table, Button, Space, Tag, Modal, Form, Input, InputNumber, Select, message, Popconfirm } from 'antd';
import { PlusOutlined, ReloadOutlined } from '@ant-design/icons';
import { getModels, addModel, removeModel, Model } from '../api';

const PROVIDERS = [
  { value: 'dashscope', label: '阿里云百炼 (DashScope)', prefix: 'openai/' },
  { value: 'openai', label: 'OpenAI', prefix: '' },
  { value: 'anthropic', label: 'Anthropic', prefix: 'anthropic/' },
  { value: 'ollama', label: 'Ollama (本地)', prefix: 'ollama/' },
  { value: 'custom', label: '自定义', prefix: '' },
];

const TASK_TYPES = [
  { value: '', label: '不分配' },
  { value: 'simple_qa', label: '简单问答' },
  { value: 'general', label: '日常对话' },
  { value: 'coding', label: '代码生成' },
  { value: 'math_logic', label: '数学推理' },
  { value: 'complex_reasoning', label: '复杂分析' },
];

export default function ModelsPage() {
  const [models, setModels] = useState<Model[]>([]);
  const [loading, setLoading] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [form] = Form.useForm();

  const refresh = async () => {
    setLoading(true);
    try {
      const { models } = await getModels();
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
      await addModel({
        name: v.name,
        provider: v.provider,
        litellm_model: v.litellm_model || `${PROVIDERS.find((p) => p.value === v.provider)?.prefix ?? ''}${v.name}`,
        api_base: v.api_base ?? '',
        api_key_env: v.api_key_env ?? '',
        task_type: v.task_type ?? '',
        input_cost_per_token: v.input_cost ?? 0,
        output_cost_per_token: v.output_cost ?? 0,
        rpm: v.rpm ?? 60,
      });
      message.success('模型添加成功');
      setModalOpen(false);
      form.resetFields();
      refresh();
    } catch (e) {
      message.error(`添加失败: ${e}`);
    }
  };

  const handleRemove = async (name: string) => {
    try {
      await removeModel(name);
      message.success('模型已删除');
      refresh();
    } catch (e) {
      message.error(`删除失败: ${e}`);
    }
  };

  const columns = [
    { title: '模型名称', dataIndex: 'name', key: 'name', render: (n: string) => <b>{n}</b> },
    { title: '供应商', dataIndex: 'provider', key: 'provider', render: (v: string) => <Tag color="blue">{v}</Tag> },
    { title: 'LiteLLM 模型', dataIndex: 'litellm_model', key: 'litellm_model' },
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
        <Popconfirm title={`确认删除 ${m.name}？`} onConfirm={() => handleRemove(m.name)}>
          <Button size="small" danger>
            删除
          </Button>
        </Popconfirm>
      ),
    },
  ];

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between' }}>
        <span style={{ color: '#999' }}>共 {models.length} 个模型</span>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={refresh}>
            刷新
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setModalOpen(true)}>
            添加模型
          </Button>
        </Space>
      </div>

      <Table rowKey="name" loading={loading} columns={columns} dataSource={models} pagination={false} />

      <Modal title="添加模型" open={modalOpen} onOk={handleAdd} onCancel={() => setModalOpen(false)} destroyOnClose>
        <Form form={form} layout="vertical" style={{ marginTop: 16 }}>
          <Form.Item name="name" label="模型名称" rules={[{ required: true, message: '请输入名称' }]}>
            <Input placeholder="如 my-gpt-4o" />
          </Form.Item>
          <Form.Item name="provider" label="供应商" initialValue="dashscope">
            <Select options={PROVIDERS} />
          </Form.Item>
          <Form.Item name="litellm_model" label="LiteLLM 模型字符串">
            <Input placeholder="留空自动生成，如 openai/my-model" />
          </Form.Item>
          <Form.Item name="api_base" label="API Base">
            <Input placeholder="可选" />
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
