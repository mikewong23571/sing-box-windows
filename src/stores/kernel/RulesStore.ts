import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { ruleService } from '@/services/rule-service'
import type { RuleItem, RuleProvider } from '@/types/controller'
import type {
  CustomRule,
  CustomRuleAction,
  CustomRuleMatchType,
} from '@/types/generated'

const normalizeRules = (input: RuleItem[] | Record<string, RuleItem>) => {
  if (Array.isArray(input)) {
    return input.map((rule, index) => ({
      ...rule,
      index: typeof rule.index === 'number' ? rule.index : index,
    }))
  }

  return Object.entries(input).map(([key, rule], index) => ({
    ...rule,
    index: typeof rule.index === 'number' ? rule.index : Number(key) || index,
  }))
}

export const useRulesStore = defineStore('rules', () => {
  const loading = ref(false)
  const rules = ref<RuleItem[]>([])
  const providers = ref<RuleProvider[]>([])
  const providerUpdatingMap = ref<Record<string, boolean>>({})
  const ruleUpdatingMap = ref<Record<number, boolean>>({})

  // 自定义规则（持久化在本地，区别于内核运行时 rules）。
  const customRules = ref<CustomRule[]>([])
  const customRuleUpdating = ref<Record<string, boolean>>({})

  const fetchAll = async () => {
    loading.value = true
    try {
      const [rulesResponse, providersResponse, customResponse] = await Promise.all([
        ruleService.getRules(),
        ruleService.getProviders(),
        ruleService.listCustomRules().catch(() => [] as CustomRule[]),
      ])

      rules.value = normalizeRules(rulesResponse.rules)
      providers.value = Object.values(providersResponse.providers || {})
      customRules.value = customResponse
    } finally {
      loading.value = false
    }
  }

  const fetchCustomRules = async () => {
    customRules.value = await ruleService.listCustomRules()
  }

  const addCustomRule = async (
    matchType: CustomRuleMatchType,
    payload: string,
    action: CustomRuleAction,
    note?: string,
  ) => {
    await ruleService.addCustomRule(matchType, payload, action, note)
    await fetchCustomRules()
  }

  const updateCustomRule = async (
    id: string,
    matchType: CustomRuleMatchType,
    payload: string,
    action: CustomRuleAction,
    note?: string,
  ) => {
    customRuleUpdating.value = { ...customRuleUpdating.value, [id]: true }
    try {
      await ruleService.updateCustomRule(id, matchType, payload, action, note)
      await fetchCustomRules()
    } finally {
      customRuleUpdating.value = { ...customRuleUpdating.value, [id]: false }
    }
  }

  const deleteCustomRule = async (id: string) => {
    customRuleUpdating.value = { ...customRuleUpdating.value, [id]: true }
    try {
      await ruleService.deleteCustomRule(id)
      await fetchCustomRules()
    } finally {
      customRuleUpdating.value = { ...customRuleUpdating.value, [id]: false }
    }
  }

  const toggleCustomRule = async (id: string) => {
    customRuleUpdating.value = { ...customRuleUpdating.value, [id]: true }
    try {
      await ruleService.toggleCustomRule(id)
      await fetchCustomRules()
    } finally {
      customRuleUpdating.value = { ...customRuleUpdating.value, [id]: false }
    }
  }

  const updateProvider = async (providerName: string) => {
    providerUpdatingMap.value = {
      ...providerUpdatingMap.value,
      [providerName]: true,
    }

    try {
      await ruleService.updateProvider(providerName)
      await fetchAll()
    } finally {
      providerUpdatingMap.value = {
        ...providerUpdatingMap.value,
        [providerName]: false,
      }
    }
  }

  const updateAllProviders = async () => {
    await Promise.all(providers.value.map((provider) => updateProvider(provider.name)))
  }

  const toggleDisabled = async (rule: RuleItem) => {
    if (typeof rule.index !== 'number') return

    ruleUpdatingMap.value = {
      ...ruleUpdatingMap.value,
      [rule.index]: true,
    }

    try {
      await ruleService.toggleDisabled(rule.index, !rule.extra?.disabled)
      rules.value = rules.value.map((item) =>
        item.index === rule.index
          ? {
              ...item,
              extra: {
                ...item.extra,
                disabled: !item.extra?.disabled,
              },
            }
          : item,
      )
    } finally {
      ruleUpdatingMap.value = {
        ...ruleUpdatingMap.value,
        [rule.index]: false,
      }
    }
  }

  const ruleTypes = computed(() => Array.from(new Set(rules.value.map((rule) => rule.type))))

  return {
    loading,
    rules,
    providers,
    providerUpdatingMap,
    ruleUpdatingMap,
    customRules,
    customRuleUpdating,
    ruleTypes,
    fetchAll,
    fetchCustomRules,
    updateProvider,
    updateAllProviders,
    toggleDisabled,
    addCustomRule,
    updateCustomRule,
    deleteCustomRule,
    toggleCustomRule,
  }
})
