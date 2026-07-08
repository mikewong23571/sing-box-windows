<template>
  <div class="page-shell">
    <PageHeader :title="t('proxy.title')" :subtitle="t('proxy.subtitle')">
      <template #actions>
        <n-space>
          <n-button secondary @click="refresh">
            <template #icon><n-icon><RefreshOutline /></n-icon></template>
            {{ t('common.refresh') }}
          </n-button>
          <n-button type="primary" secondary :loading="proxyStore.batchTesting" @click="batchTest">
            <template #icon><n-icon><SpeedometerOutline /></n-icon></template>
            {{ t('proxy.batchTest') }}
          </n-button>
        </n-space>
      </template>
    </PageHeader>

    <n-tabs v-model:value="proxyStore.viewTab" type="segment" size="small" style="width: 240px; flex-shrink: 0;">
      <n-tab-pane name="groups" :tab="t('proxy.groupsTab')" />
      <n-tab-pane name="providers" :tab="t('proxy.providersTab')" />
    </n-tabs>

    <!-- Groups View -->
    <n-flex v-if="proxyStore.viewTab === 'groups'" vertical size="large" class="groups-view">
      <!-- Toolbar -->
      <n-flex align="center" :wrap="false" gap="8" class="proxy-toolbar">
        <n-input v-model:value="searchQuery" size="small" :placeholder="t('proxy.searchNode')" clearable style="flex: 1">
          <template #prefix><n-icon><SearchOutline /></n-icon></template>
        </n-input>
        <n-select v-model:value="proxyStore.ordering" size="small" :options="orderingOptions" style="width: 120px" />
        <n-tooltip :content="favoritesOnly ? t('proxy.showAll') : t('proxy.favorites')">
          <n-button size="small" quaternary :type="favoritesOnly ? 'primary' : 'default'" @click="favoritesOnly = !favoritesOnly">
            <template #icon>
              <n-icon><Star v-if="favoritesOnly" /><StarOutline v-else /></n-icon>
            </template>
          </n-button>
        </n-tooltip>
        <n-tooltip :content="proxyStore.hideUnavailable ? t('proxy.showAll') : t('proxy.hideUnavailable')">
          <n-button size="small" quaternary :type="proxyStore.hideUnavailable ? 'primary' : 'default'" @click="proxyStore.hideUnavailable = !proxyStore.hideUnavailable">
            <template #icon>
              <n-icon><EyeOffOutline v-if="proxyStore.hideUnavailable" /><EyeOutline v-else /></n-icon>
            </template>
          </n-button>
        </n-tooltip>
      </n-flex>

      <!-- Group Collapse -->
      <n-collapse v-if="filteredGroups.length" :default-expanded-names="[proxyStore.selectedGroup]" accordion>
        <n-collapse-item
          v-for="group in filteredGroups"
          :key="group.name"
          :name="group.name"
        >
          <!-- Group Header -->
          <template #header>
            <div class="group-header">
              <span class="group-name">{{ group.name }}</span>
              <n-tag size="tiny" round :bordered="false" class="group-type-tag">{{ group.type }}</n-tag>
            </div>
          </template>

          <!-- Group Header Extra: current node + actions -->
          <template #header-extra>
            <n-flex align="center" gap="8" @click.stop>
              <!-- Current node pill -->
              <div class="current-node-pill" v-if="group.now">
                <img
                  v-if="getNodeFlagUrl(group.now)"
                  class="node-flag"
                  :src="getNodeFlagUrl(group.now)"
                  alt=""
                />
                <span class="node-icon-fallback" v-else>{{ getNodeIcon(group.now) }}</span>
                <span class="current-node-name">{{ getDisplayNodeName(group.now) }}</span>
              </div>
              <!-- Inline action buttons -->
              <n-tooltip :content="t('proxy.autoSelect')">
                <n-button size="tiny" quaternary @click.stop="switchRecommended(group)">
                  <template #icon><n-icon><FlashOutline /></n-icon></template>
                </n-button>
              </n-tooltip>
              <n-tooltip :content="t('proxy.testNode')">
                <n-button
                  size="tiny"
                  quaternary
                  :loading="proxyStore.groupTestingMap[group.name]"
                  @click.stop="testGroupDelay(group.name)"
                >
                  <template #icon><n-icon><SpeedometerOutline /></n-icon></template>
                </n-button>
              </n-tooltip>
            </n-flex>
          </template>

          <!-- Node List -->
          <div class="node-list-wrap">
            <!-- Recommended banner (shown only when a recommendation exists) -->
            <div v-if="proxyStore.getRecommendedNode(group)" class="recommended-banner">
              <n-icon size="13" class="rec-icon"><TrophyOutline /></n-icon>
              <span class="rec-label">{{ t('proxy.recommended') }}</span>
              <img
                v-if="getNodeFlagUrl(proxyStore.getRecommendedNode(group))"
                class="node-flag"
                :src="getNodeFlagUrl(proxyStore.getRecommendedNode(group))"
                alt=""
              />
              <span class="node-icon-fallback" v-else>{{ getNodeIcon(proxyStore.getRecommendedNode(group)) }}</span>
              <span class="rec-name">{{ getDisplayNodeName(proxyStore.getRecommendedNode(group)) }}</span>
              <span class="rec-count">{{ group.all.length }} {{ t('proxy.nodes') }}</span>
              <span v-if="group.all.filter(n => proxyStore.isFavorite(n)).length" class="rec-count">
                ★ {{ group.all.filter(n => proxyStore.isFavorite(n)).length }}
              </span>
            </div>

            <div v-if="getFilteredGroupNodes(group).length" class="node-list">
              <div
                v-for="node in getFilteredGroupNodes(group)"
                :key="node"
                :class="['node-row', { 'node-row--active': group.now === node }]"
                @click="changeProxy(group.name, node)"
              >
                <!-- Active indicator -->
                <span class="node-active-dot" v-if="group.now === node" />

                <!-- Flag / Icon -->
                <div class="node-icon-wrap">
                  <img v-if="getNodeFlagUrl(node)" class="node-flag" :src="getNodeFlagUrl(node)" alt="" />
                  <span v-else class="node-icon-text">{{ getNodeIcon(node) }}</span>
                </div>

                <!-- Name -->
                <div class="node-name-wrap">
                  <span :class="['node-name', { 'node-name--active': group.now === node }]">
                    {{ getDisplayNodeName(node) }}
                  </span>
                  <n-tag
                    v-if="group.now === node"
                    size="tiny"
                    round
                    type="success"
                    :bordered="false"
                    class="node-in-use-tag"
                  >
                    {{ t('sub.inUse') }}
                  </n-tag>
                </div>

                <!-- Right: latency + actions -->
                <div class="node-right">
                  <span :class="['node-latency', getLatencyClass(node)]">{{ formatLatency(node) }}</span>
                  <n-button
                    size="tiny"
                    quaternary
                    :loading="proxyStore.nodeTestingMap[node]"
                    @click.stop="testNode(node)"
                    class="node-action-btn"
                  >
                    <template #icon><n-icon size="13"><SpeedometerOutline /></n-icon></template>
                  </n-button>
                  <n-button text size="tiny" @click.stop="proxyStore.toggleFavorite(node)" class="node-action-btn">
                    <n-icon size="15">
                      <Star v-if="proxyStore.isFavorite(node)" style="color: #f59e0b" />
                      <StarOutline v-else />
                    </n-icon>
                  </n-button>
                </div>
              </div>
            </div>
            <n-empty v-else :description="t('proxy.noNodes')" style="padding: 24px 0;">
              <template #icon><n-icon><ServerOutline /></n-icon></template>
            </n-empty>
          </div>
        </n-collapse-item>
      </n-collapse>

      <n-empty v-else :description="t('proxy.noProxyGroups')" style="margin-top: 48px;">
        <template #icon><n-icon><GlobeOutline /></n-icon></template>
      </n-empty>
    </n-flex>

    <!-- Providers View -->
    <n-flex v-else vertical size="large" class="providers-view">
      <n-grid v-if="filteredProviders.length" responsive="screen" cols="1" :y-gap="16">
        <n-grid-item v-for="provider in filteredProviders" :key="provider.name">
          <n-card size="small">
            <template #header>
              <n-flex justify="space-between" align="center">
                <n-flex vertical :size="0">
                  <n-text strong>{{ provider.name }}</n-text>
                  <n-text depth="3" style="font-size: 13px">
                    {{ provider.vehicleType || '-' }} · {{ provider.behavior || '-' }}
                    · {{ t('proxy.nodeCount') }} {{ provider.proxies?.length || 0 }}
                  </n-text>
                </n-flex>
                <n-button
                  size="small"
                  secondary
                  :loading="proxyStore.providerUpdatingMap[provider.name]"
                  @click="refreshProvider(provider.name)"
                >
                  {{ t('common.refresh') }}
                </n-button>
              </n-flex>
            </template>
            <n-grid responsive="screen" cols="1 s:2 m:3 l:4 xl:5" :x-gap="12" :y-gap="12">
              <n-grid-item v-for="proxy in getProviderNodes(provider)" :key="proxy.name">
                <div class="provider-node-card">
                  <div class="provider-node-left">
                    <img v-if="getNodeFlagUrl(proxy.name)" class="node-flag" :src="getNodeFlagUrl(proxy.name)" alt="" />
                    <span v-else class="node-icon-text">{{ getNodeIcon(proxy.name) }}</span>
                    <n-ellipsis :tooltip="false" style="flex: 1; font-size: 13px;">
                      {{ getDisplayNodeName(proxy.name) }}
                    </n-ellipsis>
                  </div>
                  <span :class="['node-latency', getLatencyClass(proxy.name)]">{{ formatLatency(proxy.name) }}</span>
                </div>
              </n-grid-item>
            </n-grid>
          </n-card>
        </n-grid-item>
      </n-grid>
      <n-empty v-else :description="t('proxy.noProviders')" style="margin-top: 48px;">
        <template #icon><n-icon><GlobeOutline /></n-icon></template>
      </n-empty>
    </n-flex>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useMessage } from 'naive-ui'
