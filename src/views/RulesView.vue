<template>
  <div class="page-shell">
    <PageHeader :title="t('rules.title')" :subtitle="t('rules.subtitle')">
      <template #actions>
        <n-space>
          <n-button secondary @click="refreshAll">
            <template #icon><n-icon><RefreshOutline /></n-icon></template>
            {{ t('common.refresh') }}
          </n-button>

          <n-button v-if="activeTab === 'providers'" type="primary" secondary :loading="bulkUpdating" @click="updateAllProviders">
            {{ t('rules.updateAll') }}
          </n-button>

          <n-button v-if="activeTab === 'custom'" type="primary" secondary @click="openCreateCustomRule">
            <template #icon><n-icon><AddOutline /></n-icon></template>
            {{ t('rules.custom.add') }}
          </n-button>
        </n-space>
      </template>
    </PageHeader>

    <n-tabs v-model:value="activeTab" type="segment" size="small" style="margin-bottom: 16px; width: 320px;">
      <n-tab-pane name="rules" :tab="t('rules.rulesTab')" />
      <n-tab-pane name="providers" :tab="t('rules.providersTab')" />
      <n-tab-pane name="custom" :tab="t('rules.custom.tab')" />
    </n-tabs>

    <n-card v-if="activeTab !== 'custom'" size="small" class="toolbar-card">
      <n-grid responsive="screen" cols="1 m:2" :x-gap="16" :y-gap="16">
        <n-grid-item>
          <n-input v-model:value="searchQuery" :placeholder="t('rules.searchPlaceholder')" clearable>
            <template #prefix><n-icon><SearchOutline /></n-icon></template>
          </n-input>
        </n-grid-item>

        <n-grid-item v-if="activeTab === 'rules'">
          <n-select v-model:value="typeFilter" :options="typeOptions" clearable :placeholder="t('rules.type')" />
        </n-grid-item>
      </n-grid>
    </n-card>

    <n-flex v-if="activeTab === 'rules'" vertical style="margin-top: 24px;" class="card-list">
      <n-list v-if="filteredRules.length" hoverable class="rules-list" :show-divider="true">
        <n-list-item v-for="rule in filteredRules" :key="rule.index" style="padding: 12px 16px;">
          <n-flex justify="space-between" align="center" :wrap="false">
            <!-- Left Info -->
            <n-flex align="center" :wrap="false" style="flex: 1; overflow: hidden; min-width: 0;">
              <n-text depth="3" style="width: 40px; text-align: center;">{{ typeof rule.index === 'number' ? rule.index : '-' }}</n-text>
              <n-tag round size="small" :bordered="false" style="min-width: 80px; justify-content: center;">{{ rule.type }}</n-tag>
              <n-ellipsis :tooltip="true" style="flex: 1; margin-left: 12px;">
                <n-text>{{ rule.payload || '*' }}</n-text>
              </n-ellipsis>
            </n-flex>
            
            <!-- Right Actions -->
            <n-flex align="center" :wrap="false" style="min-width: 240px; justify-content: flex-end;">
              <n-text depth="3" style="font-size: 13px; margin-right: 12px;">{{ getProxyLabel(rule.proxy) }}</n-text>
              <n-switch
                size="small"
                :value="!rule.extra?.disabled"
                :loading="typeof rule.index === 'number' ? rulesStore.ruleUpdatingMap[rule.index] : false"
                @update:value="toggleRule(rule)"
              />
            </n-flex>
          </n-flex>
        </n-list-item>
      </n-list>

      <n-empty v-else size="huge" :description="t('rules.noRulesData')" style="margin-top: 48px;">
        <template #icon><n-icon><FilterOutline /></n-icon></template>
      </n-empty>
    </n-flex>

    <n-flex v-else-if="activeTab === 'providers'" vertical style="margin-top: 24px;" class="card-list">
      <n-grid v-if="filteredProviders.length" responsive="screen" cols="1 s:2 m:3 l:4 xl:5" :x-gap="12" :y-gap="12" class="providers-grid">
        <n-grid-item v-for="provider in filteredProviders" :key="provider.name">
          <n-card hoverable size="small" class="provider-card">
            <template #header>
              <n-flex justify="space-between" align="center">
                <n-flex vertical :size="0">
                  <n-text strong>{{ provider.name }}</n-text>
                  <n-text depth="3" style="font-size: 12px;">{{ provider.behavior || '-' }} · {{ provider.vehicleType || '-' }}</n-text>
                </n-flex>
                <n-button
                  size="small"
                  secondary
                  :loading="rulesStore.providerUpdatingMap[provider.name]"
                  @click="updateProvider(provider.name)"
                >
                  {{ t('common.refresh') }}
                </n-button>
              </n-flex>
            </template>
            <n-flex vertical size="small">
              <n-flex justify="space-between" align="center">
                <n-text depth="3">{{ t('rules.count') }}</n-text>
                <n-text strong>{{ provider.count ?? '-' }}</n-text>
              </n-flex>
              <n-flex justify="space-between" align="center">
                <n-text depth="3">{{ t('rules.updatedAt') }}</n-text>
                <n-text strong>{{ formatProviderTime(provider.updatedAt || provider.updateAt) }}</n-text>
              </n-flex>
            </n-flex>
          </n-card>
        </n-grid-item>
      </n-grid>

      <n-empty v-else size="huge" :description="t('rules.noProviders')" style="margin-top: 48px;">
        <template #icon><n-icon><FilterOutline /></n-icon></template>
      </n-empty>
    </n-flex>

    <n-flex v-if="activeTab === 'custom'" vertical style="margin-top: 24px;" class="card-list">
      <n-alert type="info" :show-icon="false" class="custom-hint">{{ t('rules.custom.hint') }}</n-alert>
      <n-list v-if="rulesStore.customRules.length" hoverable class="rules-list" :show-divider="true">
        <n-list-item v-for="rule in rulesStore.customRules" :key="rule.id" style="padding: 12px 16px;">
          <n-flex justify="space-between" align="center" :wrap="false">
            <n-flex align="center" :wrap="false" style="flex: 1; overflow: hidden; min-width: 0;">
              <n-tag round size="small" :bordered="false" style="min-width: 80px; justify-content: center;">{{ matchTypeLabel(rule.match_type) }}</n-tag>
              <n-ellipsis :tooltip="true" style="flex: 1; margin-left: 12px;">
                <n-text>{{ rule.payload || '*' }}</n-text>
              </n-ellipsis>
            </n-flex>
            
            <n-flex align="center" :wrap="false" style="min-width: 260px; justify-content: flex-end;">
              <n-tag size="small" round style="margin-right: 12px;">{{ actionLabel(rule.action) }}</n-tag>
              <n-space size="small" style="margin-right: 16px;">
                <n-button size="tiny" secondary @click="openEditCustomRule(rule)">
                  {{ t('rules.custom.edit') }}
                </n-button>
                <n-popconfirm @positive-click="onDeleteCustomRule(rule.id)">
                  <template #trigger>
                    <n-button size="tiny" secondary type="error">{{ t('rules.custom.delete') }}</n-button>
                  </template>
                  {{ t('rules.custom.deleteConfirm') }}
                </n-popconfirm>
              </n-space>
              <n-switch
                size="small"
                :value="rule.enabled"
                :loading="rulesStore.customRuleUpdating[rule.id]"
                @update:value="onToggleCustomRule(rule.id)"
              />
            </n-flex>
          </n-flex>
        </n-list-item>
      </n-list>
      <n-empty v-else size="huge" :description="t('rules.custom.empty')" style="margin-top: 48px;">
        <template #icon><n-icon><AddOutline /></n-icon></template>
        <template #extra>
          <n-button type="primary" secondary style="margin-top: 16px" @click="openCreateCustomRule">
            {{ t('rules.custom.add') }}
          </n-button>
        </template>
      </n-empty>
    </n-flex>

    <!-- 自定义规则编辑表单 -->
    <n-modal
      v-model:show="customModalShow"
      preset="card"
      :title="editingCustomRule ? t('rules.custom.editTitle') : t('rules.custom.addTitle')"
      style="max-width: 520px"
    >
      <n-form label-placement="top">
        <n-form-item :label="t('rules.custom.matchType')">
          <n-select v-model:value="customForm.matchType" :options="matchTypeOptions" />
        </n-form-item>
        <n-form-item :label="t('rules.custom.payload')">
          <n-input
            v-model:value="customForm.payload"
            type="textarea"
            :rows="3"
            :placeholder="t('rules.custom.payloadPlaceholder')"
          />
        </n-form-item>
        <n-form-item :label="t('rules.custom.action')">
          <n-select v-model:value="customForm.action" :options="actionOptions" />
        </n-form-item>
        <n-form-item :label="t('rules.custom.note')">
          <n-input v-model:value="customForm.note" :placeholder="t('rules.custom.notePlaceholder')" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="customModalShow = false">{{ t('rules.custom.cancel') }}</n-button>
          <n-button type="primary" :loading="customSubmitting" @click="submitCustomRule">
            {{ t('rules.custom.confirm') }}
          </n-button>
        </n-space>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { useMessage } from 'naive-ui'
