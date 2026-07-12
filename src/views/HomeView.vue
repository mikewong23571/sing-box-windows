<template>
  <div class="page-shell">
    <n-flex vertical size="large">
      <!-- Hero Section -->
      <n-card class="page-hero" :class="statusClass">
        <n-flex justify="space-between" align="center">
          <n-flex align="center" :size="16">
            <n-icon
              size="48"
              :class="{ 'pulse-active': kernelRunning }"
              :type="kernelRunning ? 'primary' : 'default'"
            >
              <PlanetOutline v-if="kernelRunning" />
              <CloudOfflineOutline v-else />
            </n-icon>
            <n-flex vertical :size="0">
              <n-text style="font-size: 24px; font-weight: bold">{{ statusTitle }}</n-text>
              <n-space align="center" :size="8">
                <span class="status-dot" :class="{ running: kernelRunning }"></span>
                <n-text depth="3">{{ appStatusLabel }}</n-text>
                <n-text depth="3">· {{ desiredStatusLabel }}</n-text>
              </n-space>
            </n-flex>
          </n-flex>

          <n-space>
            <n-button
              v-if="!kernelRunning"
              type="primary"
              size="large"
              round
              :loading="kernelBusy"
              @click="startKernel"
            >
              <template #icon>
                <n-icon><PlayOutline /></n-icon>
              </template>
              {{ t('home.start') }}
            </n-button>

            <n-button
              v-else
              type="error"
              size="large"
              round
              secondary
              :loading="kernelBusy"
              @click="stopKernel"
            >
              <template #icon>
                <n-icon><StopOutline /></n-icon>
              </template>
              {{ t('home.stop') }}
            </n-button>

            <n-button
              v-if="kernelRunning"
              type="primary"
              size="large"
              round
              secondary
              :loading="kernelBusy"
              @click="restartKernel"
            >
              <template #icon>
                <n-icon><RefreshOutline /></n-icon>
              </template>
              {{ t('home.restart') }}
            </n-button>

            <n-tooltip v-if="isWindowsPlatform && !isAdmin" trigger="hover">
              <template #trigger>
                <n-button size="large" round secondary type="warning" @click="restartAsAdmin">
                  <template #icon>
                    <n-icon><ShieldCheckmarkOutline /></n-icon>
                  </template>
                </n-button>
              </template>
              {{ t('home.restartAsAdmin') }}
            </n-tooltip>
          </n-space>
        </n-flex>

        <!-- Quick Stats in Hero (Only visible when running) -->
        <n-grid
          v-if="kernelRunning"
          responsive="screen"
          cols="1 s:2 m:3"
          :x-gap="16"
          :y-gap="16"
          style="margin-top: 24px"
        >
          <n-grid-item>
            <n-card size="small">
              <n-statistic :label="t('home.traffic.up') + ' / ' + t('home.traffic.down')">
                <template #prefix>
                  <n-icon color="#2080f0"><SwapVerticalOutline /></n-icon>
                </template>
                <n-text class="speed-statistic-value">
                  {{ formatSpeed(trafficStore.traffic.up) }} /
                  {{ formatSpeed(trafficStore.traffic.down) }}
                </n-text>
              </n-statistic>
            </n-card>
          </n-grid-item>

          <n-grid-item>
            <n-card size="small">
              <n-statistic
                :label="t('nav.connections')"
                :value="connectionStore.connections.length"
              >
                <template #prefix>
                  <n-icon color="#d03050"><GitNetworkOutline /></n-icon>
                </template>
              </n-statistic>
            </n-card>
          </n-grid-item>

          <n-grid-item>
            <n-card size="small">
              <n-statistic :label="t('proxy.title')">
                <template #prefix>
                  <n-icon color="#8a2be2"><GlobeOutline /></n-icon>
                </template>
                <n-text style="font-size: 16px">
                  {{
                    currentNodeProxyMode === 'global'
                      ? t('home.nodeMode.global')
                      : t('home.nodeMode.rule')
                  }}
                </n-text>
              </n-statistic>
            </n-card>
          </n-grid-item>
        </n-grid>
      </n-card>

      <!-- Error Alerts -->
      <n-alert
        v-if="kernelStore.startupDiagnosis"
        type="error"
        :title="kernelStore.startupDiagnosis.message"
      >
        <n-flex vertical size="small">
          <n-space>
            <n-tag size="small" type="error">{{ kernelStore.startupDiagnosis.stage }}</n-tag>
            <n-tag size="small">{{ kernelStore.startupDiagnosis.kind }}</n-tag>
          </n-space>
          <n-text>{{ kernelStore.startupDiagnosis.detail }}</n-text>
          <ul
            v-if="kernelStore.startupDiagnosis.suggested_actions?.length"
            style="margin: 0; padding-left: 20px"
          >
            <li v-for="action in kernelStore.startupDiagnosis.suggested_actions" :key="action">
              {{ action }}
            </li>
          </ul>
        </n-flex>
      </n-alert>

      <!-- Main Content Grid -->
      <n-grid responsive="screen" cols="1 l:3" :x-gap="24" :y-gap="24">
        <!-- Traffic Chart -->
        <n-grid-item span="1 l:2">
          <n-card
            :title="
              t('home.traffic.total') +
              ': ' +
              formatBytes(trafficStore.traffic.totalUp + trafficStore.traffic.totalDown)
            "
          >
            <template #header-extra>
              <div
                style="
                  display: flex;
                  gap: 16px;
                  font-size: 12px;
                  font-weight: 600;
                  align-items: center;
                  color: var(--text-primary);
                "
              >
                <div style="display: flex; align-items: center; gap: 6px">
                  <div
                    style="
                      width: 8px;
                      height: 8px;
                      border-radius: 50%;
                      background-color: var(--success-color);
                      box-shadow: 0 0 8px var(--success-color);
                    "
                  ></div>
                  <span>{{ t('home.traffic.uploadSpeed') }}</span>
                </div>
                <div style="display: flex; align-items: center; gap: 6px">
                  <div
                    style="
                      width: 8px;
                      height: 8px;
                      border-radius: 50%;
                      background-color: var(--primary-color);
                      box-shadow: 0 0 8px var(--primary-color);
                    "
                  ></div>
                  <span>{{ t('home.traffic.downloadSpeed') }}</span>
                </div>
              </div>
            </template>
            <TrafficChart
              :upload-speed="trafficStore.traffic.up"
              :download-speed="trafficStore.traffic.down"
            />
          </n-card>
        </n-grid-item>

        <!-- Controls & Toggles Sidebar -->
        <n-grid-item span="1 l:1">
          <n-flex vertical size="large">
            <!-- Flow Mode Panel -->
            <n-card>
              <template #header>
                <n-flex justify="space-between" align="center">
                  <span>{{ t('home.proxyHeader.flowMode') }}</span>
                  <n-button
                    size="small"
                    secondary
                    round
                    type="primary"
                    @click="showPortModal = true"
                  >
                    <template #icon
                      ><n-icon><SettingsOutline /></n-icon
                    ></template>
                    {{ t('common.edit') }}
                  </n-button>
                </n-flex>
              </template>

              <n-flex vertical size="large">
                <n-flex justify="space-between" align="center" :wrap="false">
                  <n-flex align="center" :size="12" :wrap="false">
                    <n-icon size="24"><DesktopOutline /></n-icon>
                    <n-flex vertical :size="0">
                      <n-text strong>{{ t('home.proxyMode.system') }}</n-text>
                      <n-text depth="3" style="font-size: 12px; min-height: 18px">{{
                        proxyAddress || t('common.disabled')
                      }}</n-text>
                    </n-flex>
                  </n-flex>
                  <n-switch
                    :value="systemProxyEnabled"
                    :disabled="modeSwitchPending"
                    @update:value="(v: boolean) => toggleSystemProxy(v)"
                  />
                </n-flex>

                <n-flex justify="space-between" align="center" :wrap="false">
                  <n-flex align="center" :size="12" :wrap="false">
                    <n-icon size="24"><FlashOutline /></n-icon>
                    <n-flex vertical :size="0">
                      <n-text strong>{{ t('home.proxyMode.tun') }}</n-text>
                      <n-text depth="3" style="font-size: 12px; min-height: 18px">{{
                        t('home.proxyMode.tunTip')
                      }}</n-text>
                    </n-flex>
                  </n-flex>
                  <n-switch
                    :value="tunProxyEnabled"
                    :disabled="modeSwitchPending"
                    @update:value="(v: boolean) => toggleTunProxy(v)"
                  />
                </n-flex>
              </n-flex>
            </n-card>

            <!-- Node Mode Panel -->
            <n-card :title="t('home.proxyHeader.nodeMode')">
              <n-tabs
                type="segment"
                :value="currentNodeProxyMode"
                @update:value="handleNodeProxyModeChange"
              >
                <n-tab v-for="mode in nodeProxyModes" :key="mode.value" :name="mode.value">
                  <div style="display: flex; align-items: center; gap: 6px">
                    <n-icon :size="18" style="display: flex"><component :is="mode.icon" /></n-icon>
                    <span style="line-height: 1">{{ t(mode.nameKey) }}</span>
                  </div>
                </n-tab>
              </n-tabs>
            </n-card>
          </n-flex>
        </n-grid-item>
      </n-grid>
    </n-flex>

    <!-- Modals -->
    <PortSettingsDialog v-model:show="showPortModal" />
  </div>
