<template>
  <div class="page-shell">
    <PageHeader :title="t('connections.title')" :subtitle="t('connections.subtitle')">
      <template #actions>
        <n-space>
          <n-button secondary @click="refreshConnections">
            <template #icon>
              <n-icon><RefreshOutline /></n-icon>
            </template>
            {{ t('common.refresh') }}
          </n-button>
          <n-button secondary @click="connectionStore.togglePaused()">
            <template #icon>
              <n-icon>
                <PauseOutline v-if="!connectionStore.paused" />
                <PlayOutline v-else />
              </n-icon>
            </template>
            {{ connectionStore.paused ? t('connections.resume') : t('connections.pause') }}
          </n-button>
          <n-button
            type="error"
            secondary
            :loading="connectionStore.isClosingAll"
            @click="closeAll"
          >
            <template #icon>
              <n-icon><CloseOutline /></n-icon>
            </template>
            {{ t('connections.closeAll') }}
          </n-button>
        </n-space>
      </template>
    </PageHeader>

    <n-tabs
      v-model:value="connectionStore.activeTab"
      type="segment"
      size="small"
      @update:value="updateActiveTab"
      style="margin-bottom: 16px; width: 280px;"
    >
      <n-tab-pane name="active" :tab="`${t('connections.active')} (${connectionStore.activeConnections.length})`" />
      <n-tab-pane name="closed" :tab="`${t('connections.closed')} (${connectionStore.closedConnections.length})`" />
    </n-tabs>

    <n-card size="small" class="toolbar-card">
      <n-grid responsive="screen" item-responsive :x-gap="12" :y-gap="12" class="toolbar-row">

        <n-grid-item span="24 m:12 l:6">
          <n-input
            v-model:value="connectionStore.searchQuery"
            :placeholder="t('connections.searchPlaceholder')"
            clearable
            class="search-input"
          >
            <template #prefix>
              <n-icon><SearchOutline /></n-icon>
            </template>
          </n-input>
        </n-grid-item>

        <n-grid-item span="24 m:12 l:6">
          <n-input-group>
            <n-input-group-label>{{ t('connections.sourceFilter') }}</n-input-group-label>
            <n-select
              v-model:value="connectionStore.sourceIPFilter"
              :options="sourceIpOptions"
              clearable
              :placeholder="t('common.all')"
              class="source-select"
            />
          </n-input-group>
        </n-grid-item>

        <n-grid-item span="24 m:12 l:6">
          <n-input-group>
            <n-input-group-label>{{ t('common.sort') }}</n-input-group-label>
            <n-select
              v-model:value="connectionStore.sortKey"
              :options="sortOptions"
              class="sort-select"
            />
          </n-input-group>
        </n-grid-item>

        <n-grid-item span="24 m:12 l:6">
          <n-flex wrap="nowrap" :size="8">
            <n-input-group style="flex: 1">
              <n-input-group-label>{{ t('connections.grouping') }}</n-input-group-label>
              <n-select
                v-model:value="connectionStore.groupingKey"
                :options="groupingOptions"
                clearable
                class="sort-select"
                :placeholder="t('common.all')"
              />
            </n-input-group>
            <n-button @click="connectionStore.sortDesc = !connectionStore.sortDesc">
              <template #icon>
                <n-icon>
                  <ArrowDownOutline v-if="connectionStore.sortDesc" />
                  <ArrowUpOutline v-else />
                </n-icon>
              </template>
            </n-button>
          </n-flex>
        </n-grid-item>
      </n-grid>

      <n-space style="margin-top: 12px;" class="stats-row">
        <n-tag size="small" round :bordered="false">
          ↑ {{ formatBytes(connectionStore.connectionsTotal.upload) }}
        </n-tag>
        <n-tag size="small" round :bordered="false">
          ↓ {{ formatBytes(connectionStore.connectionsTotal.download) }}
        </n-tag>
        <n-tag size="small" round :bordered="false">
          {{ t('connections.quickFilter') }}: {{ connectionStore.quickFilterEnabled ? labelsOnOff.on : labelsOnOff.off }}
        </n-tag>
      </n-space>
    </n-card>

    <n-card v-if="groupedRows.length" size="small" style="margin-top: 16px;" class="table-card">
      <div class="connection-table-wrap">
        <n-table :bordered="false" :single-line="false" striped class="connection-table">
        <thead>
          <tr>
            <th>{{ t('connections.destination') }}</th>
            <th>{{ t('connections.download') }}</th>
            <th>{{ t('connections.upload') }}</th>
            <th>{{ t('connections.downloadSpeed') }}</th>
            <th>{{ t('connections.uploadSpeed') }}</th>
            <th>{{ t('connections.chain') }}</th>
            <th>{{ t('connections.rule') }}</th>
            <th>{{ t('connections.process') }}</th>
            <th style="width: 80px">{{ t('connections.actions') }}</th>
          </tr>
        </thead>
        <tbody v-for="group in groupedRows" :key="group.key || 'all'">
          <tr v-if="group.key" class="group-row">
            <td colspan="9">
              <n-space align="center" size="small" class="group-title">
                <n-text strong>{{ group.key }}</n-text>
                <n-tag size="tiny" round>{{ group.count }}</n-tag>
              </n-space>
            </td>
          </tr>
          <tr
            v-for="connection in group.connections"
            :key="connection.id"
            class="connection-row"
            tabindex="0"
            @click="selectedConnection = connection"
            @keydown.enter="selectedConnection = connection"
            @keydown.space.prevent="selectedConnection = connection"
            style="cursor: pointer;"
          >
            <td class="destination-cell">
              <n-flex vertical :size="0">
                <n-text>{{ getDestinationText(connection) }}</n-text>
                <n-text depth="3" style="font-size: 12px">{{ getSourceText(connection) }}</n-text>
              </n-flex>
            </td>
            <td>{{ formatBytes(connection.download) }}</td>
            <td>{{ formatBytes(connection.upload) }}</td>
            <td>{{ formatSpeed(connection.downloadSpeed || 0) }}</td>
            <td>{{ formatSpeed(connection.uploadSpeed || 0) }}</td>
            <td>
              <n-ellipsis :tooltip="true" style="max-width: 150px">
                {{ getChainText(connection) }}
              </n-ellipsis>
            </td>
            <td>
              <n-ellipsis :tooltip="true" style="max-width: 150px">
                {{ getRuleText(connection) }}
              </n-ellipsis>
            </td>
            <td>
              <n-ellipsis :tooltip="true" style="max-width: 150px">
                {{ getProcessText(connection) }}
              </n-ellipsis>
            </td>
            <td class="action-cell">
              <n-button
                v-if="connectionStore.activeTab === 'active'"
                text
                size="small"
                type="error"
                :loading="connectionStore.closingMap[connection.id]"
                @click.stop="closeOne(connection.id)"
                @keydown.stop
              >
                {{ t('connections.close') }}
              </n-button>
              <span v-else>-</span>
            </td>
          </tr>
        </tbody>
        </n-table>
      </div>
    </n-card>

    <n-empty
      v-else
      size="huge"
      :description="connectionStore.activeTab === 'active' ? t('connections.noActiveConnections') : t('connections.noClosed')"
      style="margin-top: 48px"
    >
      <template #icon>
        <n-icon><LinkOutline /></n-icon>
      </template>
    </n-empty>

    <n-modal v-model:show="detailVisible" preset="card" :title="t('connections.detailTitle')" style="width: 720px" :bordered="false">
      <div v-if="selectedConnection" style="display: flex; flex-direction: column; gap: 24px;">
        <div>
          <n-h6 style="margin-top: 0; margin-bottom: 12px; color: var(--primary-color);">{{ t('connections.destination') }} & {{ t('connections.source') }}</n-h6>
          <n-descriptions :column="2" size="small">
            <n-descriptions-item :label="t('connections.destination')">{{ getDestinationText(selectedConnection) }}</n-descriptions-item>
            <n-descriptions-item :label="t('connections.source')">{{ getSourceText(selectedConnection) }}</n-descriptions-item>
            <n-descriptions-item :label="t('connections.network')">{{ selectedConnection.metadata.network || '-' }}</n-descriptions-item>
            <n-descriptions-item :label="t('connections.type')">{{ selectedConnection.metadata.type || '-' }}</n-descriptions-item>
            <n-descriptions-item :label="t('connections.inbound')">{{ selectedConnection.metadata.inboundName || selectedConnection.metadata.inboundUser || '-' }}</n-descriptions-item>
          </n-descriptions>
        </div>

        <div>
          <n-h6 style="margin-top: 0; margin-bottom: 12px; color: var(--primary-color);">{{ t('connections.rule') }} & {{ t('connections.chain') }}</n-h6>
          <n-descriptions :column="2" size="small">
            <n-descriptions-item :label="t('connections.rule')">{{ getRuleText(selectedConnection) }}</n-descriptions-item>
            <n-descriptions-item :label="t('connections.chain')">{{ getChainText(selectedConnection) }}</n-descriptions-item>
            <n-descriptions-item :label="t('connections.process')">{{ getProcessText(selectedConnection) }}</n-descriptions-item>
          </n-descriptions>
        </div>

        <div>
          <n-h6 style="margin-top: 0; margin-bottom: 12px; color: var(--primary-color);">{{ t('connections.total') }} & {{ t('connections.start') }}</n-h6>
          <n-descriptions :column="2" size="small">
            <n-descriptions-item :label="t('connections.download')">{{ formatBytes(selectedConnection.download) }} ({{ formatSpeed(selectedConnection.downloadSpeed || 0) }})</n-descriptions-item>
            <n-descriptions-item :label="t('connections.upload')">{{ formatBytes(selectedConnection.upload) }} ({{ formatSpeed(selectedConnection.uploadSpeed || 0) }})</n-descriptions-item>
            <n-descriptions-item :label="t('connections.start')">{{ formatTimeAgo(selectedConnection.start) }}</n-descriptions-item>
            <n-descriptions-item label="ID">{{ selectedConnection.id }}</n-descriptions-item>
          </n-descriptions>
        </div>
      </div>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useMessage } from 'naive-ui'