import { AddOutline, FilterOutline, RefreshOutline, SearchOutline } from '@vicons/ionicons5'
import PageHeader from '@/components/common/PageHeader.vue'
import { useRulesStore } from '@/stores/kernel/RulesStore'
import { useI18n } from 'vue-i18n'
import type { RuleItem } from '@/types/controller'
import type {
  CustomRule,
  CustomRuleAction,
  CustomRuleMatchType,
} from '@/types/generated'

defineOptions({
  name: 'RulesView',
})

const { t } = useI18n()
const message = useMessage()
const rulesStore = useRulesStore()
const activeTab = ref<'rules' | 'providers' | 'custom'>('rules')
const searchQuery = ref('')
const typeFilter = ref<string | null>(null)

// 自定义规则表单状态
const customModalShow = ref(false)
const customSubmitting = ref(false)
const editingCustomRule = ref<CustomRule | null>(null)
const customForm = reactive({
  matchType: 'domain_suffix' as CustomRuleMatchType,
  payload: '',
  action: 'direct' as CustomRuleAction,
  note: '',
})

const matchTypeOptions = computed(() => {
  return [
    { label: t('rules.custom.matchTypes.domainSuffix'), value: 'domain_suffix' as CustomRuleMatchType },
    { label: t('rules.custom.matchTypes.domain'), value: 'domain' as CustomRuleMatchType },
    { label: t('rules.custom.matchTypes.domainKeyword'), value: 'domain_keyword' as CustomRuleMatchType },
    { label: 'IP CIDR', value: 'ip_cidr' as CustomRuleMatchType },
  ]
})

