<template>
  <div ref="editorRef" class="config-editor" />
</template>

<script setup lang="ts">
import { ref, shallowRef, watch, onMounted, onUnmounted } from 'vue'
import { EditorView, basicSetup } from 'codemirror'
import { json } from '@codemirror/lang-json'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorState, Compartment } from '@codemirror/state'
import { useThemeStore } from '@/stores'

const props = defineProps<{
  modelValue: string
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
}>()

const themeStore = useThemeStore()
const editorRef = ref<HTMLElement>()
const view = shallowRef<EditorView>()
const themeCompartment = new Compartment()

const detectLanguage = (content: string): 'json' | 'yaml' | 'text' => {
  const trimmed = content.trim()
  if (trimmed.startsWith('{') || trimmed.startsWith('[')) return 'json'
  if (/^([a-zA-Z_][a-zA-Z0-9_-]*:\s|---\s*$)/m.test(trimmed)) return 'yaml'
  return 'text'
}

const getLanguageExtension = (content: string) => {
  const lang = detectLanguage(content)
  return lang === 'json' ? [json()] : []
}

const getThemeExtension = () => (themeStore.isDark ? [oneDark] : [])

onMounted(() => {
  if (!editorRef.value) return

  const state = EditorState.create({
    doc: props.modelValue,
    extensions: [
      basicSetup,
      ...getLanguageExtension(props.modelValue),
      themeCompartment.of(getThemeExtension()),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          emit('update:modelValue', update.state.doc.toString())
        }
      }),
    ],
  })

  view.value = new EditorView({ state, parent: editorRef.value })
})

watch(
  () => props.modelValue,
  (newValue) => {
    if (view.value && newValue !== view.value.state.doc.toString()) {
      view.value.dispatch({
        changes: { from: 0, to: view.value.state.doc.length, insert: newValue },
      })
    }
  },
)

watch(
  () => themeStore.isDark,
  () => {
    view.value?.dispatch({
      effects: themeCompartment.reconfigure(getThemeExtension()),
    })
  },
)

onUnmounted(() => {
  view.value?.destroy()
})
</script>

<style scoped>
.config-editor {
  border: 1px solid var(--panel-border);
  border-radius: 8px;
  overflow: hidden;
}

.config-editor :deep(.cm-editor) {
  min-height: 400px;
  outline: none;
}

.config-editor :deep(.cm-focused) {
  outline: none;
}
</style>
