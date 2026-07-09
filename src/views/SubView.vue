<template>
  <n-flex vertical size="large" style="height: 100%" class="page-shell">
    <PageHeader :title="t('sub.title')" :subtitle="t('sub.subtitle')">
      <template #actions>
        <n-button type="primary" @click="showAddModal = true" round>
          <template #icon>
            <n-icon><AddOutline /></n-icon>
          </template>
          {{ t('sub.add') }}
        </n-button>
      </template>
    </PageHeader>

    <!-- Subscription List -->
    <div class="subscription-section">
      <n-grid
        v-if="subStore.list.length > 0"
        responsive="screen"
        cols="1 s:2 m:2 l:3"
        :x-gap="16"
        :y-gap="16"
      >
        <n-grid-item v-for="(item, index) in subStore.list" :key="index">
          <div :class="['sub-card', { active: subStore.activeIndex === index }]">
            <!-- Card Header -->
            <div class="sub-card-header">
              <div :class="['sub-avatar', { active: subStore.activeIndex === index }]">
                <n-icon size="18"><LinkOutline /></n-icon>
              </div>
              <div class="sub-header-info">
                <n-ellipsis class="sub-name" :line-clamp="1" :title="item.name">{{
                  item.name
                }}</n-ellipsis>
                <div class="sub-badges">
                  <span class="badge badge-type">
                    {{ item.isManual ? t('sub.manual') : t('sub.urlSubscription') }}
                  </span>
                  <span v-if="subStore.activeIndex === index" class="badge badge-active">
                    <span class="badge-dot" />{{ t('sub.inUse') }}
                  </span>
                  <span
                    v-if="
                      !item.isManual &&
                      (item.autoUpdateIntervalMinutes ?? DEFAULT_AUTO_UPDATE_MINUTES) > 0
                    "
                    class="badge badge-timer"
                  >
                    <n-icon size="11"><TimerOutline /></n-icon>
                    {{ formatIntervalLabel(item.autoUpdateIntervalMinutes) }}
                  </span>
                </div>
              </div>
              <n-dropdown
                trigger="hover"
                placement="bottom-end"
                :options="getDropdownOptions(index)"
              >
                <n-button text class="more-btn">
                  <n-icon size="18"><EllipsisVerticalOutline /></n-icon>
                </n-button>
              </n-dropdown>
            </div>

            <!-- Traffic Bar (if available) -->
            <div v-if="hasSubscriptionTraffic(item)" class="traffic-section">
              <div class="traffic-bar-row">
                <span class="traffic-label">{{ t('sub.traffic') }}</span>
                <span class="traffic-value">{{ formatTrafficSummary(item) }}</span>
              </div>
              <div class="traffic-bar-bg">
                <div
                  class="traffic-bar-fill"
                  :style="{ width: getTrafficPercent(item) + '%' }"
                  :class="getTrafficClass(item)"
                />
              </div>
            </div>

            <!-- Meta Info -->
            <div class="sub-meta">
              <div class="meta-row" :title="item.url || t('sub.manualContent')">
                <n-icon size="13" class="meta-icon"><GlobeOutline /></n-icon>
                <span class="meta-text url-text">{{ item.url || t('sub.manualContent') }}</span>
              </div>
              <div class="meta-row">
                <n-icon size="13" class="meta-icon"><TimeOutline /></n-icon>
                <span class="meta-text">
                  {{ item.lastUpdate ? formatTime(item.lastUpdate) : t('sub.neverUsed') }}
                </span>
              </div>
              <div v-if="item.subscriptionExpire" class="meta-row">
                <n-icon size="13" class="meta-icon"><CalendarOutline /></n-icon>
                <span class="meta-text">{{ formatExpireTime(item.subscriptionExpire) }}</span>
              </div>
              <div
                v-if="item.autoUpdateFailCount && item.autoUpdateFailCount > 0"
                class="meta-row warn"
              >
                <n-icon size="13" class="meta-icon"><AlertCircleOutline /></n-icon>
                <span class="meta-text">{{ formatAutoUpdateHealth(item) }}</span>
              </div>
            </div>

            <!-- Action -->
            <n-button
              block
              :type="subStore.activeIndex === index ? 'success' : 'primary'"
              secondary
              :loading="item.isLoading"
              class="sub-action-btn"
              @click="useSubscription(index)"
            >
              <template #icon>
                <n-icon>
                  <CheckmarkCircleOutline v-if="subStore.activeIndex === index" />
                  <PlayCircleOutline v-else />
                </n-icon>
              </template>
              {{ subStore.activeIndex === index ? t('sub.useAgain') : t('sub.use') }}
            </n-button>
          </div>
        </n-grid-item>
      </n-grid>

      <!-- Empty State -->
      <n-empty v-else size="huge" :description="t('sub.noSubscriptionsYet')">
        <template #icon>
          <n-icon><LinkOutline /></n-icon>
        </template>
        <template #extra>
          <n-button type="primary" @click="showAddModal = true">
            {{ t('sub.addFirstSubscription') }}
          </n-button>
        </template>
      </n-empty>
    </div>

    <!-- Add/Edit Modal -->
    <n-modal
      v-model:show="showAddModal"
      preset="dialog"
      :title="editIndex === null ? t('sub.add') : t('sub.edit')"
      class="modern-modal"
      :style="{ width: '500px' }"
      :mask-closable="false"
    >
      <n-form
        ref="formRef"
        :model="formValue"
        :rules="rules"
        label-placement="top"
        class="sub-form"
      >
        <n-form-item :label="t('sub.name')" path="name">
          <n-input v-model:value="formValue.name" :placeholder="t('sub.namePlaceholder')" />
        </n-form-item>

        <n-tabs type="segment" animated v-model:value="activeTab">
          <n-tab-pane name="url" :tab="t('sub.urlSubscription')">
            <n-form-item label="URL" path="url">
              <n-input
                v-model:value="formValue.url"
                type="textarea"
                :rows="3"
                :placeholder="t('sub.urlPlaceholder')"
              />
              <template #feedback>
                <n-text depth="3">{{ t('sub.urlHint') }}</n-text>
              </template>
            </n-form-item>
          </n-tab-pane>
          <n-tab-pane name="manual" :tab="t('sub.manualConfig')">
            <n-form-item :label="t('sub.content')" path="manualContent">
              <n-input
                v-model:value="formValue.manualContent"
                type="textarea"
                :rows="6"
                :placeholder="t('sub.manualContentPlaceholder')"
                class="code-input"
              />
              <template #feedback>
                <n-text depth="3">{{ t('sub.manualHint') }}</n-text>
              </template>
            </n-form-item>
          </n-tab-pane>
          <n-tab-pane name="uri" :tab="t('sub.uriList')">
            <n-form-item :label="t('sub.uriContent')" path="uriContent">
              <n-input
                v-model:value="formValue.uriContent"
                type="textarea"
                :rows="6"
                :placeholder="t('sub.uriContentPlaceholder')"
                class="code-input"
              />
              <template #feedback>
                <n-text depth="3">{{ t('sub.uriHint') }}</n-text>
              </template>
            </n-form-item>
          </n-tab-pane>
        </n-tabs>

        <n-form-item
          v-if="activeTab !== 'uri'"
          :label="t('sub.useOriginalConfig')"
          path="useOriginalConfig"
        >
          <n-flex align="center" justify="space-between" style="width: 100%">
            <n-text depth="3">{{
              formValue.useOriginalConfig ? t('sub.useOriginal') : t('sub.useExtractedNodes')
            }}</n-text>
            <n-switch
              v-model:value="formValue.useOriginalConfig"
              @update:value="markUseOriginalTouched"
            />
          </n-flex>
          <template #feedback>
            <n-alert
              v-if="formValue.useOriginalConfig"
              type="warning"
              show-icon
              style="margin-top: 8px"
            >
              {{ t('sub.originalConfigWarning') }}
            </n-alert>
          </template>
        </n-form-item>

        <n-form-item :label="t('sub.autoUpdate')" path="autoUpdateIntervalMinutes">
          <n-select
            v-model:value="formValue.autoUpdateIntervalMinutes"
            :options="autoUpdateOptions"
            size="small"
            :disabled="autoUpdateDisabled"
          />
          <template #feedback v-if="autoUpdateDisabled">
            <n-text depth="3">{{ t('sub.autoUpdateManualHint') }}</n-text>
          </template>
        </n-form-item>
      </n-form>

      <template #action>
        <n-space justify="end">
          <n-button @click="handleCancel">{{ t('common.cancel') }}</n-button>
          <n-button type="primary" @click="handleConfirm" :loading="isLoading">
            {{ t('common.confirm') }}
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- Config Editor Modal -->
    <n-modal
      v-model:show="showConfigModal"
      preset="dialog"
      :title="t('sub.editCurrentConfig')"
      class="modern-modal config-editor-modal"
      :style="{ width: '800px' }"
      :mask-closable="false"
    >
      <ConfigEditor v-model="currentConfig" />
      <div v-if="configFormatError" class="config-format-error">
        <n-icon :size="16" color="#d03050"><AlertCircleOutline /></n-icon>
        <span>{{ configFormatError }}</span>
      </div>
      <template #action>
        <n-space justify="end">
          <n-button @click="showConfigModal = false">{{ t('common.cancel') }}</n-button>
          <n-button type="primary" @click="saveCurrentConfig" :loading="isConfigLoading">
            {{ t('sub.saveAndApply') }}
          </n-button>
        </n-space>
      </template>
    </n-modal>
  </n-flex>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, h, watch } from 'vue'
