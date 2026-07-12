<template>
  <div class="page-shell" style="padding: 24px;">
    <PageHeader :title="t('setting.title')" :subtitle="t('setting.subtitle')" />
    <n-flex vertical size="large" style="flex: 1; min-height: 0;">
      <n-tabs type="line" placement="left" animated v-model:value="activeTab" style="height: 100%;">
      <n-tab-pane
        v-for="item in navItems"
        :key="item.key"
        :name="item.key"
      >
        <template #tab>
          <div style="display: flex; align-items: center; gap: 6px;">
            <n-icon :size="18" style="display: flex;"><component :is="item.icon" /></n-icon>
            <span style="line-height: 1;">{{ item.label }}</span>
            <n-badge v-if="item.key === 'kernel' && hasNewVersion" dot type="warning" />
            <n-badge v-if="item.key === 'maintenance' && updateStore.hasUpdate" dot type="warning" />
          </div>
        </template>
        <div class="settings-content" style="max-width: 720px; margin: 0 auto; padding: 0 24px; height: 100%; overflow-y: auto;">
          <SettingsGeneralTab
            v-if="item.key === 'general'"
            :t="t"
            :locale-store="localeStore"
            :auto-start="autoStart"
            :auto-start-kernel="appStore.autoStartKernel"
            :auto-hide-to-tray-on-autostart="autoHideToTrayOnAutostart"
            :tray-close-behavior="trayCloseBehavior"
            :language-options="languageOptions"
            :tray-close-behavior-options="trayCloseBehaviorOptions"
            :on-auto-start-change="onAutoStartChange"
            :on-auto-start-kernel-change="onAutoStartKernelChange"
            :on-auto-hide-to-tray-on-autostart-change="onAutoHideToTrayOnAutostartChange"
            :on-tray-close-behavior-change="onTrayCloseBehaviorChange"
            :on-change-language="handleChangeLanguage"
          />

          <SettingsAppearanceTab
            v-if="item.key === 'appearance'"
            :t="t"
            :theme-store="themeStore"
            :accent-presets="accentPresets"
            :on-theme-mode-change="onThemeModeChange"
            :on-accent-change="onAccentChange"
            :select-accent-preset="selectAccentPreset"
            :on-compact-mode-change="onCompactModeChange"
          />

          <SettingsKernelTab
            v-if="item.key === 'kernel'"
            :t="t"
            :kernel-store="kernelStore"
            :selected-kernel-version="selectedKernelVersion"
            :kernel-version-options="kernelVersionOptions"
            :has-new-version="hasNewVersion"
            :kernel-latest-version="kernelLatestVersion"
            :downloading="downloading"
            :loading="loading"
            :download-progress="downloadProgress"
            :download-message="downloadMessage"
            :on-selected-kernel-version-change="onSelectedKernelVersionChange"
            :download-the-kernel="downloadTheKernel"
            :show-manual-download-modal="showManualDownloadModal"
            :check-manual-install="checkManualInstall"
            :format-version="formatVersion"
          />

          <SettingsNetworkTab
            v-if="item.key === 'network'"
            :t="t"
            :app-store="appStore"
            :tun-stack-options="tunStackOptions"
            :using-original-config="usingOriginalConfig"
            :on-ip-version-change="onIpVersionChange"
            :on-lan-access-change="onLanAccessChange"
            :show-port-settings="showPortSettings"
          />

          <SettingsMaintenanceTab
            v-if="item.key === 'maintenance'"
            :t="t"
            :update-store="updateStore"
            :checking-update="checkingUpdate"
            :update-status="updateStatus"
            :update-progress="updateProgress"
            :update-message="updateMessage"
            :is-updating="isUpdating"
            :show-update-progress="showUpdateProgress"
            :update-channel-options="updateChannelOptions"
            :backup-exporting="backupExporting"
            :backup-validating="backupValidating"
            :backup-restoring="backupRestoring"
            :backup-busy="backupBusy"
            :backup-preview="backupPreview"
            :handle-update-now="handleUpdateNow"
            :handle-check-update="handleCheckUpdate"
            :on-auto-check-update-change="onAutoCheckUpdateChange"
            :on-update-channel-change="onUpdateChannelChange"
            :handle-export-backup="handleExportBackup"
            :handle-validate-backup="handleValidateBackup"
            :handle-restore-backup="handleRestoreBackup"
          />

          <SettingsAboutTab
            v-if="item.key === 'about'"
            :t="t"
            :update-store="updateStore"
            :kernel-store="kernelStore"
            :platform-info="platformInfo"
            :format-version="formatVersion"
          />
        </div>
      </n-tab-pane>
    </n-tabs>

    <PortSettingsDialog v-model:show="showPortModal" />

    <n-modal
      v-model:show="showManualImportModal"
      preset="dialog"
      :title="t('setting.kernel.manualImportTitle')"
      class="modern-modal"
      :style="{ width: '520px' }"
    >
      <n-flex vertical size="large">
        <n-text depth="3">{{ t('setting.kernel.manualImportDesc') }}</n-text>
        <n-upload
          multiple
          directory-dnd
          :max="1"
          @before-upload="() => false"
        >
          <n-upload-dragger>
            <div style="margin-bottom: 12px">
              <n-icon size="48" :depth="3">
                <DownloadOutline />
              </n-icon>
            </div>
            <n-text style="font-size: 16px">
              {{ t('setting.kernel.dropHint') }}
            </n-text>
            <n-p depth="3" style="margin: 8px 0 0 0">
              {{ t('setting.kernel.dropSubHint') }}
            </n-p>
          </n-upload-dragger>
        </n-upload>

        <n-alert v-if="manualKernelPath" type="info">
          <n-flex vertical size="small">
            <n-text depth="3" style="font-size: 12px">{{ t('setting.kernel.selectedFile') }}</n-text>
            <n-ellipsis style="max-width: 100%">{{ manualKernelPath }}</n-ellipsis>
          </n-flex>
        </n-alert>
      </n-flex>
      <template #action>
        <n-flex justify="space-between" style="width: 100%">
          <n-button @click="pickManualKernelFile" :disabled="manualImporting">
            {{ t('setting.kernel.chooseFile') }}
          </n-button>
          <n-space>
            <n-button @click="showManualImportModal = false" :disabled="manualImporting">
              {{ t('common.cancel') }}
            </n-button>
            <n-button type="primary" @click="importManualKernel" :loading="manualImporting">
              {{ t('setting.kernel.importNow') }}
            </n-button>
          </n-space>
        </n-flex>
      </template>
    </n-modal>
    </n-flex>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, type Component } from 'vue'