import {
  ArrowDownOutline,
  ArrowUpOutline,
  CloseOutline,
  LinkOutline,
  PauseOutline,
  PlayOutline,
  RefreshOutline,
  SearchOutline,
} from '@vicons/ionicons5'
import PageHeader from '@/components/common/PageHeader.vue'
import { useConnectionStore } from '@/stores/kernel/ConnectionStore'
import { useI18n } from 'vue-i18n'
import type { ConnectionItem } from '@/types/events'
import { formatBytes, formatSpeed } from '@/utils'

defineOptions({
  name: 'ConnectionsView',
})

const { t } = useI18n()
const message = useMessage()
const connectionStore = useConnectionStore()
const selectedConnection = ref<ConnectionItem | null>(null)

const labelsOnOff = computed(() => ({
  on: t('connections.on'),
  off: t('connections.off'),
}))

const detailVisible = computed({
  get: () => !!selectedConnection.value,
  set: (value: boolean) => {
    if (!value) {
      selectedConnection.value = null
    }
  },
})

const sourceIpOptions = computed(() => {
  const values = new Set<string>()
  connectionStore.searchableConnections.forEach((connection) => {
    if (connection.metadata.sourceIP) {
      values.add(connection.metadata.sourceIP)
    }
  })
  return Array.from(values).map((value) => ({ label: value, value }))
})