</template>

<script lang="ts" setup>
import { computed, ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDialog, useMessage, type DialogReactive } from 'naive-ui'
import {
  PlayOutline,
  StopOutline,
  RefreshOutline,
  ShieldCheckmarkOutline,
  GlobeOutline,
  FlashOutline,
  RadioOutline,
  PlanetOutline,
  CloudOfflineOutline,
  SwapVerticalOutline,
  GitNetworkOutline,
  SettingsOutline,
  DesktopOutline,
} from '@vicons/ionicons5'
import { useAppStore } from '@/stores'
import { useKernelStore } from '@/stores/kernel/KernelStore'
import { useTrafficStore } from '@/stores/kernel/TrafficStore'
import { useConnectionStore } from '@/stores/kernel/ConnectionStore'
import { useProxyStore } from '@/stores/kernel/ProxyStore'
import { kernelService } from '@/services/kernel-service'
import { proxyService } from '@/services/proxy-service'
import { sudoService } from '@/services/sudo-service'
import { systemService } from '@/services/system-service'
import PortSettingsDialog from '@/components/common/PortSettingsDialog.vue'
import TrafficChart from '@/components/layout/TrafficChart.vue'
import { useKernelStatus } from '@/composables/useKernelStatus'
import { useSudoStore } from '@/stores'
import { formatBytes, formatSpeed } from '@/utils'