import { Window } from '@tauri-apps/api/window'
import { useDialog, useMessage } from 'naive-ui'
import {
  DownloadOutline,
  InformationCircleOutline,
  ColorPaletteOutline,
  HardwareChipOutline,
  GlobeOutline,
  CloudDownloadOutline,
  SettingsOutline,
} from '@vicons/ionicons5'
import { useI18n } from 'vue-i18n'
import {
  useAppStore,
  useKernelStore,
  useUpdateStore,
  useLocaleStore,
  useThemeStore,
} from '@/stores'
import { useSubStore } from '@/stores/subscription/SubStore'
import type { Locale } from '@/stores/app/LocaleStore'
import type { ThemeMode } from '@/stores/app/ThemeStore'
import type { TrayCloseBehavior } from '@/stores/app/AppStore'
import type { UpdateChannel } from '@/stores/app/UpdateStore'
import { systemService, type BackupImportResult } from '@/services/system-service'
import { supportedLocales } from '@/locales'
import PortSettingsDialog from '@/components/common/PortSettingsDialog.vue'
import { ACCENT_PRESETS, TUN_STACK_OPTIONS } from '@/views/setting/setting-options'
import { useKernelDownload } from '@/views/setting/useKernelDownload'
import { useUpdateProgressListener } from '@/views/setting/useUpdateProgressListener'
import PageHeader from '@/components/common/PageHeader.vue'
import SettingsGeneralTab from '@/views/setting/components/SettingsGeneralTab.vue'
import SettingsAppearanceTab from '@/views/setting/components/SettingsAppearanceTab.vue'
import SettingsKernelTab from '@/views/setting/components/SettingsKernelTab.vue'
import SettingsNetworkTab from '@/views/setting/components/SettingsNetworkTab.vue'
import SettingsMaintenanceTab from '@/views/setting/components/SettingsMaintenanceTab.vue'
import SettingsAboutTab from '@/views/setting/components/SettingsAboutTab.vue'
import '@/views/setting/setting-shared.css'

