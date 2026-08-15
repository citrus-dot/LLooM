// Models route — model list with hover/click, OpenCode dialog-select style.

import { createSignal, onMount } from "solid-js"
import { theme } from "../theme"
import { getModels, addModel, deleteModel, type Model } from "../api"
import { dialogOpen } from "../app"
import { useBindings } from "@opentui/keymap/solid"
import { useDialog } from "../ui/dialog"

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
            input_cost_per_token: 0,
            output_cost_per_token: 0,
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

  return (
    <box flexDirection="column" flexGrow={1} minHeight={0} paddingLeft={2} paddingRight={2} paddingTop={1}>
      <box flexDirection="row" gap={1} paddingBottom={1}>
        <text fg={theme.textMuted} attributes={1}>模型管理</text>
        <text fg={theme.textMuted}>·</text>
        <text fg={theme.textMuted}>{models().length} 个</text>
        <text fg={theme.textMuted} onMouseUp={() => refresh()}>[刷新]</text>
        <text fg={theme.primary} onMouseUp={() => add()}>[添加]</text>
      </box>

      <box flexDirection="column" backgroundColor={theme.backgroundPanel} border={["left", "right"]} borderColor={theme.border} paddingTop={1} paddingBottom={1}>
        <box flexDirection="row" paddingLeft={3} paddingRight={3} paddingBottom={1}>
          <text fg={theme.textMuted} attributes={1} width="30%">名称</text>
          <text fg={theme.textMuted} attributes={1} width="15%">提供商</text>
          <text fg={theme.textMuted} attributes={1} width="40%">LiteLLM 模型</text>
          <text fg={theme.textMuted} attributes={1}>操作</text>
        </box>
        {models().length === 0 && <text fg={theme.textDim} paddingLeft={3}>  暂无模型</text>}
        {models().map((m, i) => {
          const isSel = i === selIdx()
          const isHover = i === hoverIdx()
          return (
            <box
              flexDirection="row"
              backgroundColor={isSel ? theme.primary : isHover ? theme.backgroundElement : theme.backgroundPanel}
              paddingLeft={3}
              paddingRight={3}
              onMouseOver={() => setHoverIdx(i)}
              onMouseOut={() => setHoverIdx(null)}
              onMouseDown={() => setSelIdx(i)}
            >
              <text fg={isSel ? theme.background : theme.text} width="30%" attributes={isSel ? 1 : 0}>{m.name}</text>
              <text fg={isSel ? theme.background : theme.textMuted} width="15%">{m.provider}</text>
              <text fg={isSel ? theme.background : theme.text} width="40%">{m.litellm_model}</text>
              <text fg={isSel ? theme.background : theme.error} onMouseUp={() => del(m.name)}>[删除]</text>
            </box>
          )
        })}
      </box>

      <box paddingTop={1}>
        <text fg={theme.textDim}>  鼠标点击选中 · 点击 [删除] 移除模型 · 用 CLI 添加: lloom-cli models add</text>
      </box>
    </box>
  )
}
