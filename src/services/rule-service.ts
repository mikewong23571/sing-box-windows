import type { RuleProvidersResponse, RulesResponse } from '@/types/controller'
import type {
  CustomRule,
  CustomRuleAction,
  CustomRuleMatchType,
} from '@/types/generated'
import { invokeWithAppContext } from './invoke-client'

export const ruleService = {
  getRules(port?: number) {
    const args = typeof port === 'number' ? { port } : undefined
    return invokeWithAppContext<RulesResponse>('get_rules', args, {
      withApiPort: typeof port === 'number' ? undefined : 'port',
    })
  },

  getProviders(port?: number) {
    const args = typeof port === 'number' ? { port } : undefined
    return invokeWithAppContext<RuleProvidersResponse>('get_rule_providers', args, {
      withApiPort: typeof port === 'number' ? undefined : 'port',
    })
  },

  updateProvider(provider: string, port?: number) {
    const args = typeof port === 'number' ? { provider, port } : { provider }
    return invokeWithAppContext<void>('update_rule_provider', args, {
      withApiPort: typeof port === 'number' ? undefined : 'port',
    })
  },

  toggleDisabled(index: number, disabled: boolean, port?: number) {
    const args = typeof port === 'number' ? { index, disabled, port } : { index, disabled }
    return invokeWithAppContext<void>('toggle_rule_disabled', args, {
      withApiPort: typeof port === 'number' ? undefined : 'port',
    })
  },

  // 自定义规则 CRUD（issue #62）：持久化在本地，写入活动 sing-box 配置，重启内核生效。
  listCustomRules() {
    return invokeWithAppContext<CustomRule[]>('list_custom_rules')
  },

  addCustomRule(
    matchType: CustomRuleMatchType,
    payload: string,
    action: CustomRuleAction,
    note?: string,
  ) {
    return invokeWithAppContext<CustomRule>('add_custom_rule', {
      matchType,
      payload,
      action,
      note: note ?? null,
    })
  },

  updateCustomRule(
    id: string,
    matchType: CustomRuleMatchType,
    payload: string,
    action: CustomRuleAction,
    note?: string,
  ) {
    return invokeWithAppContext<void>('update_custom_rule', {
      id,
      matchType,
      payload,
      action,
      note: note ?? null,
    })
  },

  deleteCustomRule(id: string) {
    return invokeWithAppContext<void>('delete_custom_rule', { id })
  },

  toggleCustomRule(id: string) {
    return invokeWithAppContext<void>('toggle_custom_rule', { id })
  },
}