import { useMessage } from 'naive-ui'
import { useSubStore } from '@/stores/subscription/SubStore'
import { useAppStore } from '@/stores'
import { subscriptionService } from '@/services/subscription-service'
import type { SubscriptionPersistResult } from '@/services/subscription-service'
import { kernelService } from '@/services/kernel-service'
import { useI18n } from 'vue-i18n'
import { DEFAULT_AUTO_UPDATE_MINUTES, type FrontendSubscription } from '@/stores/subscription/types'
import {
  formatExpireTime as formatExpireTimeText,
  formatIntervalLabel as formatIntervalLabelText,
  formatAutoUpdateHealth as formatAutoUpdateHealthText,
  formatLocalTime,
  formatTrafficSummary as formatTrafficSummaryText,
  generateConfigFileName,
  hasSubscriptionTraffic,
  isJsonContent,
} from '@/views/sub/subscription-utils'
import { useSubscriptionAutoUpdate } from '@/views/sub/useSubscriptionAutoUpdate'
import {
  AddOutline,
  LinkOutline,
  CopyOutline,
  CreateOutline,
  TrashOutline,
  CheckmarkCircleOutline,
  PlayCircleOutline,
  CodeOutline,
  EllipsisVerticalOutline,
  GlobeOutline,
  TimeOutline,
  CalendarOutline,
  AlertCircleOutline,
  RefreshOutline,
  ArrowUndoOutline,
  TimerOutline,
} from '@vicons/ionicons5'
import type { FormInst, FormRules, DropdownOption } from 'naive-ui'
import PageHeader from '@/components/common/PageHeader.vue'
import ConfigEditor from '@/components/common/ConfigEditor.vue'