const actionOptions = computed(() => {
  return [
    { label: t('rules.custom.actions.direct'), value: 'direct' as CustomRuleAction },
    { label: t('rules.custom.actions.proxy'), value: 'proxy' as CustomRuleAction },
    { label: t('rules.custom.actions.block'), value: 'block' as CustomRuleAction },
  ]
})

const matchTypeLabel = (mt: CustomRuleMatchType) => {
  return matchTypeOptions.value.find((o) => o.value === mt)?.label || mt
}

const actionLabel = (act: CustomRuleAction) =>
  actionOptions.value.find((o) => o.value === act)?.label || act

const filteredRules = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  return rulesStore.rules.filter((rule) => {
    const matchesQuery =
      !query ||
      rule.type.toLowerCase().includes(query) ||
      rule.payload.toLowerCase().includes(query) ||
      rule.proxy.toLowerCase().includes(query)

    const matchesType = !typeFilter.value || rule.type === typeFilter.value
    return matchesQuery && matchesType
  })
})

const filteredProviders = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  return rulesStore.providers.filter((provider) => {
    return (
      !query ||
      provider.name.toLowerCase().includes(query) ||
      (provider.behavior || '').toLowerCase().includes(query) ||
      (provider.vehicleType || '').toLowerCase().includes(query)
    )
  })
})

const typeOptions = computed(() =>
  rulesStore.ruleTypes.map((item) => ({
    label: item,
    value: item,
  })),
)

const bulkUpdating = computed(() =>
  Object.values(rulesStore.providerUpdatingMap).some(Boolean),
)

const refreshAll = async () => {
  try {
    await rulesStore.fetchAll()
    message.success(t('rules.fetchSuccess', { count: rulesStore.rules.length }))
  } catch (error) {
    message.error(t('rules.fetchError', { error: String(error) }))
  }
}

const updateProvider = async (providerName: string) => {
  try {
    await rulesStore.updateProvider(providerName)
    message.success(providerName)
  } catch (error) {
    message.error(String(error))
  }
}

const updateAllProviders = async () => {
  try {
    await rulesStore.updateAllProviders()
    message.success(t('rules.updateAllSuccess'))
  } catch (error) {
    message.error(String(error))
  }
}