const message = useMessage()
const dialog = useDialog()
const { t } = useI18n()
const appStore = useAppStore()
const kernelStore = useKernelStore()
const updateStore = useUpdateStore()
const localeStore = useLocaleStore()
const themeStore = useThemeStore()
const subStore = useSubStore()

type SettingTabKey = 'general' | 'appearance' | 'kernel' | 'network' | 'maintenance' | 'about'

const activeTab = ref<SettingTabKey>('general')
const selectedKernelVersion = ref<string | undefined>(undefined)
const platformInfo = ref<{ os: string; arch: string; display_name: string } | null>(null)

const autoStart = ref(false)
const autoHideToTrayOnAutostart = ref(true)
const trayCloseBehavior = ref<TrayCloseBehavior>('hide')
const checkingUpdate = ref(false)
const backupExporting = ref(false)
const backupValidating = ref(false)
const backupRestoring = ref(false)
const backupPreview = ref<BackupImportResult | null>(null)
const accentPresets = ACCENT_PRESETS

const showPortModal = ref(false)
const showManualImportModal = ref(false)
const manualImporting = ref(false)
const manualKernelPath = ref('')
const manualDropActive = ref(false)
let manualDropUnlisten: (() => void) | null = null

interface NavItem {
  key: SettingTabKey
  label: string
  icon: Component
}

const navItems = computed<NavItem[]>(() => [
  {
    key: 'general',
    label: t('setting.navigation.general'),
    icon: SettingsOutline,
  },
  {
    key: 'appearance',
    label: t('setting.navigation.appearance'),
    icon: ColorPaletteOutline,
  },
  {
    key: 'kernel',
    label: t('setting.navigation.kernel'),
    icon: HardwareChipOutline,
  },
  {
    key: 'network',
    label: t('setting.navigation.network'),
    icon: GlobeOutline,
  },
  {
    key: 'maintenance',
    label: t('setting.navigation.maintenance'),
    icon: CloudDownloadOutline,
  },
  {
    key: 'about',
    label: t('setting.navigation.about'),
    icon: InformationCircleOutline,
  },
])

const languageOptions = computed<{ label: string; value: Locale }[]>(() => [
  { label: t('setting.language.auto'), value: 'auto' },
  ...supportedLocales.map((item) => ({
    label: item.name,
    value: item.code as Locale,
  })),
])
const updateChannelOptions = computed<{ label: string; value: UpdateChannel }[]>(() => [
  { label: t('setting.update.channelStable'), value: 'stable' },
  { label: t('setting.update.channelPrerelease'), value: 'prerelease' },
  { label: t('setting.update.channelAutobuild'), value: 'autobuild' },
])
const trayCloseBehaviorOptions = computed<{ label: string; value: TrayCloseBehavior }[]>(() => [
  { label: t('setting.startup.closeBehaviorHide'), value: 'hide' },
  { label: t('setting.startup.closeBehaviorLightweight'), value: 'lightweight' },
])

const tunStackOptions = TUN_STACK_OPTIONS