defineOptions({
  name: 'SubView',
})

type Subscription = FrontendSubscription

interface SubscriptionForm extends Subscription {
  uriContent?: string
}

const message = useMessage()
const subStore = useSubStore()
const appStore = useAppStore()
const { t } = useI18n()

const showAddModal = ref(false)
const editIndex = ref<number | null>(null)
const formRef = ref<FormInst | null>(null)
const isLoading = ref(false)
const activeTab = ref('url')
const useOriginalTouched = ref(false)

const showConfigModal = ref(false)
const currentConfig = ref('')
const isConfigLoading = ref(false)
const configFormatError = ref('')

const formValue = ref<SubscriptionForm>({
  name: '',
  url: '',
  isLoading: false,
  isManual: false,
  manualContent: '',
  uriContent: '',
  useOriginalConfig: false,
  autoUpdateIntervalMinutes: DEFAULT_AUTO_UPDATE_MINUTES,
})

const autoUpdateOptions = computed(() => [
  { label: t('sub.autoUpdateOff'), value: 0 },
  { label: t('sub.autoUpdate6h'), value: 360 },
  { label: t('sub.autoUpdate12h'), value: 720 },
  { label: t('sub.autoUpdate1d'), value: 1440 },
])

const autoUpdateDisabled = computed(() => activeTab.value !== 'url')

const markUseOriginalTouched = () => {
  useOriginalTouched.value = true
}

