// Models route — model list with hover/click, OpenCode dialog-select style.

import { createSignal, onMount } from "solid-js"
import { theme } from "../theme"
import { getModels, addModel, updateModel, deleteModel } from "../api"
import type { Model, ModelCreatePayload, ModelPatchPayload } from "../api"
import { dialogOpen } from "../app"
import { useBindings } from "@opentui/keymap/solid"
import { useDialog } from "../ui/dialog"
import { Button, Table, PageHeader } from "../ui"

const TASK_TYPES = ["", "simple_qa", "general", "coding", "math_logic", "complex_reasoning"]

const KIND_FIELDS = [
  { key: "kind", label: "类型", placeholder: "local / cloud（缺省按下方字段推断）" },
  { key: "compat", label: "本地协议", placeholder: "kind=local：ollama（默认）/ openai (LM Studio/vLLM)" },
  { key: "provider", label: "云端供应商", placeholder: "kind=cloud：dashscope/openai/anthropic/custom" },
  { key: "api_base", label: "API Base", placeholder: "本地缺省 11434 / 1234/v1；云端可选" },
  { key: "api_key", label: "API Key", placeholder: "仅云端：sk-... 或环境变量名；本地必须留空" },
]

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
        key: "ctrl+d",
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
        ...KIND_FIELDS,
        { key: "litellm_model", label: "LiteLLM 模型", placeholder: "留空自动拼前缀，如 ollama/qwen2.5" },
        { key: "input_cost", label: "输入成本 ($/tok)", placeholder: "如 0.000001" },
        { key: "output_cost", label: "输出成本 ($/tok)", placeholder: "如 0.000002" },
        { key: "task_type", label: "任务路由", placeholder: TASK_TYPES.filter(Boolean).join("/") },
      ],
      onConfirm: async (vals) => {
        const kind = (vals.kind.trim() ||
          (vals.compat.trim() || vals.provider.trim() === "ollama" ? "local" : "cloud")) as "local" | "cloud"
        const body: ModelCreatePayload = {
          name: vals.name.trim(),
          kind,
          task_type: vals.task_type.trim() || "general",
          input_cost_per_token: parseFloat(vals.input_cost) || 0,
          output_cost_per_token: parseFloat(vals.output_cost) || 0,
          rpm: 60,
        }
        if (vals.compat.trim()) body.compat = vals.compat.trim()
        if (vals.provider.trim()) body.provider = vals.provider.trim()
        if (vals.api_base.trim()) body.api_base = vals.api_base.trim()
        if (vals.api_key.trim()) body.api_key = vals.api_key.trim()
        if (vals.litellm_model.trim()) body.litellm_model = vals.litellm_model.trim()
        try {
          await addModel(body)
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
        { key: "kind", label: "类型", default: m.kind ?? "cloud", placeholder: "local / cloud" },
        { key: "compat", label: "本地协议", default: m.compat ?? "", placeholder: "ollama / openai（kind=local）" },
        { key: "provider", label: "云端供应商", default: m.provider ?? "", placeholder: "dashscope/openai/anthropic/custom（kind=cloud）" },
        { key: "api_base", label: "API Base", placeholder: m.api_base ?? "", default: m.api_base ?? "" },
        { key: "api_key", label: "API Key", placeholder: "仅云端；原样提交掩码=保持原值，清空=移除", default: m.api_key ?? "" },
        { key: "litellm_model", label: "LiteLLM 模型", placeholder: m.litellm_model, default: m.litellm_model },
        { key: "input_cost", label: "输入成本 ($/tok)", default: String(m.input_cost_per_token ?? 0) },
        { key: "output_cost", label: "输出成本 ($/tok)", default: String(m.output_cost_per_token ?? 0) },
        { key: "task_type", label: "任务路由", default: m.task_type },
      ],
      onConfirm: async (vals) => {
        const kind = (vals.kind.trim() || "cloud") as "local" | "cloud"
        const patch: ModelPatchPayload = {
          kind,
          input_cost_per_token: parseFloat(vals.input_cost) || 0,
          output_cost_per_token: parseFloat(vals.output_cost) || 0,
          task_type: vals.task_type.trim(),
        }
        if (kind === "local") {
          patch.compat = vals.compat.trim() || "ollama"
          patch.api_base = vals.api_base.trim()
        } else {
          patch.provider = vals.provider.trim() || "custom"
          patch.api_base = vals.api_base.trim()
          patch.api_key = vals.api_key.trim()
        }
        if (vals.litellm_model.trim()) patch.litellm_model = vals.litellm_model.trim()
        try {
          await updateModel(m.name, patch)
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
          { title: "类型", width: "20%", render: (m, { selected }) => <text fg={selected ? theme.background : theme.textMuted}>{m.kind === "local" ? `本地:${m.compat ?? "ollama"}` : `云端:${m.provider ?? "?"}`}</text> },
          { title: "LiteLLM 模型", render: (m, { selected }) => <text fg={selected ? theme.background : theme.text}>{m.litellm_model}</text> },
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