const kernelLatestVersion = computed(() => kernelStore.latestAvailableVersion || '')
const activeSubscription = computed(() => subStore.getActiveSubscription())
const usingOriginalConfig = computed(() => activeSubscription.value?.useOriginalConfig ?? false)
const hasNewVersion = computed(() => kernelStore.hasKernelUpdate)
const kernelVersionOptions = computed(() => {
  const versions = kernelStore.availableVersions || []
  return [
    { label: t('setting.kernel.latest'), value: undefined },
    ...versions.map((v) => ({ label: v, value: v })),
  ]
})
const updateStatus = computed(() => updateStore.updateState.status)
const updateProgress = computed(() => updateStore.updateState.progress || 0)
const updateMessage = computed(() => updateStore.updateState.message)
const isUpdating = computed(() => ['downloading', 'installing'].includes(updateStatus.value))
const showUpdateProgress = computed(
  () =>
    ['downloading', 'installing', 'completed'].includes(updateStatus.value) ||
    updateProgress.value > 0,
)
const backupBusy = computed(
  () => backupExporting.value || backupValidating.value || backupRestoring.value,
)

const {
  loading,
  downloading,
  downloadProgress,
  downloadMessage,
  downloadTheKernel,
  cleanupDownloadListener,
} = useKernelDownload({
  selectedVersion: selectedKernelVersion,
  message,
  t,
  checkKernelInstallation: () => kernelStore.checkKernelInstallation(),
})

const { setupUpdateProgressListener, cleanupUpdateProgressListener } = useUpdateProgressListener({
  message,
  updateStore,
  t,
})

const formatVersion = (v: string) => v.replace(/^v/, '')
const isSupportedLocale = (l: string) => languageOptions.value.some((opt) => opt.value === l)
const onSelectedKernelVersionChange = (value: string | undefined) => {
  selectedKernelVersion.value = value
}

watch(showManualImportModal, (visible) => {
  if (!visible) {
    manualDropActive.value = false
  }
})

const onAutoStartChange = async (value: boolean) => {
  try {
    await appStore.toggleAutoStart(value)
    message.success(t('common.saveSuccess'))
  } catch (error) {
    message.error(t('common.saveFailed'))
    autoStart.value = !value
  }
}

const onAutoStartKernelChange = async (value: boolean) => {
  try {
    await appStore.toggleAutoStartKernel(value)
    message.success(t('common.saveSuccess'))
  } catch (error) {
    console.error('保存内核自动启动设置失败:', error)
    message.error(t('common.saveFailed'))
  }
}

const onTrayCloseBehaviorChange = async (value: TrayCloseBehavior) => {
  const previous = appStore.trayCloseBehavior
  try {
    await appStore.setTrayCloseBehavior(value)
    trayCloseBehavior.value = value
    message.success(t('common.saveSuccess'))
  } catch (error) {
    console.error('保存关闭到托盘行为失败:', error)
    trayCloseBehavior.value = previous
    message.error(t('common.saveFailed'))
  }
}

const onAutoHideToTrayOnAutostartChange = async (value: boolean) => {
  const previous = appStore.autoHideToTrayOnAutostart
  try {
    await appStore.setAutoHideToTrayOnAutostart(value)
    autoHideToTrayOnAutostart.value = value
    message.success(t('common.saveSuccess'))
  } catch (error) {
    console.error('保存开机后自动隐藏窗口到托盘设置失败:', error)
    autoHideToTrayOnAutostart.value = previous
    message.error(t('common.saveFailed'))
  }
}
const handleChangeLanguage = async (value: string) => {
  if (!isSupportedLocale(value)) {
    console.warn('选择了不受支持的语言:', value)
    return
  }

  const nextLocale = value === 'auto' ? 'auto' : value
  await localeStore.setLocale(nextLocale as Locale)
}

