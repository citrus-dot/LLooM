// Models route — model list with hover/click, OpenCode dialog-select style.

import { createSignal, onMount } from "solid-js"
import { theme } from "../theme"
import { getModels, addModel, updateModel, deleteModel, type Model } from "../api"
import { dialogOpen } from "../app"
import { useBindings } from "@opentui/keymap/solid"
import { useDialog } from "../ui/dialog"
import { Button, Table, PageHeader } from "../ui"

const PROVIDERS: { value: string; label: string; prefix: string }[] = [
  { value: "dashscope", label: "DashScope", prefix: "openai/" },
  { value: "openai", label: "OpenAI", prefix: "" },
  { value: "anthropic", label: "Anthropic", prefix: "anthropic/" },
  { value: "ollama", label: "Ollama", prefix: "ollama/" },
  { value: "custom", label: "自定义", prefix: "" },
]

const TASK_TYPES = ["", "simple_qa", "general", "coding", "math_logic", "complex_reasoning"]

export function Models(props: { setStatus: (s: string) => void }) {
  const [models, setModels] = createSignal<Model[]>([])
  const [selIdx, setSelIdx] = createSignal(0)
  const [hoverIdx, setHoverIdx] = createSignal<number | null>(null)
  const dialog = useDialog()

  const refresh = async () => {
    try {
      setModels((await getModels()).models)
    } catch (e) {
      props.setStatus(`无法连接: ${e}`)
    }
  }

  onMount(() => {
    refresh()
  })

  useBindings(() => ({
    enabled: () => !dialogOpen(),
    bindings: [
      {
        key: "up",
        cmd: () => {
          const n = models().length
          if (n === 0) return
          setSelIdx((selIdx() - 1 + n) % n)
        },
        desc: "Previous model",
      },
      {
        key: "down",
        cmd: () => {
          const n = models().length
          if (n === 0) return
          setSelIdx((selIdx() + 1) % n)
        },
        desc: "Next model",
      },
      {
        key: "d",
        cmd: () => {
          if (models()[selIdx()]) del(models()[selIdx()].name)
        },
        desc: "Delete model",
      },
    ],
  }))

  const del = async (name: string) => {
    dialog.menu(`删除模型 ${name}?`, {
      items: [
        { title: "确认删除", danger: true, onSelect: () => void doDel(name) },
        { title: "取消", onSelect: () => {} },
      ],
    })
  }

  const doDel = async (name: string) => {
    try {
      await deleteModel(name)
      await refresh()
      props.setStatus(`已删除 ${name}`)
    } catch (e) {
      props.setStatus(`删除失败: ${e}`)
    }
  }

  const add = () => {
    dialog.form("添加模型", {
      fields: [
        { key: "name", label: "名称", placeholder: "如 qwen2.5-local", required: true },
        { key: "provider", label: "提供商", placeholder: "dashscope/openai/anthropic/ollama/custom" },
        { key: "litellm_model", label: "LiteLLM 模型", placeholder: "留空自动拼前缀，如 ollama/qwen2.5" },
        { key: "api_base", label: "API Base", placeholder: "如 http://localhost:11434" },
        { key: "input_cost", label: "输入成本 ($/tok)", placeholder: "如 0.000001" },
        { key: "output_cost", label: "输出成本 ($/tok)", placeholder: "如 0.000002" },
        { key: "task_type", label: "任务路由", placeholder: "simple_qa/general/coding/math_logic/complex_reasoning" },
      ],
      onConfirm: async (vals) => {
        const provider = vals.provider.trim() || "custom"
        const prefix = PROVIDERS.find((p) => p.value === provider)?.prefix ?? ""
        try {
          await addModel({
            name: vals.name.trim(),
            provider,
            litellm_model: vals.litellm_model.trim() || `${prefix}${vals.name.trim()}`,
            api_base: vals.api_base.trim(),
            api_key_env: "",
            task_type: vals.task_type.trim(),
            input_cost_per_token: parseFloat(vals.input_cost) || 0,
            output_cost_per_token: parseFloat(vals.output_cost) || 0,
            rpm: 60,
            is_active: 1,
          })
          props.setStatus(`✓ 已添加 ${vals.name.trim()}`)
          await refresh()
        } catch (e) {
          props.setStatus(`添加失败: ${e}`)
        }
      },
    })
  }

  const edit = (m: Model) => {
    dialog.form(`编辑模型 ${m.name}`, {
      fields: [
        { key: "litellm_model", label: "LiteLLM 模型", placeholder: m.litellm_model, default: m.litellm_model },
        { key: "api_base", label: "API Base", placeholder: m.api_base ?? "", default: m.api_base ?? "" },
        { key: "input_cost", label: "输入成本 ($/tok)", default: String(m.input_cost_per_token ?? 0) },
        { key: "output_cost", label: "输出成本 ($/tok)", default: String(m.output_cost_per_token ?? 0) },
        { key: "task_type", label: "任务路由", default: m.task_type },
      ],
      onConfirm: async (vals) => {
        try {
          await updateModel(m.name, {
            litellm_model: vals.litellm_model.trim() || m.litellm_model,
            api_base: vals.api_base.trim(),
            input_cost_per_token: parseFloat(vals.input_cost) || 0,
            output_cost_per_token: parseFloat(vals.output_cost) || 0,
            task_type: vals.task_type.trim(),
          })
          props.setStatus(`✓ 已更新 ${m.name}`)
          await refresh()
        } catch (e) {
          props.setStatus(`更新失败: ${e}`)
        }
      },
    })
  }

  const modelMenu = (m: Model) => {
    dialog.menu(m.name, {
      items: [
        { title: "编辑", desc: "修改配置/成本", onSelect: () => edit(m) },
        { title: "删除", desc: "移除该模型", danger: true, onSelect: () => del(m.name) },
      ],
    })
  }

  return (
    <box flexDirection="column" flexGrow={1} minHeight={0} paddingLeft={2} paddingRight={2} paddingTop={1}>
      <PageHeader title="模型管理">
        <text fg={theme.textMuted}>·</text>
        <text fg={theme.textMuted}>{models().length} 个</text>
        <Button variant="ghost" onClick={() => refresh()}>刷新</Button>
        <Button variant="primary" onClick={() => add()}>添加模型</Button>
      </PageHeader>

      <Table
        columns={[
          { title: "名称", width: "30%", render: (m, { selected }) => <text fg={selected ? theme.background : theme.text} attributes={selected ? 1 : 0}>{m.name}</text> },
          { title: "提供商", width: "15%", render: (m, { selected }) => <text fg={selected ? theme.background : theme.textMuted}>{m.provider}</text> },
          { title: "LiteLLM 模型", width: "40%", render: (m, { selected }) => <text fg={selected ? theme.background : theme.text}>{m.litellm_model}</text> },
          {
            title: "操作",
            render: (m, { selected }) => (
              <Button inverse={selected} variant="danger" onClick={() => del(m.name)}>删除</Button>
            ),
          },
        ]}
        rows={models()}
        selectedIndex={selIdx()}
        hoverIndex={hoverIdx()}
        onHover={setHoverIdx}
        onSelect={setSelIdx}
        onRowUp={(m, evt) => { if (evt?.button === 2) modelMenu(m) }}
        emptyText="暂无模型"
      />

      <box paddingTop={1}>
        <text fg={theme.textDim}>  点击选中 · 右键行弹出编辑/删除菜单 · [添加] 注册模型</text>
      </box>
    </box>
  )
}