const sortOptions = computed(() => {
  const labels: Record<string, string> = {
    start: t('connections.startTime'),
    download: t('connections.download'),
    upload: t('connections.upload'),
    downloadSpeed: t('connections.downloadSpeed'),
    uploadSpeed: t('connections.uploadSpeed'),
    host: t('connections.destinationAddress'),
    process: t('connections.process'),
    rule: t('connections.rule'),
  }

  return Object.entries(labels).map(([value, label]) => ({ label, value }))
})

const groupingOptions = computed(() => [
  { label: t('connections.groupByRule'), value: 'rule' },
  { label: t('connections.groupByProcess'), value: 'process' },
  { label: t('connections.groupByDestination'), value: 'host' },
  { label: t('connections.groupBySourceIP'), value: 'sourceIP' },
])

const filteredConnections = computed(() => {
  const query = connectionStore.searchQuery.trim().toLowerCase()
  return connectionStore.searchableConnections.filter((connection) => {
    const matchesQuery =
      !query ||
      connection.id.toLowerCase().includes(query) ||
      getDestinationText(connection).toLowerCase().includes(query) ||
      getSourceText(connection).toLowerCase().includes(query) ||
      getRuleText(connection).toLowerCase().includes(query) ||
      (connection.metadata.process || connection.metadata.processPath || '').toLowerCase().includes(query)

    const matchesSource =
      !connectionStore.sourceIPFilter || connection.metadata.sourceIP === connectionStore.sourceIPFilter

    const matchesQuickFilter =
      !connectionStore.quickFilterEnabled ||
      !getRuleText(connection).toLowerCase().includes('direct')

    return matchesQuery && matchesSource && matchesQuickFilter
  })
})