import PageHeader from '@/components/common/PageHeader.vue'
import {
  GlobeOutline,
  RefreshOutline,
  SearchOutline,
  SpeedometerOutline,
  Star,
  StarOutline,
  EyeOutline,
  EyeOffOutline,
  FlashOutline,
  TrophyOutline,
  ServerOutline,
} from '@vicons/ionicons5'
import { useProxyStore, type ProxyGroup } from '@/stores/kernel/ProxyStore'
import { useI18n } from 'vue-i18n'
import type { ProxyProvider } from '@/types/controller'

defineOptions({
  name: 'ProxyView',
})

const { t, locale } = useI18n()
const message = useMessage()
const proxyStore = useProxyStore()
const searchQuery = ref('')
const favoritesOnly = ref(false)

const flagIconUrls = import.meta.glob('../../node_modules/flag-icons/flags/4x3/*.svg', {
  eager: true,
  query: '?url',
  import: 'default',
}) as Record<string, string>

const countryCodeAliases: Record<string, string> = {
  UK: 'GB',
}

const countryDisplayLocales = ['zh-CN', 'zh-Hans', 'zh-TW', 'zh-Hant', 'ja-JP', 'en-US']
const allCountryCodes = [
  'AC','AD','AE','AF','AG','AI','AL','AM','AO','AQ','AR','AS','AT','AU','AW','AX','AZ',
  'BA','BB','BD','BE','BF','BG','BH','BI','BJ','BL','BM','BN','BO','BQ','BR','BS','BT',
  'BV','BW','BY','BZ','CA','CC','CD','CF','CG','CH','CI','CK','CL','CM','CN','CO','CP',
  'CR','CU','CV','CW','CX','CY','CZ','DE','DG','DJ','DK','DM','DO','DZ','EA','EC','EE',
  'EG','EH','ER','ES','ET','EU','FI','FJ','FK','FM','FO','FR','GA','GB','GD','GE','GF',
  'GG','GH','GI','GL','GM','GN','GP','GQ','GR','GS','GT','GU','GW','GY','HK','HM','HN',
  'HR','HT','HU','IC','ID','IE','IL','IM','IN','IO','IQ','IR','IS','IT','JE','JM','JO',
  'JP','KE','KG','KH','KI','KM','KN','KP','KR','KW','KY','KZ','LA','LB','LC','LI','LK',
  'LR','LS','LT','LU','LV','LY','MA','MC','MD','ME','MF','MG','MH','MK','ML','MM','MN',
  'MO','MP','MQ','MR','MS','MT','MU','MV','MW','MX','MY','MZ','NA','NC','NE','NF','NG',
  'NI','NL','NO','NP','NR','NU','NZ','OM','PA','PE','PF','PG','PH','PK','PL','PM','PN',
  'PR','PS','PT','PW','PY','QA','RE','RO','RS','RU','RW','SA','SB','SC','SD','SE','SG',
  'SH','SI','SJ','SK','SL','SM','SN','SO','SR','SS','ST','SV','SX','SY','SZ','TA','TC',
  'TD','TF','TG','TH','TJ','TK','TL','TM','TN','TO','TR','TT','TV','TW','TZ','UA','UG',
  'UM','UN','US','UY','UZ','VA','VC','VE','VG','VI','VN','VU','WF','WS','XK','YE','YT',
  'ZA','ZM','ZW',
]

