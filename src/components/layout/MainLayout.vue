<template>
    <n-layout class="app-layout" position="absolute">
      <!-- Top Header (Native Window Drag Region) -->
      <n-layout-header class="app-header" data-tauri-drag-region>
        <div class="header-brand">
          <div class="brand-logo-wrapper" @click="onSelect('home')">
            <img
              :src="logo"
              alt="Logo"
              class="brand-logo"
              :class="{ 'is-running': kernelStatusClass === 'running' }"
            />
          </div>
          <div class="brand-text">
            <h1 class="app-name">{{ t('common.appName') }}</h1>
            <div class="app-status" :class="kernelStatusClass">
              <span class="status-dot"></span>
              <span class="status-label">{{ appStatusLabel }}</span>
            </div>
          </div>
        </div>

        <!-- Window Controls -->
        <div class="window-controls">
          <button class="control-btn minimize" @click="windowStore.minimizeWindow">
            <n-icon size="16"><RemoveOutline /></n-icon>
          </button>
          <button class="control-btn maximize" @click="windowStore.toggleMaximize">
            <n-icon size="14"><SquareOutline /></n-icon>
          </button>
          <button class="control-btn close" @click="() => windowStore.closeToTray(router)">
            <n-icon size="16"><CloseOutline /></n-icon>
          </button>
        </div>
      </n-layout-header>

      <!-- Main Layout with Sidebar and Content -->
      <n-layout has-sider position="absolute" class="app-body">
        <!-- Modern Sidebar -->
        <n-layout-sider
          class="app-sider"
          :width="220"
          :collapsed-width="72"
          v-model:collapsed="collapsed"
          collapse-mode="width"
          :native-scrollbar="false"
          show-trigger="bar"
        >
          <div class="sider-inner">
            <!-- Navigation Menu -->
            <div class="sider-menu">
              <div class="menu-group">
                <div class="menu-label" v-if="!collapsed">{{ t('nav.navigation') }}</div>
                <div
                  v-for="item in menuItems"
                  :key="item.key"
                  class="menu-item"
                  :class="{ active: currentMenu === item.key, disabled: item.disabled }"
                  @click="!item.disabled && onSelect(item.key)"
                >
                  <n-icon :size="20" class="menu-icon">
                    <component :is="item.icon" />
                  </n-icon>
                  <transition name="fade-slide">
                    <span v-if="!collapsed" class="menu-text">{{ item.label }}</span>
                  </transition>
                </div>
              </div>
            </div>
          </div>
        </n-layout-sider>

        <!-- Main Content Area -->
        <n-layout-content class="app-content" :native-scrollbar="false">
          <div class="content-container">
            <router-view v-slot="{ Component }">
              <transition name="page-fade" mode="out-in">
                <component :is="Component" :key="$route.path" />
              </transition>
            </router-view>
          </div>
        </n-layout-content>
      </n-layout>
    </n-layout>

    <!-- Update Modal -->
    <UpdateModal
      v-model:show="showUpdateModal"
      :latest-version="updateInfo.latestVersion"
      :current-version="updateInfo.currentVersion"
      :download-url="updateInfo.downloadUrl"
      :release-page-url="updateInfo.releasePageUrl"
      :release-notes="updateInfo.releaseNotes"
      :release-date="updateInfo.releaseDate"
      :file-size="updateInfo.fileSize"
      :supports-in-app-update="updateInfo.supportsInAppUpdate"
      @update="handleUpdate"
      @cancel="handleUpdateCancel"
      @skip="handleUpdateSkip"
    />
</template>

<script lang="ts" setup>
import { computed, ref, onMounted, onUnmounted } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useThemeStore } from '@/stores/app/ThemeStore'
import { useLocaleStore } from '@/stores/app/LocaleStore'
import { useAppStore } from '@/stores'
import { useWindowStore } from '@/stores/app/WindowStore'
import { useUpdateStore } from '@/stores/app/UpdateStore'
import { useKernelStore } from '@/stores/kernel/KernelStore'
import { useI18n } from 'vue-i18n'
import {
  HomeOutline,
  SwapHorizontalOutline,
  LinkOutline,
  DocumentTextOutline,
  SettingsOutline,
  FolderOutline,
  MoonOutline,
  SunnyOutline,
  ChevronBackOutline,
  ChevronForwardOutline,
  RemoveOutline,
  SquareOutline,
  CloseOutline,
  AnalyticsOutline,
} from '@vicons/ionicons5'
import { useMessage } from 'naive-ui'
import mitt from 'mitt'
import UpdateModal from '@/components/UpdateModal.vue'
import logo from '@/assets/icon.png'
import { useKernelStatus } from '@/composables/useKernelStatus'

defineOptions({
  name: 'MainLayout',
})

const router = useRouter()
const route = useRoute()
const collapsed = ref(false)
const message = useMessage()
const mittInstance = mitt()