const toggleRule = async (rule: RuleItem) => {
  try {
    await rulesStore.toggleDisabled(rule)
  } catch (error) {
    message.error(String(error))
  }
}

const resetCustomForm = () => {
  customForm.matchType = 'domain_suffix'
  customForm.payload = ''
  customForm.action = 'direct'
  customForm.note = ''
}

const openCreateCustomRule = () => {
  editingCustomRule.value = null
  resetCustomForm()
  customModalShow.value = true
}

const openEditCustomRule = (rule: CustomRule) => {
  editingCustomRule.value = rule
  customForm.matchType = rule.match_type
  customForm.payload = rule.payload
  customForm.action = rule.action
  customForm.note = rule.note ?? ''
  customModalShow.value = true
}

const submitCustomRule = async () => {
  if (!customForm.payload.trim()) {
    message.error(t('rules.custom.payloadPlaceholder'))
    return
  }
  customSubmitting.value = true
  try {
    if (editingCustomRule.value) {
      await rulesStore.updateCustomRule(
        editingCustomRule.value.id,
        customForm.matchType,
        customForm.payload,
        customForm.action,
        customForm.note || undefined,
      )
      message.success(t('rules.custom.updateSuccess'))
    } else {
      await rulesStore.addCustomRule(
        customForm.matchType,
        customForm.payload,
        customForm.action,
        customForm.note || undefined,
      )
      message.success(t('rules.custom.addSuccess'))
    }
    customModalShow.value = false
  } catch (error) {
    message.error(String(error))
  } finally {
    customSubmitting.value = false
  }
}

const onToggleCustomRule = (id: string) => async () => {
  try {
    await rulesStore.toggleCustomRule(id)
  } catch (error) {
    message.error(String(error))
  }
}

const onDeleteCustomRule = async (id: string) => {
  try {
    await rulesStore.deleteCustomRule(id)
    message.success(t('rules.custom.deleteSuccess'))
  } catch (error) {
    message.error(String(error))
  }
}

const getProxyLabel = (proxy: string) => {
  if (proxy === 'direct') return t('rules.directConnect')
  if (proxy === 'reject') return t('rules.blockAction')
  return proxy
}

const formatProviderTime = (value?: string) => {
  if (!value) return '-'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleString()
}

if (!rulesStore.rules.length && !rulesStore.providers.length && !rulesStore.customRules.length) {
  refreshAll()
}
</script>

<style scoped>
.page-shell {
  display: flex;
  flex-direction: column;
  gap: var(--layout-page-gap, 16px);
  height: 100%;
}

.toolbar-card,
.rule-card,
.provider-card {
  background: var(--glass-bg);
  backdrop-filter: var(--glass-blur);
  border: 1px solid var(--border-light);
  border-radius: var(--radius-lg);
  box-shadow: 0 4px 24px -6px rgba(0, 0, 0, 0.05);
}

.toolbar-card {
  padding: 16px;
}

.toolbar-row {
  margin-top: 12px;
  display: grid;
  grid-template-columns: minmax(220px, 1fr) 220px;
  gap: 12px;
}

.card-list {
  display: flex;
  flex-direction: column;
}

.rules-grid,
.providers-grid {
  display: grid;
  gap: 16px;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
}

.rule-card,
.provider-card {
  padding: 16px;
}

.rule-head,
.provider-head,
.rule-footer,
.provider-row {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  align-items: center;
}

.rule-meta {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.rule-payload {
  margin: 14px 0;
  color: var(--text-primary);
  line-height: 1.5;
  word-break: break-word;
}

.custom-note {
  margin: -8px 0 14px;
  font-size: 12px;
  color: var(--text-tertiary);
  word-break: break-word;
}

.custom-hint {
  font-size: 12px;
  color: var(--text-tertiary);
  line-height: 1.5;
  margin-bottom: 12px;
}

.rule-footer,
.provider-meta,
.provider-row {
  color: var(--text-secondary);
  font-size: 13px;
}

.provider-name {
  font-weight: 600;
  color: var(--text-primary);
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 64px 0;
  color: var(--text-secondary);
}

.empty-icon {
  margin-bottom: 16px;
  opacity: 0.5;
}

.empty-title {
  margin: 0;
  font-size: 18px;
  color: var(--text-primary);
}

@media (max-width: 900px) {
  .toolbar-row {
    grid-template-columns: 1fr;
  }
}
</style>