defineOptions({
  name: 'HomeView',
})

const { t } = useI18n()
const message = useMessage()
const dialog = useDialog()

const appStore = useAppStore()
const kernelStore = useKernelStore()
const trafficStore = useTrafficStore()
const connectionStore = useConnectionStore()
const proxyStore = useProxyStore()
const sudoStore = useSudoStore()

const {
  statusClass,
  statusState,
  isRunning: kernelRunning,
  isLoading: kernelLoading,
} = useKernelStatus(kernelStore)
const isAdmin = ref(false)
const platform = ref<'windows' | 'linux' | 'macos' | 'unknown'>('unknown')
const currentNodeProxyMode = ref('rule')
const modeSwitchPending = ref(false)
const showPortModal = ref(false)

const isWindowsPlatform = computed(() => platform.value === 'windows')
const isUnixPlatform = computed(() => platform.value === 'linux' || platform.value === 'macos')
const kernelBusy = computed(
  () => kernelLoading.value || statusState.value === 'starting' || statusState.value === 'stopping',
)

const statusTitle = computed(() => {
  switch (statusState.value) {
    case 'starting':
      return t('status.starting')
    case 'stopping':
      return t('status.stopping')
    case 'running':
      return t('status.running')
    case 'disconnected':
      return t('status.disconnected')
    case 'failed':
      return t('status.failed')
    case 'crashed':
      return t('status.crashed')
    default:
      return t('status.stopped')
  }
})

