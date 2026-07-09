<template>
  <div class="setting-section">
    <div class="setting-row config-viewer-row">
      <div class="setting-info">
        <div class="setting-label">{{ props.t('setting.network.currentConfig') }}</div>
        <div class="setting-desc">{{ props.t('setting.network.currentConfigDesc') }}</div>
      </div>
      <n-button
        size="small"
        secondary
        :loading="isCurrentConfigLoading"
        @click="openCurrentConfigViewer"
      >
        <template #icon><n-icon :size="14"><CodeOutline /></n-icon></template>
        {{ props.t('setting.network.viewCurrentConfig') }}
      </n-button>
    </div>

    <h3 class="setting-section-title">{{ props.t('setting.network.sections.inbound') }}</h3>

    <div class="setting-row">
      <div class="setting-info">
        <div class="setting-label">
          {{ props.t('setting.network.ipv6') }}
          <n-tag size="small" type="default" class="source-badge">
            {{ props.t('setting.network.badge.runtime') }}
          </n-tag>
        </div>
        <div class="setting-desc">{{ props.t('setting.network.ipv6Desc') }}</div>
      </div>
      <n-switch :value="props.appStore.preferIpv6" @update:value="props.onIpVersionChange" />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <div class="setting-label">
          {{ props.t('setting.network.ports') }}
          <n-tag size="small" type="default" class="source-badge">
            {{ props.t('setting.network.badge.runtime') }}
          </n-tag>
        </div>
        <div class="setting-desc">{{ props.t('setting.network.portsDesc') }}</div>
      </div>
      <n-button size="small" secondary @click="props.showPortSettings">
        <template #icon><n-icon :size="14"><SettingsOutline /></n-icon></template>
        {{ props.t('setting.network.configure') }}
      </n-button>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <div class="setting-label">
          {{ props.t('setting.network.allowLanAccess') }}
          <n-tag size="small" type="default" class="source-badge">
            {{ props.t('setting.network.badge.runtime') }}
          </n-tag>
        </div>
        <div class="setting-desc">{{ props.t('setting.network.allowLanAccessDesc') }}</div>
      </div>
      <n-switch
        :value="props.appStore.allowLanAccess"
        @update:value="props.onLanAccessChange"
      />
    </div>

    <h3 class="setting-section-title">{{ props.t('setting.network.sections.systemProxy') }}</h3>

    <n-form label-placement="top" class="advanced-form">
      <n-form-item>
        <template #label>
          <span class="form-item-label">
            {{ props.t('setting.proxyAdvanced.systemBypass') }}
            <n-tag size="small" type="default" class="source-badge">
              {{ props.t('setting.network.badge.runtime') }}
            </n-tag>
          </span>
        </template>
        <n-input
          v-model:value="proxyAdvancedForm.systemProxyBypass"
          type="textarea"
          :rows="3"
          :placeholder="props.t('setting.proxyAdvanced.systemBypassPlaceholder')"
        />
      </n-form-item>
    </n-form>

    <n-divider />
    <n-button
      type="primary"
      block
      :loading="savingAdvanced"
      @click="saveProxyAdvancedSettings"
    >
      {{ props.t('setting.proxyAdvanced.save') }}
    </n-button>

    <h3 class="setting-section-title">{{ props.t('setting.network.sections.tun') }}</h3>

    <n-form label-placement="top" class="advanced-form">
      <div class="setting-form-grid">
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.proxyAdvanced.tunMtu') }}
              <n-tag size="small" type="default" class="source-badge">
                {{ props.t('setting.network.badge.runtime') }}
              </n-tag>
            </span>
          </template>
          <n-input-number v-model:value="proxyAdvancedForm.tunMtu" :min="576" :max="9000" />
        </n-form-item>
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.proxyAdvanced.tunStack') }}
              <n-tag size="small" type="default" class="source-badge">
                {{ props.t('setting.network.badge.runtime') }}
              </n-tag>
            </span>
          </template>
          <n-select v-model:value="proxyAdvancedForm.tunStack" :options="props.tunStackOptions" />
        </n-form-item>
      </div>

      <n-form-item>
        <template #label>
          <span class="form-item-label">
            {{ props.t('setting.proxyAdvanced.tunRouteExcludeAddress') }}
            <n-tag size="small" type="default" class="source-badge">
              {{ props.t('setting.network.badge.runtime') }}
            </n-tag>
          </span>
        </template>
        <n-input
          v-model:value="proxyAdvancedForm.tunRouteExcludeAddressText"
          type="textarea"
          :rows="3"
          :placeholder="props.t('setting.proxyAdvanced.tunRouteExcludeAddressPlaceholder')"
        />
      </n-form-item>

      <div class="setting-toggles-grid">
        <div class="setting-toggle-item">
          <span class="setting-toggle-label">
            {{ props.t('setting.proxyAdvanced.enableIpv6') }}
            <n-tag size="small" type="default" class="source-badge">
              {{ props.t('setting.network.badge.runtime') }}
            </n-tag>
          </span>
          <n-switch v-model:value="proxyAdvancedForm.tunEnableIpv6" />
        </div>
        <div class="setting-toggle-item">
          <span class="setting-toggle-label">
            {{ props.t('setting.proxyAdvanced.autoRoute') }}
            <n-tag size="small" type="default" class="source-badge">
              {{ props.t('setting.network.badge.runtime') }}
            </n-tag>
          </span>
          <n-switch v-model:value="proxyAdvancedForm.tunAutoRoute" />
        </div>
        <div class="setting-toggle-item">
          <span class="setting-toggle-label">
            {{ props.t('setting.proxyAdvanced.strictRoute') }}
            <n-tag size="small" type="default" class="source-badge">
              {{ props.t('setting.network.badge.runtime') }}
            </n-tag>
          </span>
          <n-switch v-model:value="proxyAdvancedForm.tunStrictRoute" />
        </div>
        <div class="setting-toggle-item">
          <span class="setting-toggle-label">
            {{ props.t('setting.proxyAdvanced.tunSelfHeal') }}
            <n-tag size="small" type="default" class="source-badge">
              {{ props.t('setting.network.badge.runtime') }}
            </n-tag>
          </span>
          <n-switch v-model:value="proxyAdvancedForm.tunSelfHealEnabled" />
        </div>
      </div>

      <n-form-item v-if="proxyAdvancedForm.tunSelfHealEnabled">
        <template #label>
          <span class="form-item-label">
            {{ props.t('setting.proxyAdvanced.tunSelfHealCooldown') }}
            <n-tag size="small" type="default" class="source-badge">
              {{ props.t('setting.network.badge.runtime') }}
            </n-tag>
          </span>
        </template>
        <n-input-number
          v-model:value="proxyAdvancedForm.tunSelfHealCooldownSecs"
          :min="15"
          :max="600"
        />
      </n-form-item>
    </n-form>

    <n-divider />
    <n-button
      type="primary"
      block
      :loading="savingAdvanced"
      @click="saveProxyAdvancedSettings"
    >
      {{ props.t('setting.proxyAdvanced.save') }}
    </n-button>

    <h3 class="setting-section-title">{{ props.t('setting.network.sections.dns') }}</h3>

    <n-alert
      v-if="props.usingOriginalConfig"
      type="info"
      class="subscription-hint"
    >
      <n-flex align="center" :wrap="false" :size="8">
        <n-icon :size="16"><InformationCircleOutline /></n-icon>
        <span>{{ props.t('setting.network.hint.originalConfig') }}</span>
      </n-flex>
    </n-alert>

    <n-form label-placement="top" class="advanced-form">
      <div class="setting-form-grid">
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.singboxProfile.dnsProxy') }}
              <n-tag size="small" type="info" class="source-badge">
                {{ props.t('setting.network.badge.subscriptionProfile') }}
              </n-tag>
            </span>
          </template>
          <n-input
            v-model:value="singboxProfileForm.dnsProxy"
            placeholder="https://1.1.1.1/dns-query"
          />
        </n-form-item>
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.singboxProfile.dnsCn') }}
              <n-tag size="small" type="info" class="source-badge">
                {{ props.t('setting.network.badge.subscriptionProfile') }}
              </n-tag>
            </span>
          </template>
          <n-input
            v-model:value="singboxProfileForm.dnsCn"
            placeholder="h3://dns.alidns.com/dns-query"
          />
        </n-form-item>
      </div>

      <div class="setting-form-grid">
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.singboxProfile.dnsResolver') }}
              <n-tag size="small" type="info" class="source-badge">
                {{ props.t('setting.network.badge.subscriptionProfile') }}
              </n-tag>
            </span>
          </template>
          <n-input
            v-model:value="singboxProfileForm.dnsResolver"
            placeholder="114.114.114.114"
          />
        </n-form-item>
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.singboxProfile.dnsHijack') }}
              <n-tag size="small" type="info" class="source-badge">
                {{ props.t('setting.network.badge.subscriptionProfile') }}
              </n-tag>
            </span>
          </template>
          <n-switch v-model:value="singboxProfileForm.dnsHijack" />
        </n-form-item>
      </div>

      <div class="form-section-title">{{ props.t('setting.singboxProfile.fakeDnsTitle') }}</div>

      <div class="setting-form-grid">
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.singboxProfile.fakeDnsEnabled') }}
              <n-tag size="small" type="info" class="source-badge">
                {{ props.t('setting.network.badge.subscriptionProfile') }}
              </n-tag>
            </span>
          </template>
          <n-switch v-model:value="singboxProfileForm.fakeDnsEnabled" />
        </n-form-item>
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.singboxProfile.fakeDnsFilterMode') }}
              <n-tag size="small" type="info" class="source-badge">
                {{ props.t('setting.network.badge.subscriptionProfile') }}
              </n-tag>
            </span>
          </template>
          <n-select
            v-model:value="singboxProfileForm.fakeDnsFilterMode"
            :options="fakeDnsFilterOptions"
            :disabled="!singboxProfileForm.fakeDnsEnabled"
          />
        </n-form-item>
      </div>

      <div class="setting-form-grid">
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.singboxProfile.fakeDnsIpv4Range') }}
              <n-tag size="small" type="info" class="source-badge">
                {{ props.t('setting.network.badge.subscriptionProfile') }}
              </n-tag>
            </span>
          </template>
          <n-input
            v-model:value="singboxProfileForm.fakeDnsIpv4Range"
            placeholder="198.18.0.0/15"
            :disabled="!singboxProfileForm.fakeDnsEnabled"
          />
        </n-form-item>
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.singboxProfile.fakeDnsIpv6Range') }}
              <n-tag size="small" type="info" class="source-badge">
                {{ props.t('setting.network.badge.subscriptionProfile') }}
              </n-tag>
            </span>
          </template>
          <n-input
            v-model:value="singboxProfileForm.fakeDnsIpv6Range"
            placeholder="fc00::/18"
            :disabled="!singboxProfileForm.fakeDnsEnabled"
          />
        </n-form-item>
      </div>
    </n-form>

    <n-divider />
    <n-button
      type="primary"
      block
      :loading="savingSingboxProfile"
      @click="saveSingboxProfileSettings"
    >
      {{ props.t('setting.singboxProfile.save') }}
    </n-button>

    <h3 class="setting-section-title">{{ props.t('setting.network.sections.routing') }}</h3>

    <n-form label-placement="top" class="advanced-form">
      <div class="setting-form-grid">
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.singboxProfile.defaultOutbound') }}
              <n-tag size="small" type="info" class="source-badge">
                {{ props.t('setting.network.badge.subscriptionProfile') }}
              </n-tag>
            </span>
          </template>
          <n-select
            v-model:value="singboxProfileForm.defaultProxyOutbound"
            :options="defaultOutboundOptions"
          />
        </n-form-item>
        <n-form-item>
          <template #label>
            <span class="form-item-label">
              {{ props.t('setting.singboxProfile.downloadDetour') }}
              <n-tag size="small" type="info" class="source-badge">
                {{ props.t('setting.network.badge.subscriptionProfile') }}
              </n-tag>
            </span>
          </template>
          <n-select
            v-model:value="singboxProfileForm.downloadDetour"
            :options="downloadDetourOptions"
          />
        </n-form-item>
      </div>

      <div class="setting-toggles-grid">
        <div class="setting-toggle-item">
          <span class="setting-toggle-label">
            {{ props.t('setting.singboxProfile.blockAds') }}
            <n-tag size="small" type="info" class="source-badge">
              {{ props.t('setting.network.badge.subscriptionProfile') }}
            </n-tag>
          </span>
          <n-switch v-model:value="singboxProfileForm.blockAds" />
        </div>
        <div class="setting-toggle-item">
          <span class="setting-toggle-label">
            {{ props.t('setting.singboxProfile.enableAppGroups') }}
            <n-tag size="small" type="info" class="source-badge">
              {{ props.t('setting.network.badge.subscriptionProfile') }}
            </n-tag>
          </span>
          <n-switch v-model:value="singboxProfileForm.enableAppGroups" />
        </div>
      </div>

      <n-form-item>
        <template #label>
          <span class="form-item-label">
            {{ props.t('setting.singboxProfile.urltestUrl') }}
            <n-tag size="small" type="info" class="source-badge">
              {{ props.t('setting.network.badge.subscriptionProfile') }}
            </n-tag>
          </span>
        </template>
        <n-input
          v-model:value="singboxProfileForm.urltestUrl"
          placeholder="http://cp.cloudflare.com/generate_204"
        />
      </n-form-item>
    </n-form>

    <n-divider />
    <n-button
      type="primary"
      block
      :loading="savingSingboxProfile"
      @click="saveSingboxProfileSettings"
    >
      {{ props.t('setting.singboxProfile.save') }}
    </n-button>

    <h3 class="setting-section-title">{{ props.t('setting.extra.dashboardTitle') }}</h3>

    <div class="form-section-title">{{ props.t('setting.extra.proxyPrefs') }}</div>
    <div class="setting-form-grid">
      <n-form-item :label="props.t('setting.extra.proxyOrdering')">
        <n-select v-model:value="proxyStore.ordering" :options="proxyOrderingOptions" />
      </n-form-item>
      <n-form-item :label="props.t('setting.extra.proxyDisplay')">
        <n-select v-model:value="proxyStore.displayMode" :options="proxyDisplayOptions" />
      </n-form-item>
    </div>

    <div class="setting-toggles-grid">
      <div class="setting-toggle-item">
        <span class="setting-toggle-label">{{ props.t('setting.extra.proxyHideUnavailable') }}</span>
        <n-switch v-model:value="proxyStore.hideUnavailable" />
      </div>
      <div class="setting-toggle-item">
        <span class="setting-toggle-label">{{ props.t('setting.extra.proxyAutoClose') }}</span>
        <n-switch v-model:value="proxyStore.autoCloseConnections" />
      </div>
    </div>

    <div class="setting-form-grid">
      <n-form-item :label="props.t('setting.extra.latencyTimeout')">
        <n-input-number
          v-model:value="proxyStore.latencyTimeoutMs"
          :min="1000"
          :max="20000"
          :step="500"
        />
      </n-form-item>
      <n-form-item :label="props.t('setting.extra.latencyUrl')">
        <n-input v-model:value="proxyStore.latencyTestUrl" :placeholder="props.appStore.singboxUrltestUrl" />
      </n-form-item>
    </div>

    <div class="form-section-title">{{ props.t('setting.extra.logRetentionPrefs') }}</div>
    <div class="setting-form-grid">
      <n-form-item :label="props.t('setting.extra.logMaxRows')">
        <n-input-number
          v-model:value="logStore.maxLogs"
          :min="100"
          :max="5000"
          :step="100"
        />
      </n-form-item>
    </div>
    <div class="setting-hint">{{ props.t('setting.extra.logRetentionHint') }}</div>

    <n-modal
      v-model:show="showCurrentConfigModal"
      preset="dialog"
      :title="props.t('setting.network.currentConfigDialogTitle')"
      class="modern-modal config-viewer-modal"
      :style="{ width: '800px' }"
      :mask-closable="true"
    >
      <n-input
        v-model:value="currentConfigContent"
        type="textarea"
        :rows="20"
        class="code-input"
        readonly
        placeholder="JSON Config..."
      />
      <template #action>
        <n-space justify="end">
          <n-button @click="showCurrentConfigModal = false">{{ props.t('common.close') }}</n-button>
        </n-space>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  SettingsOutline,
  InformationCircleOutline,
  CodeOutline,
} from '@vicons/ionicons5'
import { useMessage } from 'naive-ui'
import type { useAppStore } from '@/stores'
import { useAdvancedSettingsForm } from '@/views/setting/useAdvancedSettingsForm'
import { useProxyStore } from '@/stores/kernel/ProxyStore'
import { useLogStore } from '@/stores/kernel/LogStore'
import { subscriptionService } from '@/services/subscription-service'