const rules: FormRules = {
  name: [{ required: true, message: t('sub.nameRequired'), trigger: 'blur' }],
  url: [
    {
      required: true,
      message: t('sub.urlRequired'),
      trigger: 'blur',
      validator: (rule, value) => (activeTab.value === 'url' ? !!value : true),
    },
  ],
  manualContent: [
    {
      required: true,
      message: t('sub.contentRequired'),
      trigger: 'blur',
      validator: (rule, value) => (activeTab.value === 'manual' ? !!value : true),
    },
  ],
  uriContent: [
    {
      required: true,
      message: t('sub.uriContentRequired'),
      trigger: 'blur',
      validator: (rule, value) => (activeTab.value === 'uri' ? !!value : true),
    },
  ],
}

const resolvePersistOptionsFor = (item: Subscription) => {
  if (item.configPath && item.configPath.length > 0) {
    return { configPath: item.configPath }
  }
  return { fileName: generateConfigFileName(item.name || 'sub') }
}

const formatIntervalLabel = (minutes?: number) =>
  formatIntervalLabelText(minutes, t, DEFAULT_AUTO_UPDATE_MINUTES)

const getDropdownOptions = (index: number): DropdownOption[] => [
  {
    label: t('sub.copyLink'),
    key: 'copy',
    icon: () => h('span', { class: 'icon' }, [h(CopyOutline)]),
    props: { onClick: () => copyUrl(subStore.list[index].url) },
  },
  {
    label: t('sub.edit'),
    key: 'edit',
    icon: () => h('span', { class: 'icon' }, [h(CreateOutline)]),
    props: { onClick: () => handleEdit(index, subStore.list[index]) },
  },
  {
    label: t('sub.editConfig'),
    key: 'edit-config',
    icon: () => h('span', { class: 'icon' }, [h(CodeOutline)]),
    show: subStore.activeIndex === index,
    props: { onClick: editCurrentConfig },
  },
  {
    label: t('sub.refreshNow'),
    key: 'refresh',
    icon: () => h('span', { class: 'icon' }, [h(RefreshOutline)]),
    props: {
      onClick: () =>
        refreshSubscription(index, subStore.activeIndex === index && appStore.isRunning),
    },
  },
  {
    label: t('sub.rollback'),
    key: 'rollback',
    icon: () => h('span', { class: 'icon' }, [h(ArrowUndoOutline)]),
    props: { onClick: () => rollbackSubscription(index) },
  },
  { type: 'divider', key: 'd1' },
  {
    label: t('common.delete'),
    key: 'delete',
    icon: () => h('span', { class: 'icon delete' }, [h(TrashOutline)]),
    disabled: subStore.activeIndex === index,
    props: { onClick: () => deleteSubscription(index) },
  },
]

const resetForm = () => {
  formValue.value = {
    name: '',
    url: '',
    isLoading: false,
    isManual: false,
    manualContent: '',
    uriContent: '',
    useOriginalConfig: false,
    autoUpdateIntervalMinutes: DEFAULT_AUTO_UPDATE_MINUTES,
  }
  useOriginalTouched.value = false
  editIndex.value = null
  activeTab.value = 'url'
}

watch(activeTab, (tab) => {
  if (tab === 'uri') {
    formValue.value.useOriginalConfig = false
  }
  if (tab !== 'url') {
    formValue.value.autoUpdateIntervalMinutes = 0
  }
  if (editIndex.value !== null || useOriginalTouched.value) return
  formValue.value.useOriginalConfig = tab === 'manual'
})

const handleEdit = (index: number, item: Subscription) => {
  const manualContent = item.manualContent ?? ''
  const isJson = isJsonContent(manualContent)
  const isNodeList = !isJson && manualContent.trim().length > 0
  editIndex.value = index
  formValue.value = {
    ...item,
    manualContent: item.isManual && isJson ? manualContent : '',
    uriContent: item.isManual && !isJson && isNodeList ? manualContent : '',
  }
  activeTab.value = item.isManual ? (isNodeList ? 'uri' : 'manual') : 'url'
  showAddModal.value = true
}