const appStatusLabel = computed(() => {
  switch (statusState.value) {
    case 'starting':
      return t('status.starting')
    case 'stopping':
      return t('status.stopping')
    case 'running':
      return t('status.running')
    case 'disconnected':
      return t('status.disconnected')
    case 'failed':
      return t('status.failed')
    case 'crashed':
      return t('status.crashed')
    default:
      return t('status.stopped')
  }
})

const desiredStatusLabel = computed(() =>
  kernelStore.status.desired_state === 'running'
    ? t('status.desiredRunning')
    : t('status.desiredStopped'),
)

const systemProxyEnabled = computed(() => appStore.systemProxyEnabled)
const tunProxyEnabled = computed(() => appStore.tunEnabled)
const proxyAddress = computed(() => `127.0.0.1:${appStore.proxyPort}`)

const nodeProxyModes = [
  {
    value: 'global',
    nameKey: 'home.nodeMode.global',
    tipKey: 'home.nodeMode.globalTip',
    icon: GlobeOutline,
  },
  {
    value: 'rule',
    nameKey: 'home.nodeMode.rule',
    tipKey: 'home.nodeMode.ruleTip',
    icon: RadioOutline,
  },
]

const getKernelFailureText = (fallback: string) =>
  kernelStore.startupDiagnosisSummary || kernelStore.lastError || fallback

const syncCurrentNodeProxyMode = async () => {
  try {
    const mode = await proxyService.getCurrentProxyMode()
    if (mode === 'global' || mode === 'rule') {
      currentNodeProxyMode.value = mode
    }
  } catch {}
}

const toggleSystemProxy = async (value: boolean) => {
  if (modeSwitchPending.value) return

  try {
    modeSwitchPending.value = true
    await appStore.toggleSystemProxy(value)

    const success = await kernelStore.applyProxySettings()
    if (success) {
      message.success(t('notification.proxyModeChanged'))
    } else {
      message.error(getKernelFailureText(t('notification.proxyModeChangeFailed')))
    }
  } catch (error) {
    message.error(t('notification.proxyModeChangeFailed'))
  } finally {
    modeSwitchPending.value = false
  }
}

const confirmTunSwitch = () => {
  let dialogReactive: DialogReactive | null = null
  let resolved = false

  return new Promise<boolean>((resolve) => {
    const finish = (result: boolean) => {
      if (resolved) return
      resolved = true
      resolve(result)
    }

    const handlePositiveClick = async () => {
      modeSwitchPending.value = true
      if (dialogReactive) dialogReactive.loading = true

      try {
        const success = await prepareTunModeWithAdminRestart()
        finish(success)
        return success
      } finally {
        modeSwitchPending.value = false
        if (dialogReactive) dialogReactive.loading = false
      }
    }

    dialogReactive = dialog.warning({
      title: t('home.tunConfirm.title'),
      content: t('home.tunConfirm.description'),
      positiveText: t('home.tunConfirm.confirm'),
      negativeText: t('common.cancel'),
      maskClosable: false,
      onPositiveClick: handlePositiveClick,
      onNegativeClick: () => finish(false),
      onClose: () => finish(false),
    })
  })
}

