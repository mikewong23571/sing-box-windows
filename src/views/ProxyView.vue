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

    <n-tabs v-model:value="proxyStore.viewTab" type="segment" size="small" style="margin-bottom: 16px; width: 240px;">
      <n-tab-pane name="groups" :tab="t('proxy.groupsTab')" />
      <n-tab-pane name="providers" :tab="t('proxy.providersTab')" />
    </n-tabs>

    <n-flex v-if="proxyStore.viewTab === 'groups'" vertical size="large" class="node-panel">
      <n-flex align="center" :wrap="false" class="node-toolbar">
        <n-input v-model:value="searchQuery" size="small" :placeholder="t('proxy.searchNode')" clearable style="flex: 1">
          <template #prefix>
            <n-icon><SearchOutline /></n-icon>
          </template>
        </n-input>
        <n-select v-model:value="proxyStore.ordering" size="small" :options="orderingOptions" style="width: 120px" />
        <n-button size="small" quaternary :type="favoritesOnly ? 'primary' : 'default'" @click="favoritesOnly = !favoritesOnly">
          <template #icon>
            <n-icon>
              <Star v-if="favoritesOnly" />
              <StarOutline v-else />
            </n-icon>
          </template>
        </n-button>
        <n-button
          size="small"
          quaternary
          :type="proxyStore.hideUnavailable ? 'primary' : 'default'"
          @click="proxyStore.hideUnavailable = !proxyStore.hideUnavailable"
          :title="proxyStore.hideUnavailable ? t('proxy.hideUnavailable') : t('proxy.showAll')"
        >
          <template #icon>
            <n-icon>
              <EyeOffOutline v-if="proxyStore.hideUnavailable" />
              <EyeOutline v-else />
            </n-icon>
          </template>
        </n-button>
      </n-flex>

      <n-collapse v-if="filteredGroups.length" :default-expanded-names="[proxyStore.selectedGroup]" accordion>
        <n-collapse-item
          v-for="group in filteredGroups"
          :key="group.name"
          :name="group.name"
        >
          <template #header>
            <n-space align="center">
              <n-text strong>{{ group.name }}</n-text>
              <n-tag size="tiny" round :bordered="false">{{ group.type }}</n-tag>
            </n-space>
          </template>
          <template #header-extra>
            <n-space align="center">
              <n-text depth="3" style="font-size: 13px">{{ group.now ? getDisplayNodeName(group.now) : '-' }}</n-text>
            </n-space>
          </template>

          <n-card size="small" style="margin-bottom: 12px; background: transparent;">
            <n-flex align="center" justify="space-between">
              <n-space align="center">
                <n-text strong>{{ t('proxy.recommended') }}</n-text>
                <n-text v-if="proxyStore.getRecommendedNode(group)">
                  <img
                    v-if="getNodeFlagUrl(proxyStore.getRecommendedNode(group))"
                    class="node-flag"
                    :src="getNodeFlagUrl(proxyStore.getRecommendedNode(group))"
                    alt=""
                    style="height: 14px; border-radius: 2px; vertical-align: middle; margin-right: 4px;"
                  />
                  {{ getDisplayNodeName(proxyStore.getRecommendedNode(group)) }}
                </n-text>
                <n-text v-else>-</n-text>
                <n-divider vertical />
                <n-text>{{ group.all.length }} {{ t('proxy.nodes') }}</n-text>
                <n-divider vertical />
                <n-text>{{ t('proxy.favorites') }} {{ group.all.filter(n => proxyStore.isFavorite(n)).length }}</n-text>
              </n-space>
              <n-space>
                <n-button size="small" secondary @click.stop="testGroupDelay(group.name)" :loading="proxyStore.groupTestingMap[group.name]">
                  {{ t('proxy.testNode') }}
                </n-button>
                <n-button size="small" tertiary @click.stop="switchRecommended(group)">
                  {{ t('proxy.autoSelect') }}
                </n-button>
              </n-space>
            </n-flex>
          </n-card>

          <n-list
            v-if="getFilteredGroupNodes(group).length"
            hoverable
            clickable
            class="proxy-node-list"
            :show-divider="true"
          >
            <n-list-item
              v-for="node in getFilteredGroupNodes(group)"
              :key="node"
              :class="{ 'active-node-item': group.now === node }"
              @click="changeProxy(group.name, node)"
              style="padding: 12px 16px;"
            >
              <n-flex justify="space-between" align="center" :wrap="false">
                <n-flex align="center" :wrap="false" style="flex: 1; overflow: hidden; min-width: 0;">
                  <img v-if="getNodeFlagUrl(node)" class="node-flag" :src="getNodeFlagUrl(node)" alt="" style="height: 14px; border-radius: 2px; vertical-align: middle; margin-right: 8px;" />
                  <n-ellipsis :tooltip="true" style="flex: 1;">
                    <n-text :strong="group.now === node">{{ getDisplayNodeName(node) }}</n-text>
                  </n-ellipsis>
                </n-flex>
                
                <n-flex align="center" :wrap="false" style="min-width: 140px; justify-content: flex-end;">
                  <n-tag
                    size="small"
                    round
                    :bordered="false"
                    style="margin-right: 8px;"
                  >
                    {{ formatLatency(node) }}
                  </n-tag>
                  <n-button
                    size="small"
                    quaternary
                    :loading="proxyStore.nodeTestingMap[node]"
                    @click.stop="testNode(node)"
                    style="margin-right: 4px;"
                  >
                    <template #icon>
                      <n-icon size="14"><SpeedometerOutline /></n-icon>
                    </template>
                  </n-button>
                  <n-button text size="tiny" @click.stop="proxyStore.toggleFavorite(node)">
                    <n-icon size="18">
                      <Star v-if="proxyStore.isFavorite(node)" style="color: #f59e0b" />
                      <StarOutline v-else />
                    </n-icon>
                  </n-button>
                </n-flex>
              </n-flex>
            </n-list-item>
          </n-list>
          <n-empty v-else :description="t('proxy.noNodes')" style="margin-top: 24px;">
            <template #icon><n-icon><ServerOutline /></n-icon></template>
          </n-empty>
        </n-collapse-item>
      </n-collapse>
      
      <n-empty v-else :description="t('proxy.noProxyGroups')" style="margin-top: 48px;">
        <template #icon><n-icon><GlobeOutline /></n-icon></template>
      </n-empty>
    </n-flex>

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
                <n-card size="small" class="provider-node-item">
                  <n-flex justify="space-between" align="center" :wrap="false">
                    <n-ellipsis :tooltip="false" style="flex: 1">
                      <img v-if="getNodeFlagUrl(proxy.name)" class="node-flag" :src="getNodeFlagUrl(proxy.name)" alt="" style="height: 14px; border-radius: 2px; vertical-align: middle; margin-right: 4px;" />
                      <n-text>{{ getDisplayNodeName(proxy.name) }}</n-text>
                    </n-ellipsis>
                    <n-tag size="small" round :bordered="false">{{ formatLatency(proxy.name) }}</n-tag>
                  </n-flex>
                </n-card>
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
  EyeOffOutline
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
    .replace(/[\s·・,，.。()（）[\]【】'’"“”_-]/g, '')
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

const selectGroup = (name: string) => {
  proxyStore.selectedGroup = name
}

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

const formatLatency = (proxyName: string) => {
  const latency = proxyStore.getLatency(proxyName)
  return latency > 0 ? `${latency}ms` : t('proxy.clickToTest')
}

if (!proxyStore.proxyGroups.length) {
  refresh()
}
</script>

<style scoped>
.page-shell {
  display: flex;
  flex-direction: column;
  gap: 20px;
  height: 100%;
}

.proxy-node-list {
  background: transparent;
  border-radius: var(--radius-lg);
}

.active-node-item {
  background: var(--bg-tertiary);
  font-weight: 600;
  color: var(--primary-color);
}

.proxy-top-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-shrink: 0;
}

.top-bar-left {
  flex-shrink: 0;
}

.top-bar-right {
  display: flex;
  gap: 12px;
}

.split-view {
  display: flex;
  gap: 0;
  flex: 1;
  min-height: 0;
  border-radius: var(--radius-lg);
  border: 1px solid var(--border-light);
  background: var(--glass-bg);
  backdrop-filter: var(--glass-blur);
  overflow: hidden;
  box-shadow: 0 4px 24px -6px rgba(0, 0, 0, 0.05);
}

.group-sidebar {
  width: 260px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border-light);
  background: transparent;
}