const handleConfirm = () => {
  formRef.value?.validate(async (errors) => {
    if (errors) return
    try {
      isLoading.value = true
      const isUrlTab = activeTab.value === 'url'
      const isJsonTab = activeTab.value === 'manual'
      const isUriTab = activeTab.value === 'uri'
      const urlInput = formValue.value.url?.trim() ?? ''
      const manualInput = formValue.value.manualContent?.trim() ?? ''
      const uriInput = formValue.value.uriContent?.trim() ?? ''
      const resolvedManualContent = isJsonTab ? manualInput : isUriTab ? uriInput : ''
      const isManual = !isUrlTab
      const useOriginalConfig = isUriTab ? false : formValue.value.useOriginalConfig
      const persistOptions = { fileName: generateConfigFileName(formValue.value.name || 'sub') }
      let savedResult: SubscriptionPersistResult | null = null

      if (editIndex.value === null) {
        if (isManual && resolvedManualContent) {
          if (useOriginalConfig && !isJsonContent(resolvedManualContent)) {
            message.warning(t('sub.originalConfigJsonOnly'))
            return
          }
          savedResult = await subscriptionService.addManualSubscription(
            resolvedManualContent,
            useOriginalConfig,
            { ...persistOptions, applyRuntime: false },
          )
        } else if (!isManual) {
          savedResult = await subscriptionService.downloadSubscription(
            urlInput,
            useOriginalConfig,
            { ...persistOptions, applyRuntime: false },
          )
        }
        const savedPath = savedResult?.configPath ?? null

        const { uriContent, ...base } = formValue.value
        const newItem: Subscription = {
          ...base,
          url: isManual ? '' : urlInput,
          lastUpdate: Date.now(),
          isManual,
          manualContent: isManual ? resolvedManualContent : undefined,
          useOriginalConfig,
          autoUpdateIntervalMinutes: isManual ? 0 : base.autoUpdateIntervalMinutes,
          configPath: savedPath || undefined,
          backupPath: savedPath ? `${savedPath}.bak` : undefined,
        }
        applySubscriptionUserinfo(newItem, savedResult, isManual)

        subStore.list.push(newItem)
        await subStore.saveToBackend()
        await subStore.setActiveIndex(subStore.list.length - 1)

        if (savedPath) {
          await subscriptionService.setActiveConfig(savedPath, { useOriginalConfig })
          await appStore.setActiveConfigPath(savedPath)
        }

        message.success(t('sub.addAndUseSuccess'))
      } else {
        if (isManual && resolvedManualContent && useOriginalConfig) {
          if (!isJsonContent(resolvedManualContent)) {
            message.warning(t('sub.originalConfigJsonOnly'))
            return
          }
        }
        const { uriContent, ...base } = formValue.value
        subStore.list[editIndex.value] = {
          ...subStore.list[editIndex.value],
          ...base,
          url: isManual ? '' : urlInput,
          isManual,
          manualContent: isManual ? resolvedManualContent : undefined,
          useOriginalConfig,
          autoUpdateIntervalMinutes: isManual ? 0 : base.autoUpdateIntervalMinutes,
        }
        if (isManual) {
          applySubscriptionUserinfo(subStore.list[editIndex.value], null, true)
        }
        await subStore.saveToBackend()
        message.success(t('sub.updateSuccess'))
      }

      showAddModal.value = false
      resetForm()
    } catch (error) {
      message.error(t('sub.operationFailed') + error)
    } finally {
      isLoading.value = false
    }
  })
}

const handleCancel = () => {
  showAddModal.value = false
  resetForm()
}

const deleteSubscription = async (index: number) => {
  if (subStore.activeIndex === index) {
    message.warning(t('sub.cannotDeleteActive'))
    return
  }
  const target = subStore.list[index]
  try {
    if (target?.configPath) {
      await subscriptionService.deleteConfig(target.configPath)
    }
    subStore.list.splice(index, 1)
    if (subStore.activeIndex !== null && subStore.activeIndex > index) {
      await subStore.setActiveIndex(subStore.activeIndex - 1)
    }
    message.success(t('sub.deleteSuccess'))
  } catch (error) {
    message.error(t('sub.operationFailed') + error)
  }
}