const countryNameOverrides: Array<[RegExp, string]> = [
  [/东京|東京|大阪/i, 'JP'],
  [/香港|hong\s*kong/i, 'HK'],
  [/台湾|台灣|taiwan/i, 'TW'],
  [/美国|美國|洛杉矶|洛杉磯|西雅图|西雅圖|纽约|紐約|america|united\s*states/i, 'US'],
  [/英国|英國|伦敦|倫敦|britain|united\s*kingdom/i, 'GB'],
  [/首尔|首爾|korea/i, 'KR'],
  [/澳洲/i, 'AU'],
  [/沙特阿拉伯|沙特|saudi/i, 'SA'],
  [/阿联酋|阿聯酋|uae|united\s*arab\s*emirates/i, 'AE'],
]

const proxyPrefixCountryCodes = new Set(['CF', 'CL', 'SS'])
const informationalNodePatterns = [
  /^剩余流量[:：]/,
  /^距离下次重置剩余[:：]/,
  /^套餐到期[:：]/,
  /^官网[:：]/,
  /^星云[:：]/,
  /^https?:\/\//i,
]

// Special node name → icon mapping
const specialNodeIcons: Record<string, string> = {
  DIRECT: '⟶',
  direct: '⟶',
  REJECT: '✕',
  reject: '✕',
  BLOCK: '✕',
  block: '✕',
  AUTO: '⚡',
  auto: '⚡',
  PROXY: '◈',
  proxy: '◈',
  GLOBAL: '◈',
  global: '◈',
  SELECTOR: '▾',
}