type LabeledOption = { label: string; value: string }
type AppStoreLike = ReturnType<typeof useAppStore>

const props = defineProps<{
  t: (key: string, params?: Record<string, string | number>) => string
  appStore: AppStoreLike
  tunStackOptions: LabeledOption[]
  usingOriginalConfig: boolean
  onIpVersionChange: (value: boolean) => void | Promise<void>
  onLanAccessChange: (value: boolean) => void | Promise<void>
  showPortSettings: () => void
}>()

const message = useMessage()
const proxyStore = useProxyStore()
const logStore = useLogStore()

const {
  savingAdvanced,
  proxyAdvancedForm,
  savingSingboxProfile,
  singboxProfileForm,
  defaultOutboundOptions,
  downloadDetourOptions,
  fakeDnsFilterOptions,
  saveProxyAdvancedSettings,
  saveSingboxProfileSettings,
} = useAdvancedSettingsForm({
  appStore: props.appStore,
  message,
  t: props.t,
})

const proxyOrderingOptions = computed(() => {
  return [
    { label: props.t('setting.extra.ordering.natural'), value: 'natural' },
    { label: props.t('setting.extra.ordering.latency'), value: 'latency' },
    { label: props.t('setting.extra.ordering.name'), value: 'name' },
  ]
})

const proxyDisplayOptions = computed(() => {
  return [
    { label: props.t('setting.extra.display.card'), value: 'card' },
    { label: props.t('setting.extra.display.list'), value: 'list' },
  ]
})

const showCurrentConfigModal = ref(false)
const currentConfigContent = ref('')
const isCurrentConfigLoading = ref(false)

const openCurrentConfigViewer = async () => {
  try {
    isCurrentConfigLoading.value = true
    const config = await subscriptionService.getCurrentConfig()
    currentConfigContent.value =
      typeof config === 'string' ? config : JSON.stringify(config, null, 2)
    showCurrentConfigModal.value = true
  } catch (error) {
    message.error(props.t('setting.network.currentConfigLoadFailed'))
  } finally {
    isCurrentConfigLoading.value = false
  }
}
</script>

<style scoped>
.config-viewer-row {
  background: var(--bg-secondary);
  border: 1px solid var(--panel-border);
  border-radius: 10px;
  padding: 14px 16px;
  margin-bottom: 8px;
}

.subscription-hint {
  margin-bottom: 12px;
}

.form-item-label {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.setting-label {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.source-badge {
  font-size: 10px;
  line-height: 1;
}

.form-section-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.06em;
  margin: 16px 0 8px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-color);
}

.form-section-title:first-child {
  margin-top: 8px;
}

.setting-hint {
  font-size: 12px;
  color: var(--text-tertiary);
  line-height: 1.5;
  margin: 4px 0 0;
}
</style>
