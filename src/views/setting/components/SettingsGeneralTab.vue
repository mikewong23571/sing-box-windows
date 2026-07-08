<template>
  <div class="setting-section">

    <h3 class="setting-section-title">{{ props.t('setting.general.title') }}</h3>

    <div class="setting-row">
      <div class="setting-info">
        <div class="setting-label">{{ props.t('setting.language.title') }}</div>
        <div class="setting-desc">{{ props.t('setting.language.description') }}</div>
      </div>
      <n-select
        :value="props.localeStore.locale"
        :options="props.languageOptions"
        size="small"
        style="width: 160px"
        @update:value="props.onChangeLanguage"
      />
    </div>

    <h3 class="setting-section-title">{{ props.t('setting.startup.title') }}</h3>

    <div class="setting-row">
      <div class="setting-info">
        <div class="setting-label">{{ props.t('setting.autoStart.app') }}</div>
        <div class="setting-desc">{{ props.t('setting.autoStart.appDesc') }}</div>
      </div>
      <n-switch :value="props.autoStart" @update:value="props.onAutoStartChange" />
    </div>

    <div v-if="props.autoStart" class="setting-row" style="padding-left: 24px;">
      <div class="setting-info">
        <div class="setting-label">
          {{ props.t('setting.startup.autoHideToTrayOnAutostart') }}
        </div>
        <div class="setting-desc">
          {{ props.t('setting.startup.autoHideToTrayOnAutostartDesc') }}
        </div>
      </div>
      <n-switch
        :value="props.autoHideToTrayOnAutostart"
        @update:value="props.onAutoHideToTrayOnAutostartChange"
      />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <div class="setting-label">{{ props.t('setting.startup.closeBehavior') }}</div>
        <div class="setting-desc">{{ props.t('setting.startup.closeBehaviorDesc') }}</div>
      </div>
      <n-select
        :value="props.trayCloseBehavior"
        :options="props.trayCloseBehaviorOptions"
        size="small"
        style="width: 160px"
        @update:value="props.onTrayCloseBehaviorChange"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import type { Locale } from '@/stores/app/LocaleStore'
import type { TrayCloseBehavior } from '@/stores/app/AppStore'
import type { useLocaleStore } from '@/stores'

type LocaleStoreLike = ReturnType<typeof useLocaleStore>

interface Option<T extends string = string> {
  label: string
  value: T
}

const props = defineProps<{
  t: (key: string, params?: Record<string, string | number>) => string
  localeStore: LocaleStoreLike
  autoStart: boolean
  autoHideToTrayOnAutostart: boolean
  trayCloseBehavior: TrayCloseBehavior
  languageOptions: Option<Locale>[]
  trayCloseBehaviorOptions: Option<TrayCloseBehavior>[]
  onChangeLanguage: (v: Locale) => void
  onAutoStartChange: (v: boolean) => void
  onAutoHideToTrayOnAutostartChange: (v: boolean) => void
  onTrayCloseBehaviorChange: (v: TrayCloseBehavior) => void
}>()
</script>

<style scoped>
.theme-toggle-group {
  display: flex;
  gap: 3px;
  background: var(--bg-tertiary);
  border-radius: 8px;
  padding: 3px;
}

.theme-toggle-btn {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 5px 10px;
  border-radius: 6px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s ease;
  white-space: nowrap;
}

.theme-toggle-btn:hover {
  color: var(--text-primary);
}

.theme-toggle-btn.active {
  background: var(--bg-secondary);
  color: var(--primary-color);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.08);
}

.accent-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.accent-presets {
  display: flex;
  gap: 5px;
}

.accent-dot {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  border: 2px solid transparent;
  cursor: pointer;
  transition: all 0.2s ease;
}

.accent-dot:hover {
  transform: scale(1.15);
}

.accent-dot.active {
  border-color: var(--text-primary);
  box-shadow: 0 0 0 2px var(--bg-secondary), 0 2px 8px rgba(0, 0, 0, 0.2);
}
</style>