const onIpVersionChange = async (value: boolean) => {
  try {
    await appStore.setPreferIpv6(value)
    await appStore.saveToBackend()

    const toggled = await kernelStore.toggleIpVersion(value)
    if (!toggled) {
      throw new Error(kernelStore.lastError || t('notification.ipVersionChangeFailed'))
    }

    message.success(t('common.saveSuccess'))
  } catch (error) {
    console.error('切换IPv6优先失败:', error)
    message.error(t('notification.proxyModeChangeFailed'))
  }
}

const onLanAccessChange = async (value: boolean) => {
  const previous = appStore.allowLanAccess
  appStore.allowLanAccess = value

  try {
    await appStore.saveToBackend({ applyRuntime: true })
    message.success(t('common.saveSuccess'))
  } catch (error) {
    console.error('切换局域网访问失败:', error)
    appStore.allowLanAccess = previous
    message.error(t('common.saveFailed'))
  }
}

const onThemeModeChange = async (value: ThemeMode) => {
  await themeStore.setThemeMode(value)
}

const onAccentChange = async (value: string) => {
  await themeStore.setAccentColor(value)
}

const selectAccentPreset = async (color: string) => {
  await themeStore.setAccentColor(color)
}

const onCompactModeChange = async (value: boolean) => {
  await themeStore.setCompactMode(value)
}

const setupManualDropListener = async () => {
  if (manualDropUnlisten) return

  const appWindow = Window.getCurrent()
  manualDropUnlisten = await appWindow.onDragDropEvent((event) => {
    if (!showManualImportModal.value || manualImporting.value) return

    if (event.payload.type === 'enter' || event.payload.type === 'over') {
      manualDropActive.value = true
      return
    }

    if (event.payload.type === 'leave') {
      manualDropActive.value = false
      return
    }

    if (event.payload.type === 'drop') {
      manualDropActive.value = false
      if (event.payload.paths.length > 0) {
        manualKernelPath.value = event.payload.paths[0]
      }
    }
  })
}

const cleanupManualDropListener = () => {
  if (manualDropUnlisten) {
    manualDropUnlisten()
    manualDropUnlisten = null
  }
}

const showManualDownloadModal = () => {
  manualKernelPath.value = ''
  showManualImportModal.value = true
}

const pickManualKernelFile = async () => {
  try {
    const selected = await systemService.pickKernelImportFile()
    if (selected) {
      manualKernelPath.value = selected
    }
  } catch (error) {
    console.error('选择内核文件失败:', error)
    message.error(t('setting.kernel.pickFailed'))
  }
}

const importManualKernel = async () => {
  if (!manualKernelPath.value) {
    message.warning(t('setting.kernel.noFileSelected'))
    return
  }

  manualImporting.value = true
  try {
    const result = await systemService.importKernelExecutable(manualKernelPath.value)
    message.success(result.message || t('common.saveSuccess'))
    showManualImportModal.value = false
    manualKernelPath.value = ''
    await kernelStore.checkKernelInstallation()
  } catch (error) {
    console.error('导入内核失败:', error)
    const errMsg = error instanceof Error ? error.message : t('setting.kernel.importFailed')
    message.error(errMsg)
  } finally {
    manualImporting.value = false
  }
}

const checkManualInstall = async () => {
  await kernelStore.checkKernelInstallation()
}

const handleUpdateNow = async () => {
  if (!updateStore.hasUpdate) {
    message.info(t('setting.update.alreadyLatest'))
    return
  }

  try {
    if (!updateStore.supportsInAppUpdate) {
      await updateStore.openReleasePage()
      return
    }

    updateStore.updateProgress('downloading', 0, t('setting.update.preparingDownload'))
    await updateStore.downloadAndInstallUpdate()
  } catch (error) {
    console.error('执行更新操作失败:', error)
    const errMsg = error instanceof Error ? error.message : t('setting.update.updateFailed')
    message.error(errMsg)
  }
}