const sortedConnections = computed(() => {
  const list = [...filteredConnections.value]
  const factor = connectionStore.sortDesc ? -1 : 1

  return list.sort((left, right) => {
    const leftValue = getSortValue(left, connectionStore.sortKey)
    const rightValue = getSortValue(right, connectionStore.sortKey)

    if (typeof leftValue === 'number' && typeof rightValue === 'number') {
      return (leftValue - rightValue) * factor
    }

    return String(leftValue).localeCompare(String(rightValue)) * factor
  })
})

const groupedRows = computed(() => {
  const grouping = connectionStore.groupingKey
  if (!grouping) {
    return [
      {
        type: 'group' as const,
        key: '',
        count: sortedConnections.value.length,
        connections: sortedConnections.value,
      },
    ]
  }

  const groups = new Map<string, ConnectionItem[]>()
  sortedConnections.value.forEach((connection) => {
    const key = String(getGroupValue(connection, grouping) || '-')
    const list = groups.get(key) || []
    list.push(connection)
    groups.set(key, list)
  })

  return Array.from(groups.entries()).map(([key, connections]) => ({
    type: 'group' as const,
    key,
    count: connections.length,
    connections,
  }))
})

const refreshConnections = async () => {
  await connectionStore.setupEventListeners()
  message.success(t('connections.refreshSuccess'))
}

const closeAll = async () => {
  try {
    await connectionStore.closeAllConnections()
    message.success(t('connections.closeAll'))
  } catch (err) {
    message.error(String(err))
  }
}

const closeOne = async (id: string) => {
  try {
    await connectionStore.closeConnection(id)
    message.success(t('connections.close'))
  } catch (err) {
    message.error(String(err))
  }
}

const updateActiveTab = (value: string) => {
  if (value === 'active' || value === 'closed') {
    connectionStore.activeTab = value
  }
}

const getSortValue = (connection: ConnectionItem, key: string) => {
  switch (key) {
    case 'download':
      return connection.download
    case 'upload':
      return connection.upload
    case 'downloadSpeed':
      return connection.downloadSpeed || 0
    case 'uploadSpeed':
      return connection.uploadSpeed || 0
    case 'host':
      return getDestinationText(connection)
    case 'process':
      return connection.metadata.process || connection.metadata.processPath || ''
    case 'rule':
      return getRuleText(connection)
    default:
      return new Date(connection.start).getTime()
  }
}

const getGroupValue = (connection: ConnectionItem, key: string) => {
  switch (key) {
    case 'rule':
      return connection.rule
    case 'process':
      return connection.metadata.process || connection.metadata.processPath
    case 'host':
      return getDestinationText(connection)
    case 'sourceIP':
      return connection.metadata.sourceIP
    default:
      return getSortValue(connection, key)
  }
}

const getSourceText = (connection: ConnectionItem) =>
  `${connection.metadata.sourceIP || '-'}:${connection.metadata.sourcePort || '-'}`

const getDestinationText = (connection: ConnectionItem) =>
  connection.metadata.remoteDestination ||
  connection.metadata.host ||
  `${connection.metadata.destinationIP || '-'}:${connection.metadata.destinationPort || '-'}`

