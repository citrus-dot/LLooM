import { useEffect, useState } from 'react';
import { Alert, Button, Card, DatePicker, Form, Input, InputNumber, Modal, Popconfirm, Select, Space, Statistic, Table, Tag, Typography, message } from 'antd';
import { CopyOutlined, KeyOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons';
import { ApiKeyItem, createApiKey, deleteApiKey, getModels, listApiKeys, updateApiKey } from '../api';

const { Text, Paragraph } = Typography;

export default function ApiKeysPage() {
  const [keys,setKeys]=useState<ApiKeyItem[]>([]); const [models,setModels]=useState<string[]>([]);
  const [open,setOpen]=useState(false); const [editing,setEditing]=useState<ApiKeyItem|null>(null); const [created,setCreated]=useState<string|null>(null);
  const [form]=Form.useForm();
  const refresh=async()=>{ try{ const [k,m]=await Promise.all([listApiKeys(),getModels()]); setKeys(k.keys); setModels(m.models.map(x=>x.name)); }catch(e){message.error(String(e));} };
  useEffect(()=>{void refresh();},[]);
  const edit=(item?:ApiKeyItem)=>{ setEditing(item??null); form.setFieldsValue(item?{...item,expires_at:undefined}:{rpm:60,status:'active',allowed_models:[]}); setOpen(true); };
  const save=async()=>{ const v=await form.validateFields(); const body={name:v.name,status:v.status,quota_usd:v.quota_usd??null,rpm:v.rpm,allowed_models:v.allowed_models??[],expires_at:v.expires_at?.format?.('YYYY-MM-DD')??null}; try{ if(editing) await updateApiKey(editing.id,body); else setCreated((await createApiKey(body)).key); setOpen(false); await refresh(); message.success(editing?'已更新':'已创建'); }catch(e){message.error(String(e));} };
  const active=keys.filter(k=>k.status==='active').length; const used=keys.reduce((a,k)=>a+k.used_usd,0);
  const columns=[
    {title:'名称',dataIndex:'name',render:(v:string,k:ApiKeyItem)=><Space><KeyOutlined/><b>{v}</b>{k.status==='active'?<Tag color="success">启用</Tag>:<Tag>禁用</Tag>}</Space>},
    {title:'Key',dataIndex:'key_prefix',render:(v:string)=><Text code>{v}</Text>},
    {title:'额度',render:(_:unknown,k:ApiKeyItem)=>k.quota_usd==null?`$${k.used_usd.toFixed(4)} / 不限`:`$${k.used_usd.toFixed(4)} / $${k.quota_usd.toFixed(2)}`},
    {title:'RPM',dataIndex:'rpm'},
    {title:'模型范围',render:(_:unknown,k:ApiKeyItem)=>k.allowed_models.length?k.allowed_models.map(m=><Tag key={m}>{m}</Tag>):<Tag color="blue">全部模型</Tag>},
    {title:'最后使用',dataIndex:'last_used_at',render:(v:string|null)=>v||'从未'},
    {title:'操作',render:(_:unknown,k:ApiKeyItem)=><Space><Button size="small" onClick={()=>edit(k)}>编辑</Button><Popconfirm title="删除此 Key？" onConfirm={async()=>{await deleteApiKey(k.id);await refresh();}}><Button danger size="small">删除</Button></Popconfirm></Space>},
  ];
  return <Space direction="vertical" size={16} style={{width:'100%'}}>
    <div className="page-hero"><div><h2>API Key 管理</h2><p>为外部客户端签发独立凭证，并控制额度、速率与可用模型。</p></div><Space><Button icon={<ReloadOutlined/>} onClick={refresh}>刷新</Button><Button type="primary" icon={<PlusOutlined/>} onClick={()=>edit()}>创建 API Key</Button></Space></div>
    <div className="metric-grid"><Card><Statistic title="全部 Key" value={keys.length}/></Card><Card><Statistic title="启用中" value={active}/></Card><Card><Statistic title="累计消费" value={used} precision={4} prefix="$"/></Card></div>
    <Alert type="info" showIcon message="Key 明文只在创建后显示一次" description="服务端仅保存 SHA-256 摘要。丢失后请删除并重新创建。"/>
    <Card className="clean-card"><Table rowKey="id" columns={columns} dataSource={keys} pagination={false}/></Card>
    <Modal title={editing?'编辑 API Key':'创建 API Key'} open={open} onOk={save} onCancel={()=>setOpen(false)} destroyOnClose><Form form={form} layout="vertical">
      <Form.Item name="name" label="名称" rules={[{required:true}]}><Input placeholder="例如：Open WebUI / 开发环境"/></Form.Item>
      {editing&&<Form.Item name="status" label="状态"><Select options={[{value:'active',label:'启用'},{value:'disabled',label:'禁用'}]}/></Form.Item>}
      <Space style={{display:'flex'}}><Form.Item name="quota_usd" label="额度上限（USD）"><InputNumber min={0} placeholder="留空不限"/></Form.Item><Form.Item name="rpm" label="RPM"><InputNumber min={1} max={10000}/></Form.Item></Space>
      <Form.Item name="allowed_models" label="允许模型"><Select mode="multiple" allowClear options={models.map(m=>({value:m,label:m}))} placeholder="留空允许全部"/></Form.Item>
      <Form.Item name="expires_at" label="有效期"><DatePicker style={{width:'100%'}}/></Form.Item>
    </Form></Modal>
    <Modal title="API Key 已创建" open={created!==null} onCancel={()=>setCreated(null)} footer={<Button type="primary" onClick={()=>setCreated(null)}>我已保存</Button>}><Alert type="warning" showIcon message="请立即复制，关闭后无法再次查看"/><Paragraph copyable={{text:created??''}} style={{marginTop:16}}><Text code>{created}</Text></Paragraph><Button block icon={<CopyOutlined/>} onClick={()=>navigator.clipboard.writeText(created??'')}>复制 Key</Button></Modal>
  </Space>;
}