const getFlagIconCountryCode = (code: string) => {
  if (code === 'EU') return 'eu'
  const normalizedCode = countryCodeAliases[code] || code
  if (!/^[A-Z]{2}$/.test(normalizedCode)) return ''
  return normalizedCode.toLowerCase()
}

const stripLeadingFlagEmoji = (nodeName: string) =>
  nodeName.replace(/^[\u{1f1e6}-\u{1f1ff}]{2}\s*/u, '').trim()

const isInformationalNodeName = (nodeName: string) =>
  informationalNodePatterns.some((pattern) => pattern.test(stripLeadingFlagEmoji(nodeName)))

const normalizeCountryLabel = (value: string) =>
  value
    .toLowerCase()
    .replace(/[\s·・,，.。()（）[\]【】''""\\"_-]/g, '')
    .trim()

const getLocalizedCountryNameEntries = () => {
  const entries: Array<{ code: string; label: string }> = []
  const seen = new Set<string>()

  countryDisplayLocales.forEach((displayLocale) => {
    const displayNames = new Intl.DisplayNames([displayLocale], { type: 'region' })
    allCountryCodes.forEach((countryCode) => {
      let label: string | undefined
      try {
        label = displayNames.of(countryCode)
      } catch {
        return
      }
      if (!label || label === countryCode) return
      const normalizedLabel = normalizeCountryLabel(label)
      if (!normalizedLabel || normalizedLabel.length < 2) return
      const key = `${countryCode}:${normalizedLabel}`
      if (seen.has(key)) return
      seen.add(key)
      entries.push({ code: countryCode, label: normalizedLabel })
    })
  })

  return entries.sort((left, right) => right.label.length - left.label.length)
}

const localizedCountryNameEntries = getLocalizedCountryNameEntries()

const orderingOptions = computed(() => [
  { label: t('proxy.orderNatural'), value: 'natural' },
  { label: t('proxy.orderLatency'), value: 'latency' },
  { label: t('proxy.orderName'), value: 'name' },
])

const filteredGroups = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  return proxyStore.proxyGroups.filter((group) => {
    const matchesQuery =
      !query ||
      group.name.toLowerCase().includes(query) ||
      group.all.some((node) => node.toLowerCase().includes(query))
    return matchesQuery
  })
})

const getFilteredGroupNodes = (group: ProxyGroup) => {
  let list = proxyStore.getSortedNodesForGroup(group)
  const query = searchQuery.value.trim().toLowerCase()

  if (query) {
    list = list.filter((node) => node.toLowerCase().includes(query))
  }

  if (favoritesOnly.value) {
    list = list.filter((node) => proxyStore.isFavorite(node))
  }

  return list
}