const refreshSubscription = async (index: number, applyRuntime = false, silent = false) => {
  const item = subStore.list[index]
  if (!item) return

  if (item.isManual && !item.manualContent) {
    message.error(t('sub.manualContentMissing'))
    return
  }
  if (item.isManual && item.useOriginalConfig && !isJsonContent(item.manualContent ?? '')) {
    message.warning(t('sub.originalConfigJsonOnly'))
    return
  }

  const persistOptions = {
    ...resolvePersistOptionsFor(item),
    applyRuntime,
  }

  try {
    subStore.list[index].isLoading = true
    const savedResult = item.isManual
      ? await subscriptionService.addManualSubscription(
          item.manualContent || '',
          item.useOriginalConfig,
          persistOptions,
        )
      : await subscriptionService.downloadSubscription(
          item.url,
          item.useOriginalConfig,
          persistOptions,
        )

    const savedPath = savedResult.configPath
    if (savedPath) {
      subStore.list[index].configPath = savedPath
      subStore.list[index].backupPath = `${savedPath}.bak`
    }
    applySubscriptionUserinfo(subStore.list[index], savedResult, item.isManual)
    subStore.list[index].lastUpdate = Date.now()
    await subStore.saveToBackend()

    if (savedPath && applyRuntime) {
      await subscriptionService.setActiveConfig(savedPath, {
        useOriginalConfig: item.useOriginalConfig,
      })
      await appStore.setActiveConfigPath(savedPath)
    }

    if (!silent) {
      message.success(applyRuntime ? t('sub.refreshAndApplied') : t('sub.refreshSuccess'))
    }
  } catch (error) {
    message.error(t('sub.refreshFailed') + error)
  } finally {
    if (index >= 0 && index < subStore.list.length) {
      subStore.list[index].isLoading = false
    }
  }
}

const rollbackSubscription = async (index: number) => {
  const item = subStore.list[index]
  if (!item?.configPath) {
    message.error(t('sub.missingConfigFile'))
    return
  }
  try {
    await subscriptionService.rollbackConfig(item.configPath)
    message.success(t('sub.rollbackSuccess'))
    if (subStore.activeIndex === index) {
      await subscriptionService.setActiveConfig(item.configPath, {
        useOriginalConfig: item.useOriginalConfig,
      })
      await appStore.setActiveConfigPath(item.configPath)
      if (appStore.isRunning) {
        await kernelService.restartKernel()
      }
    }
  } catch (error) {
    message.error(t('sub.rollbackFailed') + error)
  }
}

const useSubscription = async (index: number) => {
  const item = subStore.list[index]
  if (!item) return
  if (!item.configPath) {
    message.warning(t('sub.missingConfigFile'))
    return
  }

  try {
    subStore.list[index].isLoading = true
    const activePath = appStore.activeConfigPath
    if (activePath && item.configPath === activePath && subStore.activeIndex !== index) {
      message.warning(t('sub.configPathCollision'))
      const regeneratedPath = await regenerateConfigFor(item)
      if (regeneratedPath) {
        subStore.list[index].configPath = regeneratedPath
        subStore.list[index].backupPath = `${regeneratedPath}.bak`
        subStore.list[index].lastUpdate = Date.now()
        item.configPath = regeneratedPath
      }
    }
    if (!item.configPath) {
      throw new Error(t('sub.missingConfigFile'))
    }
    await subStore.saveToBackend()
    await subscriptionService.setActiveConfig(item.configPath, {
      useOriginalConfig: item.useOriginalConfig,
    })
    await appStore.setActiveConfigPath(item.configPath)
    await subStore.setActiveIndex(index)
    subStore.list[index].lastUpdate = Date.now()
    message.success(t('sub.useSuccess'))
  } catch (error) {
    message.error(t('sub.useFailed') + error)
  } finally {
    if (index >= 0 && index < subStore.list.length) {
      subStore.list[index].isLoading = false
    }
  }
}

const copyUrl = (url: string) => {
  navigator.clipboard.writeText(url)
  message.success(t('sub.linkCopied'))
}

const applySubscriptionUserinfo = (
  target: Subscription,
  result: SubscriptionPersistResult | null,
  isManual: boolean,
) => {
  if (isManual || !result) {
    target.subscriptionUpload = undefined
    target.subscriptionDownload = undefined
    target.subscriptionTotal = undefined
    target.subscriptionExpire = undefined
    return
  }

  target.subscriptionUpload = result.subscriptionUpload
  target.subscriptionDownload = result.subscriptionDownload
  target.subscriptionTotal = result.subscriptionTotal
  target.subscriptionExpire = result.subscriptionExpire
}

const formatTrafficSummary = (item: Subscription) => formatTrafficSummaryText(item, t)
const formatExpireTime = (timestamp?: number) => formatExpireTimeText(timestamp, t)
const formatTime = (timestamp: number): string => formatLocalTime(timestamp)
const formatAutoUpdateHealth = (item: Subscription) => formatAutoUpdateHealthText(item, t)