const parseSudoCode = (raw: unknown) => {
  const msg = raw instanceof Error ? raw.message : String(raw || '')
  if (msg.includes('SUDO_PASSWORD_REQUIRED')) return 'required'
  if (msg.includes('SUDO_PASSWORD_INVALID')) return 'invalid'
  return null
}

const getErrorMessage = (error: unknown) => {
  if (error instanceof Error) {
    return error.message
  }
  return String(error || '')
}

const enableTunWithKernelRestart = async (options?: { allowSudoRetry?: boolean }) => {
  try {
    modeSwitchPending.value = true
    await appStore.toggleTun(true)

    const applied = await kernelStore.applyProxySettings()
    if (!applied) {
      await appStore.toggleTun(false)
      message.error(t('notification.proxyModeChangeFailed'))
      return false
    }

    const success = await kernelStore.restartKernel()
    if (success) {
      message.success(t('notification.proxyModeChanged'))
      return true
    }

    await appStore.toggleTun(false)

    if (isUnixPlatform.value) {
      const code = parseSudoCode(getKernelFailureText(''))
      if (code === 'required' || code === 'invalid') {
        message.error(
          code === 'invalid' ? t('home.sudoPassword.invalid') : t('home.sudoPassword.required'),
        )

        const allowRetry = options?.allowSudoRetry ?? true
        const ok = await sudoStore.requestPassword()
        if (ok && allowRetry) {
          return enableTunWithKernelRestart({ allowSudoRetry: false })
        }
        return false
      }
    }

    message.error(t('home.restartFailed'))
    return false
  } catch (error) {
    await appStore.toggleTun(false)
    message.error(t('notification.proxyModeChangeFailed'))
    return false
  } finally {
    modeSwitchPending.value = false
  }
}

const toggleTunProxy = async (value: boolean) => {
  if (modeSwitchPending.value) return

  // 停止状态下只保存配置。启用 TUN 所需的提权在用户显式启动内核时处理。
  if (!kernelRunning.value) {
    try {
      modeSwitchPending.value = true
      await appStore.toggleTun(value)
      const applied = await kernelStore.applyProxySettings()
      if (applied) {
        message.success(t('common.saveSuccess'))
      } else {
        await appStore.toggleTun(!value)
        message.error(getKernelFailureText(t('notification.proxyModeChangeFailed')))
      }
    } finally {
      modeSwitchPending.value = false
    }
    return
  }

  if (value) {
    if (isWindowsPlatform.value) {
      await checkAdmin()

      if (isAdmin.value) {
        await enableTunWithKernelRestart()
      } else {
        await confirmTunSwitch()
      }
    } else if (isUnixPlatform.value) {
      const status = await sudoService.getStatus()
      if (!status.supported) {
        message.error(t('home.sudoPassword.unsupported'))
        return
      }
      if (!status.has_saved) {
        const ok = await sudoStore.requestPassword()
        if (!ok) return
      }
      await enableTunWithKernelRestart()
    } else {
      message.error(t('home.sudoPassword.unsupported'))
    }
  } else {
    try {
      modeSwitchPending.value = true
      await appStore.toggleTun(false)

      const applied = await kernelStore.applyProxySettings()
      if (!applied) {
        await appStore.toggleTun(true)
        message.error(t('notification.proxyModeChangeFailed'))
        return
      }

      const success = await kernelStore.restartKernel()
      if (success) {
        message.success(t('notification.proxyModeChanged'))
      } else {
        await appStore.toggleTun(true)
        message.error(t('home.restartFailed'))
      }
    } catch (error) {
      await appStore.toggleTun(true)
      message.error(t('notification.proxyModeChangeFailed'))
    } finally {
      modeSwitchPending.value = false
    }
  }
}

const restartKernel = async () => {
  if (kernelBusy.value) return

  try {
    const result = await kernelStore.restartKernel()
    if (result) {
      message.success(t('home.restartSuccess'))
    } else {
      message.error(getKernelFailureText(t('home.restartFailed')))
    }
  } catch (error) {
    message.error(t('home.restartFailed'))
  }
}