// Stores
const themeStore = useThemeStore()
const localeStore = useLocaleStore()
const appStore = useAppStore()
const windowStore = useWindowStore()
const updateStore = useUpdateStore()
const kernelStore = useKernelStore()
const { t } = useI18n()
const { statusState: kernelStatusState, statusClass: kernelStatusClass } =
  useKernelStatus(kernelStore)

const appStatusLabel = computed(() => {
  switch (kernelStatusState.value) {
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

// Update Modal State
const showUpdateModal = ref(false)
const updateInfo = ref({
  latestVersion: '',
  currentVersion: '',
  downloadUrl: '',
  releasePageUrl: '',
  releaseNotes: '',
  releaseDate: '',
  fileSize: 0,
  supportsInAppUpdate: false,
})

// Theme Configuration
const theme = computed(() => themeStore.naiveTheme)
const themeOverrides = computed(() => themeStore.themeOverrides)

// Menu Configuration
const currentMenu = computed(() => {
  const path = route.path
  if (path === '/' || path === '/home') return 'home'

  const pathToMenuMap: Record<string, string> = {
    '/log': 'logs',
    '/sub': 'subscription',
    '/setting': 'settings',
    '/connections': 'connections',
    '/proxy': 'proxy',
    '/rules': 'rules',
  }
  return pathToMenuMap[path] || path.slice(1)
})

const menuItems = computed(() => [
  { label: t('nav.home'), key: 'home', icon: HomeOutline, disabled: false },
  { label: t('nav.subscription'), key: 'subscription', icon: FolderOutline, disabled: false },
  { label: t('nav.proxy'), key: 'proxy', icon: SwapHorizontalOutline, disabled: false },
  { label: t('nav.connections'), key: 'connections', icon: LinkOutline, disabled: false },
  { label: t('nav.logs'), key: 'logs', icon: DocumentTextOutline, disabled: false },
  { label: t('nav.rules'), key: 'rules', icon: AnalyticsOutline, disabled: false },
  { label: t('nav.settings'), key: 'settings', icon: SettingsOutline, disabled: false },
])

// Navigation
const onSelect = (key: string) => {
  if (key === 'home') {
    router.push('/')
  } else {
    const routeMap: Record<string, string> = {
      logs: '/log',
      subscription: '/sub',
      settings: '/setting',
      connections: '/connections',
      proxy: '/proxy',
      rules: '/rules',
    }
    router.push(routeMap[key] || `/${key}`)
  }
}

// Update Handling
const handleShowUpdateModal = (data: unknown) => {
  if (!data || typeof data !== 'object') return
  const payload = data as Record<string, unknown>
  updateInfo.value = {
    latestVersion: typeof payload.latestVersion === 'string' ? payload.latestVersion : typeof payload.latest_version === 'string' ? payload.latest_version : '',
    currentVersion: typeof payload.currentVersion === 'string' ? payload.currentVersion : updateStore.appVersion,
    downloadUrl: typeof payload.downloadUrl === 'string' ? payload.downloadUrl : typeof payload.download_url === 'string' ? payload.download_url : '',
    releasePageUrl: typeof payload.releasePageUrl === 'string' ? payload.releasePageUrl : typeof payload.release_page_url === 'string' ? payload.release_page_url : updateStore.releasePageUrl,
    releaseNotes: typeof payload.releaseNotes === 'string' ? payload.releaseNotes : typeof payload.release_notes === 'string' ? payload.release_notes : '',
    releaseDate: typeof payload.releaseDate === 'string' ? payload.releaseDate : typeof payload.release_date === 'string' ? payload.release_date : '',
    fileSize: typeof payload.fileSize === 'number' ? payload.fileSize : typeof payload.file_size === 'number' ? payload.file_size : 0,
    supportsInAppUpdate: typeof payload.supportsInAppUpdate === 'boolean' ? payload.supportsInAppUpdate : typeof payload.supports_in_app_update === 'boolean' ? payload.supports_in_app_update : updateStore.supportsInAppUpdate,
  }
  showUpdateModal.value = true
}

const handleUpdate = async () => {
  try {
    if (updateInfo.value.supportsInAppUpdate) {
      message.info(t('setting.update.preparingDownload'))
      await updateStore.downloadAndInstallUpdate()
    } else {
      await updateStore.openReleasePage()
      showUpdateModal.value = false
    }
    showUpdateModal.value = false
  } catch (error) {
    const errMsg = error instanceof Error ? error.message : String(error)
    message.error(errMsg)
  }
}

const handleUpdateCancel = () => {
  showUpdateModal.value = false
}

const handleUpdateSkip = async () => {
  showUpdateModal.value = false
  await updateStore.skipCurrentVersion()
  message.success(t('setting.update.skipSuccess'))
}

// Lifecycle
onMounted(() => {
  mittInstance.on('show-update-modal', handleShowUpdateModal)
})

onUnmounted(() => {
  mittInstance.off('show-update-modal', handleShowUpdateModal)
})
</script>

<style scoped>
.app-layout {
  height: 100vh;
  background: var(--bg-app);
}

.app-body {
  top: 56px !important;
  bottom: 0;
  background: transparent;
}

/* Header Styles - macOS/Windows Native feel */
.app-header {
  height: 56px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px 0 20px;
  background: var(--glass-bg) !important;
  backdrop-filter: var(--glass-blur);
  -webkit-backdrop-filter: var(--glass-blur);
  border-bottom: 1px solid var(--border-light);
  z-index: 200;
  box-shadow: 0 1px 3px 0 rgba(0, 0, 0, 0.02);
}

.header-brand {
  display: flex;
  align-items: center;
  gap: 16px;
  cursor: default;
}

.brand-logo-wrapper {
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: transform var(--transition-normal);
}

.brand-logo-wrapper:hover {
  transform: scale(1.08) rotate(-2deg);
}

.brand-logo {
  width: 28px;
  height: 28px;
  object-fit: contain;
  filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.1));
  transition: all var(--transition-slow);
}

.brand-logo.is-running {
  filter: drop-shadow(0 0 12px rgba(16, 185, 129, 0.6));
}

.brand-text {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 12px;
  white-space: nowrap;
}

.app-name {
  font-size: 16px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0;
  line-height: 1;
}

.app-status {
  font-size: 12px;
  font-weight: 600;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  border-radius: var(--radius-full);
  background: var(--bg-tertiary);
  transition: all var(--transition-normal);
}

.status-label {
  color: var(--text-secondary);
}

.status-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background-color: var(--text-tertiary);
  transition: background-color var(--transition-normal);
}