const getTrafficPercent = (item: Subscription): number => {
  const total = item.subscriptionTotal
  const used = (item.subscriptionUpload ?? 0) + (item.subscriptionDownload ?? 0)
  if (!total || total <= 0) return 0
  return Math.min(100, Math.round((used / total) * 100))
}

const getTrafficClass = (item: Subscription): string => {
  const pct = getTrafficPercent(item)
  if (pct >= 90) return 'danger'
  if (pct >= 70) return 'warning'
  return 'normal'
}

const regenerateConfigFor = async (item: Subscription) => {
  const persistOptions = {
    fileName: generateConfigFileName(item.name || 'sub'),
    applyRuntime: false,
  }
  if (item.isManual) {
    const content = item.manualContent?.trim() ?? ''
    if (!content) {
      throw new Error(t('sub.manualContentMissing'))
    }
    const result = await subscriptionService.addManualSubscription(
      content,
      item.useOriginalConfig,
      persistOptions,
    )
    return result.configPath
  }
  const result = await subscriptionService.downloadSubscription(
    item.url,
    item.useOriginalConfig,
    persistOptions,
  )
  return result.configPath
}

const detectConfigFormat = (content: string): 'json' | 'yaml' | 'text' => {
  const trimmed = content.trim()
  if (trimmed.startsWith('{') || trimmed.startsWith('[')) return 'json'
  if (/^([a-zA-Z_][a-zA-Z0-9_-]*:\s|---\s*$)/m.test(trimmed)) return 'yaml'
  return 'text'
}

const validateConfigFormat = (content: string): string => {
  const format = detectConfigFormat(content)
  if (format === 'json') {
    try {
      JSON.parse(content)
      return ''
    } catch (error) {
      return error instanceof Error ? error.message : t('sub.invalidJson')
    }
  }
  return ''
}

const editCurrentConfig = async () => {
  try {
    isConfigLoading.value = true
    configFormatError.value = ''
    const config = await subscriptionService.getCurrentConfig()
    if (typeof config === 'string') {
      currentConfig.value = config
      showConfigModal.value = true
    }
  } catch (error) {
    message.error(t('sub.readConfigFailed') + error)
  } finally {
    isConfigLoading.value = false
  }
}

const saveCurrentConfig = async () => {
  const formatError = validateConfigFormat(currentConfig.value)
  if (formatError) {
    configFormatError.value = formatError
    return
  }

  try {
    isConfigLoading.value = true
    const activeItem = subStore.getActiveSubscription()
    const persistOptions = activeItem?.configPath
      ? { configPath: activeItem.configPath, applyRuntime: true }
      : { fileName: generateConfigFileName(activeItem?.name || 'sub'), applyRuntime: true }

    const savedResult = await subscriptionService.addManualSubscription(
      currentConfig.value,
      false,
      persistOptions,
    )
    const savedPath = savedResult.configPath

    if (activeItem) {
      if (activeItem.isManual) {
        activeItem.manualContent = currentConfig.value
      }
      activeItem.lastUpdate = Date.now()
      activeItem.configPath = savedPath || activeItem.configPath
      activeItem.backupPath = savedPath ? `${savedPath}.bak` : activeItem.backupPath
    }
    message.success(t('sub.configSaved'))
    showConfigModal.value = false
  } catch (error) {
    message.error(t('sub.saveConfigFailed') + error)
  } finally {
    isConfigLoading.value = false
  }
}

const { startAutoUpdateLoop, stopAutoUpdateLoop } = useSubscriptionAutoUpdate({
  getSubscriptions: () => subStore.list,
  getActiveIndex: () => subStore.activeIndex,
  isKernelRunning: () => appStore.isRunning,
  defaultIntervalMinutes: DEFAULT_AUTO_UPDATE_MINUTES,
  onRefresh: refreshSubscription,
})

onMounted(() => {
  subStore.resetLoadingState()
  startAutoUpdateLoop()
})

onUnmounted(() => {
  stopAutoUpdateLoop()
})
</script>

<style scoped>
.page-shell {
  display: flex;
  flex-direction: column;
  gap: var(--layout-page-gap, 24px);
  height: 100%;
}

.subscription-section {
  flex: 1;
  overflow-y: auto;
  padding: 2px 2px 10px;
}