const filteredProviders = computed(() => {
  const query = searchQuery.value.trim().toLowerCase()
  return proxyStore.proxyProviders.filter((provider) => {
    return (
      !query ||
      provider.name.toLowerCase().includes(query) ||
      provider.proxies?.some((proxy) => proxy.name.toLowerCase().includes(query))
    )
  })
})

const refresh = async () => {
  try {
    await proxyStore.fetchProxies()
    message.success(t('proxy.loadSuccess'))
  } catch (error) {
    message.error(t('proxy.loadFailedCheck'))
  }
}

const changeProxy = async (groupName: string, proxyName: string) => {
  try {
    await proxyStore.changeProxy(groupName, proxyName)
    message.success(t('proxy.switchSuccess', { group: groupName, proxy: proxyName }))
  } catch (error) {
    message.error(t('proxy.switchErrorMessage'))
  }
}

const testNode = async (proxyName: string) => {
  try {
    await proxyStore.testNodeDelay(proxyName)
  } catch (error) {
    message.error(t('proxy.testFailed'))
  }
}

const testGroupDelay = async (groupName: string) => {
  try {
    await proxyStore.testGroupDelay(groupName)
    message.success(t('proxy.groupTestComplete'))
  } catch (error) {
    message.error(t('proxy.testErrorMessage'))
  }
}

const batchTest = async () => {
  try {
    await proxyStore.testAllGroups()
    message.success(t('proxy.batchTestComplete'))
  } catch (error) {
    message.error(t('proxy.testErrorMessage'))
  }
}

const switchRecommended = async (group: ProxyGroup) => {
  try {
    const recommended = await proxyStore.switchToRecommended(group)
    if (recommended) {
      message.success(t('proxy.switchSuccess', { group: group.name, proxy: recommended }))
    }
  } catch (error) {
    message.error(t('proxy.switchErrorMessage'))
  }
}

const refreshProvider = async (providerName: string) => {
  try {
    await proxyStore.updateProvider(providerName)
    message.success(providerName)
  } catch (error) {
    message.error(String(error))
  }
}

const getProviderNodes = (provider: ProxyProvider) =>
  (provider.proxies || []).slice().sort((left, right) => left.name.localeCompare(right.name))

const getNodeCountryCode = (nodeName: string) => {
  const normalizedName = stripLeadingFlagEmoji(nodeName)
  if (isInformationalNodeName(normalizedName)) return ''

  const override = countryNameOverrides.find(([pattern]) => pattern.test(normalizedName))
  if (override) {
    return getFlagIconCountryCode(override[1])
  }

  const normalizedNodeName = normalizeCountryLabel(normalizedName)
  const localizedCountry = localizedCountryNameEntries.find((entry) =>
    normalizedNodeName.includes(entry.label),
  )
  if (localizedCountry) {
    return getFlagIconCountryCode(localizedCountry.code)
  }

  const tokens = normalizedName
    .toUpperCase()
    .split(/[^A-Z]+/)
    .filter(Boolean)
  const countryCodes = tokens.filter((token) => token === 'EU' || /^[A-Z]{2}$/.test(token))
  if (!countryCodes.length) return ''

  const preferredCode =
    countryCodes.length > 1 && proxyPrefixCountryCodes.has(countryCodes[0])
      ? countryCodes[1]
      : countryCodes[0]
  return getFlagIconCountryCode(preferredCode)
}

const getDisplayNodeName = (nodeName?: string | null) =>
  nodeName ? stripLeadingFlagEmoji(nodeName) : '-'

const getNodeFlagUrl = (nodeName?: string | null) => {
  if (!nodeName) return ''
  const countryCode = getNodeCountryCode(nodeName)
  return countryCode ? flagIconUrls[`../../node_modules/flag-icons/flags/4x3/${countryCode}.svg`] || '' : ''
}

/**
 * Returns a text icon for nodes without a country flag (DIRECT, REJECT, etc.)
 */
const getNodeIcon = (nodeName?: string | null): string => {
  if (!nodeName) return '○'
  const upper = nodeName.toUpperCase().trim()
  for (const [key, icon] of Object.entries(specialNodeIcons)) {
    if (upper === key.toUpperCase()) return icon
  }
  // Informational nodes
  if (isInformationalNodeName(stripLeadingFlagEmoji(nodeName))) return 'ℹ'
  // Generic proxy node without a detected country
  return '○'
}