const startKernel = async () => {
  if (kernelBusy.value) return

  try {
    const result = await kernelStore.startKernel()
    if (result) {
      message.success(t('home.startSuccess'))
    } else {
      message.error(getKernelFailureText(t('home.startFailed')))
    }
  } catch (error) {
    message.error(t('home.startFailed'))
  }
}

const stopKernel = async () => {
  if (kernelBusy.value) return

  try {
    const result = await kernelStore.stopKernel()
    if (result) {
      message.success(t('home.stopSuccess'))
    } else {
      message.error(getKernelFailureText(t('home.stopFailed')))
    }
  } catch (error) {
    message.error(t('home.stopFailed'))
  }
}

const restartAsAdmin = async () => {
  try {
    await requestRestartAsAdmin()
  } catch (error) {
    const details = getErrorMessage(error)
    message.error(details ? `${t('home.restartFailed')}：${details}` : t('home.restartFailed'))
  }
}

const requestRestartAsAdmin = async () => {
  await systemService.restartAsAdmin()
}

const prepareTunModeWithAdminRestart = async () => {
  try {
    await appStore.toggleTun(true)
    const applied = await kernelStore.applyProxySettings()
    if (!applied) {
      await appStore.toggleTun(false)
      message.error(t('notification.proxyModeChangeFailed'))
      return false
    }
    await appStore.saveToBackend()

    if (kernelRunning.value) {
      await kernelStore.stopKernel()
    }

    await requestRestartAsAdmin()
    return true
  } catch (error) {
    await appStore.toggleTun(false)
    const details = getErrorMessage(error)
    message.error(details ? `${t('home.restartFailed')}：${details}` : t('home.restartFailed'))
    return false
  }
}

const handleNodeProxyModeChange = async (mode: string) => {
  if (currentNodeProxyMode.value === mode) return

  try {
    const result = await kernelService.switchNodeProxyMode(mode as 'global' | 'rule')
    await syncCurrentNodeProxyMode()

    if (result.includes('重启后生效')) {
      message.warning(result)
      return
    }

    message.success(t('home.nodeModeChangeSuccess'))
  } catch (error) {
    message.error(t('home.nodeModeChangeFailed'))
  }
}

const checkAdmin = async () => {
  try {
    isAdmin.value = await systemService.checkAdmin()
  } catch (error) {
    isAdmin.value = false
  }
}

onMounted(async () => {
  try {
    const raw = await systemService.getPlatformInfo()
    platform.value = raw === 'windows' || raw === 'linux' || raw === 'macos' ? raw : 'unknown'
  } catch (error) {
    platform.value = 'unknown'
  }
  checkAdmin()
  await kernelStore.initializeStore()
  await proxyStore.fetchProxies().catch(() => undefined)
  await syncCurrentNodeProxyMode()
})
</script>