const getRuleText = (connection: ConnectionItem) =>
  connection.rulePayload ? `${connection.rule} : ${connection.rulePayload}` : connection.rule || '-'

const getChainText = (connection: ConnectionItem) => connection.chains.join(' > ') || '-'

const getProcessText = (connection: ConnectionItem) =>
  connection.metadata.process || connection.metadata.processPath || '-'

const formatTimeAgo = (time: string) => {
  const diff = Date.now() - new Date(time).getTime()
  if (diff < 60_000) return t('connections.secondsAgo', { count: Math.max(1, Math.floor(diff / 1000)) })
  if (diff < 3_600_000) return t('connections.minutesAgo', { count: Math.floor(diff / 60_000) })
  if (diff < 86_400_000) return t('connections.hoursAgo', { count: Math.floor(diff / 3_600_000) })
  return new Date(time).toLocaleString()
}

watch(
  () => connectionStore.activeTab,
  () => {
    selectedConnection.value = null
  },
)
</script>

<style scoped>
.page-shell {
  display: flex;
  flex-direction: column;
  gap: var(--layout-page-gap, 16px);
  height: 100%;
}

.toolbar-card,
.table-card {
  background: var(--glass-bg);
  backdrop-filter: var(--glass-blur);
  border: 1px solid var(--border-light);
  border-radius: var(--radius-lg);
  padding: 20px;
  box-shadow: 0 4px 24px -6px rgba(0, 0, 0, 0.05);
}

.toolbar-row {
  display: grid;
  grid-template-columns: auto minmax(220px, 1fr) 180px 180px 180px auto;
  gap: 12px;
  align-items: center;
}

.search-input,
.source-select,
.sort-select {
  width: 100%;
}

.stats-row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin-top: 12px;
}

.connection-table-wrap {
  overflow-x: auto;
}

.connection-table {
  width: 100%;
  min-width: 1200px;
  border-collapse: collapse;
  table-layout: fixed;
}

.connection-table th {
  padding: 0 12px 10px;
  color: var(--text-tertiary);
  font-size: 12px;
  font-weight: 600;
  text-align: left;
  white-space: nowrap;
}

.connection-table td {
  padding: 12px;
  border-top: 1px solid var(--border-color);
  color: var(--text-secondary);
  vertical-align: middle;
}

.connection-table th:nth-child(1),
.connection-table td:nth-child(1) {
  width: 22%;
}

.connection-table th:nth-child(2),
.connection-table td:nth-child(2),
.connection-table th:nth-child(3),
.connection-table td:nth-child(3),
.connection-table th:nth-child(4),
.connection-table td:nth-child(4),
.connection-table th:nth-child(5),
.connection-table td:nth-child(5) {
  width: 9%;
}

.connection-table th:nth-child(6),
.connection-table td:nth-child(6),
.connection-table th:nth-child(7),
.connection-table td:nth-child(7),
.connection-table th:nth-child(8),
.connection-table td:nth-child(8) {
  width: 11.333%;
}

.connection-table th:nth-child(9),
.connection-table td:nth-child(9) {
  width: 8%;
}

.group-row td {
  padding: 14px 12px 8px;
  border-top: 0;
  background: transparent;
}

.group-title {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-weight: 600;
  color: var(--text-primary);
}

.connection-row {
  cursor: pointer;
  outline: none;
  transition: background-color 0.2s ease;
}

.connection-row:hover,
.connection-row:focus-visible {
  background: var(--hover-bg);
}

.destination-cell {
  min-width: 0;
}

.primary-cell,
.secondary-cell {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.primary-cell {
  color: var(--text-primary);
  font-weight: 600;
}

.secondary-cell {
  margin-top: 3px;
  color: var(--text-tertiary);
  font-size: 12px;
}

.truncate-cell {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.action-column,
.action-cell {
  text-align: right;
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
  font-size: 18px;
  font-weight: 600;
  margin: 0;
  color: var(--text-primary);
}

.detail-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.detail-grid div {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.detail-grid span {
  word-break: break-all;
}

@media (max-width: 960px) {
  .toolbar-row {
    grid-template-columns: 1fr;
  }

  .detail-grid {
    grid-template-columns: 1fr;
  }
}
</style>