/* ── Subscription Card ─────────────────────────────────── */
.sub-card {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  border-radius: 10px;
  border: 1px solid var(--border-color);
  background: var(--panel-bg);
  backdrop-filter: var(--glass-blur);
  transition: all var(--transition-normal);
  box-shadow: 0 2px 10px -6px rgba(15, 23, 42, 0.18);
  height: 100%;
  box-sizing: border-box;
}

.sub-card:hover {
  border-color: var(--border-hover);
  box-shadow: 0 8px 22px -12px rgba(15, 23, 42, 0.28);
  transform: translateY(-1px);
}

.sub-card.active {
  border-color: rgba(16, 185, 129, 0.55);
  background: rgba(16, 185, 129, 0.04);
  box-shadow: 0 8px 24px -14px rgba(16, 185, 129, 0.35);
}

/* ── Card Header ───────────────────────────────────────── */
.sub-card-header {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding-bottom: 2px;
}

.sub-avatar {
  width: 36px;
  height: 36px;
  border-radius: 8px;
  background: var(--bg-tertiary);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-secondary);
  flex-shrink: 0;
  margin-top: 2px;
  transition: background var(--transition-fast);
}

.sub-avatar.active {
  background: var(--success-color);
  color: #fff;
}

.sub-header-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.sub-name {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
  line-height: 1.3;
}

/* ── Badges ────────────────────────────────────────────── */
.sub-badges {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}

.badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11px;
  font-weight: 500;
  padding: 2px 8px;
  border-radius: 20px;
  line-height: 1.6;
}

.badge-type {
  background: var(--bg-tertiary);
  color: var(--text-tertiary);
}

.badge-active {
  background: rgba(16, 185, 129, 0.12);
  color: var(--success-color, #10b981);
}

.badge-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
}

.badge-timer {
  background: var(--bg-tertiary);
  color: var(--text-tertiary);
}

.more-btn {
  color: var(--text-tertiary);
  flex-shrink: 0;
  margin-top: 2px;
}
.more-btn:hover {
  color: var(--text-primary);
}

/* ── Traffic Section ───────────────────────────────────── */
.traffic-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.traffic-bar-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.traffic-label {
  font-size: 12px;
  color: var(--text-tertiary);
  font-weight: 500;
}

.traffic-value {
  font-size: 12px;
  color: var(--text-secondary);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.traffic-bar-bg {
  height: 5px;
  border-radius: 3px;
  background: var(--bg-tertiary);
  overflow: hidden;
}

.traffic-bar-fill {
  height: 100%;
  border-radius: 3px;
  transition: width 0.4s ease;
}

.traffic-bar-fill.normal {
  background: linear-gradient(90deg, var(--primary-color), #818cf8);
}

.traffic-bar-fill.warning {
  background: linear-gradient(90deg, #f59e0b, #fbbf24);
}

.traffic-bar-fill.danger {
  background: linear-gradient(90deg, #ef4444, #f87171);
}

/* ── Meta Info ─────────────────────────────────────────── */
.sub-meta {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  background: rgba(148, 163, 184, 0.08);
  border: 1px solid var(--border-light);
  border-radius: 8px;
  flex: 1;
}

.meta-row {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 12.5px;
  color: var(--text-secondary);
  min-width: 0;
}

.meta-row.warn {
  color: #f59e0b;
}

.meta-icon {
  flex-shrink: 0;
  color: var(--text-tertiary);
}

.meta-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.url-text {
  font-family: monospace;
  font-size: 11.5px;
  color: var(--text-tertiary);
}

/* ── Action Button ─────────────────────────────────────── */
.sub-action-btn {
  margin-top: auto;
}

/* ── Empty State ───────────────────────────────────────── */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 64px 0;
  color: var(--text-secondary);
}

.code-input {
  font-family: 'JetBrains Mono', monospace;
}

.config-editor-modal :deep(.n-dialog__content) {
  padding-bottom: 8px;
}

.config-format-error {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  padding: 8px 12px;
  border-radius: 8px;
  background: rgba(208, 48, 80, 0.08);
  border: 1px solid rgba(208, 48, 80, 0.15);
  color: #d03050;
  font-size: 13px;
  line-height: 1.5;
}

/* Dropdown Icon Styles */
:deep(.icon) {
  display: flex;
  align-items: center;
  justify-content: center;
}

:deep(.icon.delete) {
  color: #ef4444;
}
</style>