.app-status.running { background: rgba(16, 185, 129, 0.1); }
.app-status.running .status-label { color: var(--success-color); }
.app-status.running .status-dot {
  background-color: var(--success-color);
  box-shadow: 0 0 8px var(--success-color);
}

.app-status.pending, .app-status.disconnected { background: rgba(245, 158, 11, 0.1); }
.app-status.pending .status-label, .app-status.disconnected .status-label { color: var(--warning-color); }
.app-status.pending .status-dot, .app-status.disconnected .status-dot {
  background-color: var(--warning-color);
  box-shadow: 0 0 6px var(--warning-color);
}

.app-status.stopped, .app-status.failed { background: rgba(239, 68, 68, 0.1); }
.app-status.stopped .status-label, .app-status.failed .status-label { color: var(--error-color); }
.app-status.stopped .status-dot, .app-status.failed .status-dot {
  background-color: var(--error-color);
  box-shadow: 0 0 6px var(--error-color);
}

/* Window Controls */
.window-controls {
  display: flex;
  gap: 6px;
  -webkit-app-region: no-drag;
}

.control-btn {
  width: 32px;
  height: 32px;
  border-radius: var(--radius-sm);
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all var(--transition-fast);
}

.control-btn:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.control-btn.close:hover {
  background: rgba(239, 68, 68, 0.1);
  color: var(--error-color);
}

/* Sidebar Styles */
.app-sider {
  background: transparent !important;
  border-right: 1px solid var(--border-light);
  z-index: 100;
}

.sider-inner {
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 20px 12px;
  background: transparent;
}

/* Menu Styles */
.sider-menu {
  flex: 1;
  overflow-y: auto;
}

.menu-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.menu-label {
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  color: var(--text-tertiary);
  letter-spacing: 0.08em;
  padding: 0 12px;
  margin: 12px 0 8px 0;
}

.menu-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 14px;
  border-radius: var(--radius-md);
  cursor: pointer;
  color: var(--text-secondary);
  transition: all var(--transition-fast);
  height: 44px;
}

.menu-item:hover:not(.disabled) {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.menu-item.active {
  background: var(--chip-bg);
  color: var(--primary-color);
  font-weight: 600;
}

.menu-item.disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.menu-icon {
  display: flex;
  align-items: center;
  justify-content: center;
}

.menu-text {
  font-size: 14px;
  white-space: nowrap;
}

/* Content Styles */
.app-content {
  background: transparent !important;
  position: relative;
}

.content-container {
  height: 100%;
  overflow: hidden; /* Page views manage their own scroll using page-shell */
}

/* Transitions */
.fade-slide-enter-active,
.fade-slide-leave-active {
  transition: all var(--transition-fast);
}

.fade-slide-enter-from,
.fade-slide-leave-to {
  opacity: 0;
  transform: translateX(-10px);
}

.page-fade-enter-active,
.page-fade-leave-active {
  transition: opacity var(--transition-normal), transform var(--transition-normal);
}

.page-fade-enter-from {
  opacity: 0;
  transform: translateY(12px) scale(0.99);
}

.page-fade-leave-to {
  opacity: 0;
  transform: translateY(-12px) scale(0.99);
}
</style>