const handleCheckUpdate = async () => {
  checkingUpdate.value = true
  try {
    await updateStore.checkUpdate()
  } finally {
    checkingUpdate.value = false
  }
}

const onAutoCheckUpdateChange = (value: boolean) => {
  updateStore.autoCheckUpdate = value
}

const onUpdateChannelChange = async (value: UpdateChannel) => {
  const previous = updateStore.updateChannel
  try {
    await updateStore.setUpdateChannel(value)
  } catch (error) {
    console.error('保存更新通道设置失败:', error)
    message.error(t('common.saveFailed'))
    updateStore.updateChannel = previous
  }
}

const reloadStoresAfterBackupRestore = async () => {
  await Promise.all([
    appStore.loadFromBackend(),
    themeStore.loadFromBackend(),
    localeStore.loadFromBackend(),
    updateStore.loadFromBackend(),
    subStore.loadFromBackend(),
  ])
  autoStart.value = appStore.autoStartApp
}

const handleExportBackup = async () => {
  backupExporting.value = true
  try {
    const result = await systemService.backupExportSnapshot()
    message.success(t('setting.backup.exportSuccess', { path: result.file_path }))
  } catch (error) {
    console.error('导出备份失败:', error)
    const errMsg = error instanceof Error ? error.message : t('setting.backup.operationFailed')
    message.error(errMsg)
  } finally {
    backupExporting.value = false
  }
}

const handleValidateBackup = async () => {
  backupValidating.value = true
  try {
    const result = await systemService.backupImportSnapshot({ dryRun: true })
    backupPreview.value = result
    if (result.warnings.length > 0) {
      message.warning(t('setting.backup.validateWithWarnings', { count: result.warnings.length }))
    } else {
      message.success(t('setting.backup.validateSuccess', { count: result.subscriptions_count }))
    }
  } catch (error) {
    console.error('预检备份失败:', error)
    const errMsg = error instanceof Error ? error.message : t('setting.backup.operationFailed')
    message.error(errMsg)
  } finally {
    backupValidating.value = false
  }
}

const confirmRestoreBackup = () => {
  let resolved = false

  return new Promise<boolean>((resolve) => {
    const finish = (result: boolean) => {
      if (resolved) return
      resolved = true
      resolve(result)
    }

    dialog.warning({
      title: t('setting.backup.restoreAction'),
      content: t('setting.backup.restoreConfirm'),
      positiveText: t('common.confirm'),
      negativeText: t('common.cancel'),
      maskClosable: false,
      onPositiveClick: () => finish(true),
      onNegativeClick: () => finish(false),
      onClose: () => finish(false),
    })
  })
}

const handleRestoreBackup = async () => {
  const confirmed = await confirmRestoreBackup()
  if (!confirmed) {
    return
  }

  backupRestoring.value = true
  try {
    const result = await systemService.backupImportSnapshot({
      filePath: backupPreview.value?.file_path,
      dryRun: false,
    })
    backupPreview.value = result
    await reloadStoresAfterBackupRestore()
    if (result.warnings.length > 0) {
      message.warning(t('setting.backup.restoreWithWarnings', { count: result.warnings.length }))
    } else {
      message.success(t('setting.backup.restoreSuccess', { count: result.subscriptions_count }))
    }
  } catch (error) {
    console.error('恢复备份失败:', error)
    const errMsg = error instanceof Error ? error.message : t('setting.backup.operationFailed')
    message.error(errMsg)
  } finally {
    backupRestoring.value = false
  }
}

const showPortSettings = () => {
  showPortModal.value = true
}