.sidebar-title {
  padding: 20px 20px 12px;
  font-size: 12px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.08em;
  color: var(--text-tertiary);
}

.sidebar-list {
  flex: 1;
  overflow-y: auto;
  padding: 0 12px 12px;
}

.sidebar-item {
  padding: 12px 14px;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: all var(--transition-fast);
  margin-bottom: 4px;
}

.sidebar-item:hover {
  background: var(--glass-bg-hover);
}

.sidebar-item.active {
  background: var(--chip-bg);
}

.sidebar-item-main {
  display: flex;
  align-items: center;
  gap: 10px;
}

.sidebar-item-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-secondary);
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sidebar-item.active .sidebar-item-name {
  color: var(--primary-color);
}

.sidebar-item-now {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
  font-size: 12px;
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sidebar-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 48px 16px;
  gap: 12px;
  color: var(--text-tertiary);
  font-size: 13px;
}

.node-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  background: transparent;
}

.node-toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-light);
  flex-shrink: 0;
}

.node-summary {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 12px 20px;
  font-size: 13px;
  color: var(--text-secondary);
  border-bottom: 1px solid var(--border-light);
  flex-shrink: 0;
  background: transparent;
}

.summary-text {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.summary-sep {
  color: var(--text-tertiary);
  opacity: 0.3;
}

.summary-spacer {
  flex: 1;
}

.node-grid {
  flex: 1;
  overflow-y: auto;
  padding: 20px;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 16px;
  align-content: start;
}

.node-card {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-light);
  background: transparent;
  cursor: pointer;
  transition: all var(--transition-normal);
}