<style scoped>
/* State-specific Hero Styling */
.page-hero.running .hero-icon {
  background: linear-gradient(135deg, #10b981, #34d399);
  box-shadow: 0 12px 24px -8px rgba(16, 185, 129, 0.5);
}

.page-hero.pending .hero-icon,
.page-hero.disconnected .hero-icon {
  background: linear-gradient(135deg, #f59e0b, #fbbf24);
  box-shadow: 0 12px 24px -8px rgba(245, 158, 11, 0.5);
}

.page-hero.stopped .hero-icon,
.page-hero.failed .hero-icon {
  background: linear-gradient(135deg, #ef4444, #f87171);
  box-shadow: 0 12px 24px -8px rgba(239, 68, 68, 0.5);
}

.page-hero.crashed .hero-icon {
  background: linear-gradient(135deg, #f97316, #fb923c);
  box-shadow: 0 12px 24px -8px rgba(249, 115, 22, 0.5);
}

/* Animations */
@keyframes pulse-icon {
  0% {
    transform: scale(1);
    box-shadow: 0 0 0 0 rgba(16, 185, 129, 0.4);
  }
  70% {
    transform: scale(1.05);
    box-shadow: 0 0 0 15px rgba(16, 185, 129, 0);
  }
  100% {
    transform: scale(1);
    box-shadow: 0 0 0 0 rgba(16, 185, 129, 0);
  }
}

.pulse-active {
  animation: pulse-icon 3s infinite ease-in-out;
}

/* Status Indicator */
.status-indicator {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-weight: 500;
  padding: 4px 12px;
  background: var(--bg-tertiary);
  border-radius: var(--radius-full);
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-tertiary);
}

.running .status-dot {
  background: var(--success-color);
  box-shadow: 0 0 8px var(--success-color);
}
.pending .status-dot,
.disconnected .status-dot {
  background: var(--warning-color);
  box-shadow: 0 0 8px var(--warning-color);
}
.stopped .status-dot,
.failed .status-dot,
.crashed .status-dot {
  background: var(--error-color);
  box-shadow: 0 0 8px var(--error-color);
}

.speed-separator {
  color: var(--text-tertiary);
  margin: 0 4px;
  font-weight: 400;
}

.speed-statistic-value {
  display: inline-block;
  font-size: 16px;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 100%;
}

/* Main Grid Layout */
.main-grid {
  display: grid;
  grid-template-columns: 1fr 340px;
  gap: 24px;
  min-height: 0;
}

.chart-panel {
  display: flex;
  flex-direction: column;
  min-height: 320px;
}

.chart-wrapper {
  flex: 1;
  min-height: 200px;
  margin-top: 16px;
  position: relative;
}

.side-panels {
  display: flex;
  flex-direction: column;
  gap: 20px;
}

/* Panel Header Utilities */
.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 20px;
}

.panel-title {
  font-size: 13px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--text-secondary);
}

/* List Controls (System Proxy / Tun) */
.control-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.control-item {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 16px;
  border-radius: var(--radius-md);
  background: var(--bg-tertiary);
  border: 1px solid transparent;
  transition: all var(--transition-normal);
}

.control-item:hover {
  background: var(--bg-secondary);
  border-color: var(--border-color);
}

.control-item.active {
  background: var(--bg-primary);
  border-color: var(--primary-color);
  box-shadow: 0 4px 12px -2px rgba(99, 102, 241, 0.1);
}

.control-icon {
  width: 40px;
  height: 40px;
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  transition: all var(--transition-normal);
}

.control-item.active .control-icon {
  background: linear-gradient(135deg, var(--primary-color), #818cf8);
  color: white;
  box-shadow: 0 4px 12px rgba(99, 102, 241, 0.4);
}

.control-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.control-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.control-desc {
  font-size: 12px;
  color: var(--text-tertiary);
  font-family: var(--font-mono);
}

/* Segmented Control (Node Mode) */
.segmented-control {
  display: flex;
  background: var(--bg-tertiary);
  padding: 4px;
  border-radius: var(--radius-md);
  gap: 4px;
}

.segment {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 10px 16px;
  border-radius: var(--radius-sm);
  font-size: 14px;
  font-weight: 600;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all var(--transition-fast);
}

.segment:hover {
  color: var(--text-primary);
}

.segment.active {
  background: var(--bg-primary);
  color: var(--primary-color);
  box-shadow: 0 2px 8px -2px rgba(0, 0, 0, 0.1);
}

/* Alert styling */
.diagnosis-alert {
  border-radius: var(--radius-lg);
}

.diagnosis-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.diagnosis-meta {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.diagnosis-detail {
  white-space: pre-wrap;
  word-break: break-word;
}

.diagnosis-actions {
  margin: 0;
  padding-left: 20px;
}

@media (max-width: 1024px) {
  .main-grid {
    grid-template-columns: 1fr;
  }
}
</style>