onMounted(async () => {
  try {
    await setupManualDropListener()
  } catch (error) {
    console.error('初始化手动导入拖放监听失败:', error)
  }

  await appStore.waitForDataRestore()
  await appStore.syncAutoStartWithSystem()
  autoStart.value = appStore.autoStartApp
  autoHideToTrayOnAutostart.value = appStore.autoHideToTrayOnAutostart
  trayCloseBehavior.value = appStore.trayCloseBehavior
  watch(
    () => appStore.autoStartApp,
    (enabled) => {
      autoStart.value = enabled
    },
    { immediate: false },
  )
  watch(
    () => appStore.autoHideToTrayOnAutostart,
    (enabled) => {
      autoHideToTrayOnAutostart.value = enabled
    },
    { immediate: false },
  )
  watch(
    () => appStore.trayCloseBehavior,
    (behavior) => {
      trayCloseBehavior.value = behavior
    },
    { immediate: false },
  )

  try {
    platformInfo.value = await systemService.getDetailedPlatformInfo()
  } catch (error) {
    console.error('获取平台信息失败:', error)
  }

  await kernelStore.checkKernelInstallation()
  if (kernelStore.fetchLatestKernelVersion) {
    await kernelStore.fetchLatestKernelVersion()
  }
  if (
    kernelStore.fetchKernelReleases &&
    (!kernelStore.availableVersions || kernelStore.availableVersions.length === 0)
  ) {
    await kernelStore.fetchKernelReleases()
  }
  await updateStore.initializeStore?.()
  await setupUpdateProgressListener()
})

onUnmounted(() => {
  cleanupManualDropListener()
  cleanupDownloadListener()
  cleanupUpdateProgressListener()
})
</script>

<style scoped>
.page-shell {
  display: flex;
  flex-direction: column;
  gap: var(--layout-page-gap, 24px);
  height: 100%;
}

.settings-tabs {
  display: flex;
  gap: 8px;
  background: var(--glass-bg);
  backdrop-filter: var(--glass-blur);
  border: 1px solid var(--border-light);
  border-radius: var(--radius-lg);
  padding: 6px;
  box-shadow: 0 4px 24px -6px rgba(0, 0, 0, 0.05);
}

.settings-tab {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 8px 16px;
  border-radius: 8px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s ease;
  position: relative;
  white-space: nowrap;
}

.settings-tab:hover {
  color: var(--text-primary);
  background: var(--bg-tertiary);
}

.settings-tab.active {
  background: var(--primary-color);
  color: white;
  box-shadow: 0 2px 8px rgba(99, 102, 241, 0.25);
}

.tab-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #f59e0b;
  box-shadow: 0 0 6px rgba(245, 158, 11, 0.5);
  flex-shrink: 0;
}

.settings-body {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
}

.settings-content {
  max-width: 720px;
}

.section-fade-enter-active,
.section-fade-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}

.section-fade-enter-from {
  opacity: 0;
  transform: translateY(6px);
}

.section-fade-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}

.manual-import-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.manual-import-desc {
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.5;
}

.manual-drop-zone {
  border: 1px dashed var(--panel-border);
  border-radius: 10px;
  background: var(--bg-tertiary);
  padding: 16px;
  text-align: center;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  color: var(--text-secondary);
  transition:
    border-color 0.2s ease,
    background 0.2s ease;
}

.manual-drop-zone.active {
  border-color: var(--primary-color);
  background: rgba(59, 130, 246, 0.12);
}

.manual-drop-sub {
  font-size: 12px;
  color: var(--text-tertiary);
}

.manual-selected {
  border: 1px solid var(--panel-border);
  border-radius: 8px;
  padding: 10px 12px;
  background: var(--bg-tertiary);
}

.manual-selected-label {
  font-size: 12px;
  color: var(--text-tertiary);
  margin-bottom: 4px;
}

.manual-selected-path {
  font-size: 12px;
  color: var(--text-primary);
  word-break: break-all;
  line-height: 1.45;
}

@media (max-width: 768px) {
  .page-shell {
    padding: 14px;
  }

  .settings-tabs {
    overflow-x: auto;
  }

  .settings-tab span:not(.tab-dot) {
    display: none;
  }

  .settings-tab {
    padding: 8px 12px;
  }
}
</style>