.node-card:hover {
  background: var(--glass-bg-hover);
  border-color: var(--border-color);
  transform: translateY(-2px);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.05);
}

.node-card.active {
  background: var(--chip-bg);
  border-color: var(--success-color);
  box-shadow: 0 4px 12px rgba(16, 185, 129, 0.1);
}

.node-card-top {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.node-card-name {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.node-fav-btn {
  flex-shrink: 0;
  opacity: 0;
  transition: opacity var(--transition-fast);
}

.node-card:hover .node-fav-btn,
.node-card.active .node-fav-btn {
  opacity: 1;
}

.node-card-bottom {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.node-flag {
  width: 1.4em;
  height: 1em;
  min-width: 1.4em;
  object-fit: cover;
  border-radius: 2px;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.05);
}

.node-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 64px 16px;
  gap: 16px;
  color: var(--text-tertiary);
  font-size: 14px;
}

.providers-view {
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.providers-list {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.provider-card {
  background: var(--glass-bg);
  backdrop-filter: var(--glass-blur);
  border: 1px solid var(--border-light);
  border-radius: var(--radius-lg);
  padding: 20px;
  box-shadow: 0 4px 24px -6px rgba(0, 0, 0, 0.05);
}

.provider-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  margin-bottom: 16px;
}

.provider-name {
  font-size: 16px;
  font-weight: 700;
  color: var(--text-primary);
}

.provider-meta {
  font-size: 13px;
  color: var(--text-tertiary);
  margin-top: 4px;
}

.provider-node-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 12px;
}

.provider-node-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 14px;
  border-radius: var(--radius-md);
  background: transparent;
  border: 1px solid var(--border-light);
  transition: all var(--transition-fast);
}

.provider-node-item:hover {
  background: var(--glass-bg-hover);
  border-color: var(--border-color);
}

.provider-node-name {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

@media (max-width: 900px) {
  .split-view {
    flex-direction: column;
  }

  .group-sidebar {
    width: 100%;
    border-right: none;
    border-bottom: 1px solid var(--border-light);
    max-height: 240px;
  }

  .proxy-top-bar {
    flex-direction: column;
    align-items: stretch;
  }
}
</style>