const formatLatency = (proxyName: string) => {
  const latency = proxyStore.getLatency(proxyName)
  return latency > 0 ? `${latency}ms` : '-'
}

const getLatencyClass = (proxyName: string): string => {
  const latency = proxyStore.getLatency(proxyName)
  if (latency <= 0) return 'latency-none'
  if (latency <= 200) return 'latency-good'
  if (latency <= 500) return 'latency-mid'
  return 'latency-high'
}

if (!proxyStore.proxyGroups.length) {
  refresh()
}
</script>

<style scoped>
.page-shell {
  display: flex;
  flex-direction: column;
  gap: 16px;
  height: 100%;
}

.groups-view {
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

/* ── Toolbar ───────────────────────────────────────────── */
.proxy-toolbar {
  flex-shrink: 0;
}

/* ── Group Header ──────────────────────────────────────── */
.group-header {
  display: flex;
  align-items: center;
  gap: 8px;
}

.group-name {
  font-weight: 600;
  font-size: 14px;
  color: var(--text-primary);
}

.group-type-tag {
  font-size: 11px;
  opacity: 0.7;
}

/* ── Current node pill ─────────────────────────────────── */
.current-node-pill {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 10px 2px 6px;
  background: var(--bg-tertiary);
  border-radius: 20px;
  font-size: 12px;
  color: var(--text-secondary);
  max-width: 160px;
  overflow: hidden;
}

.current-node-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ── Recommended banner ────────────────────────────────── */
.recommended-banner {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 14px;
  margin-bottom: 4px;
  background: linear-gradient(90deg, rgba(99,102,241,0.07) 0%, transparent 100%);
  border-left: 3px solid var(--primary-color);
  border-radius: 0 6px 6px 0;
  font-size: 12.5px;
  color: var(--text-secondary);
}

.rec-icon {
  color: var(--primary-color);
  flex-shrink: 0;
}

.rec-label {
  font-weight: 600;
  color: var(--primary-color);
  flex-shrink: 0;
}

.rec-name {
  font-weight: 500;
  flex-shrink: 0;
}

.rec-count {
  color: var(--text-tertiary);
  font-size: 11.5px;
  margin-left: 4px;
}

/* ── Node List ─────────────────────────────────────────── */
.node-list-wrap {
  padding: 4px 0;
}

.node-list {
  display: flex;
  flex-direction: column;
}

.node-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 14px;
  border-radius: 8px;
  cursor: pointer;
  transition: background var(--transition-fast);
  position: relative;
}

.node-row:hover {
  background: var(--glass-bg-hover);
}

.node-row--active {
  background: var(--bg-tertiary);
}

/* Active left bar */
.node-active-dot {
  position: absolute;
  left: 3px;
  top: 50%;
  transform: translateY(-50%);
  width: 3px;
  height: 60%;
  border-radius: 2px;
  background: var(--success-color, #10b981);
}

/* Node icon (flag or text) */
.node-icon-wrap {
  width: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.node-flag {
  width: 20px;
  height: 14px;
  object-fit: cover;
  border-radius: 2px;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.08);
}

.node-icon-text {
  font-size: 13px;
  color: var(--text-tertiary);
  line-height: 1;
  font-family: monospace;
  user-select: none;
}

.node-icon-fallback {
  font-size: 13px;
  color: var(--text-tertiary);
  line-height: 1;
}

/* Node name */
.node-name-wrap {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  overflow: hidden;
}

.node-name {
  font-size: 13.5px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.node-name--active {
  color: var(--text-primary);
  font-weight: 600;
}

.node-in-use-tag {
  flex-shrink: 0;
  font-size: 10px;
}

/* Right side */
.node-right {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}

.node-latency {
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  min-width: 44px;
  text-align: right;
}

.latency-none  { color: var(--text-tertiary); }
.latency-good  { color: #10b981; }
.latency-mid   { color: #f59e0b; }
.latency-high  { color: #ef4444; }

.node-action-btn {
  opacity: 0;
  transition: opacity var(--transition-fast);
}

.node-row:hover .node-action-btn {
  opacity: 1;
}

/* ── Provider Nodes ────────────────────────────────────── */
.provider-node-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 8px;
  border: 1px solid var(--border-light);
  background: var(--bg-tertiary);
  font-size: 13px;
}

.provider-node-left {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  flex: 1;
  overflow: hidden;
}
</style>
