import { invokeWithAppContext } from './invoke-client'

export interface SubscriptionPersistOptions {
  fileName?: string
  configPath?: string
  applyRuntime?: boolean
}

export interface SetActiveConfigOptions {
  useOriginalConfig?: boolean
  restartIfRunning?: boolean
}

interface BackendSubscriptionPersistResult {
  config_path: string
  config_changed: boolean
  subscription_upload?: number | null
  subscription_download?: number | null
  subscription_total?: number | null
  subscription_expire?: number | null
}

export interface SubscriptionPersistResult {
  configPath: string
  configChanged: boolean
  subscriptionUpload?: number
  subscriptionDownload?: number
  subscriptionTotal?: number
  subscriptionExpire?: number
}

const mapPersistResult = (result: BackendSubscriptionPersistResult): SubscriptionPersistResult => ({
  configPath: result.config_path,
  configChanged: result.config_changed,
  subscriptionUpload: result.subscription_upload ?? undefined,
  subscriptionDownload: result.subscription_download ?? undefined,
  subscriptionTotal: result.subscription_total ?? undefined,
  subscriptionExpire: result.subscription_expire ?? undefined,
})

export const subscriptionService = {
  downloadSubscription(url: string, useOriginalConfig: boolean, options: SubscriptionPersistOptions = {}) {
    return invokeWithAppContext<BackendSubscriptionPersistResult>(
      'download_subscription',
      {
        url,
        useOriginalConfig,
        fileName: options.fileName,
        configPath: options.configPath,
        applyRuntime: options.applyRuntime,
      },
      { withProxyPort: true, withApiPort: true },
    ).then(mapPersistResult)
  },

  addManualSubscription(content: string, useOriginalConfig: boolean, options: SubscriptionPersistOptions = {}) {
    return invokeWithAppContext<BackendSubscriptionPersistResult>(
      'add_manual_subscription',
      {
        content,
        useOriginalConfig,
        fileName: options.fileName,
        configPath: options.configPath,
        applyRuntime: options.applyRuntime,
      },
      { withProxyPort: true, withApiPort: true },
    ).then(mapPersistResult)
  },

  setActiveConfig(configPath: string | null, options: SetActiveConfigOptions = {}) {
    return invokeWithAppContext<void>(
      'set_active_config_path',
      {
        configPath,
        useOriginalConfig: options.useOriginalConfig,
        restartIfRunning: options.restartIfRunning,
      },
      undefined,
    )
  },

  deleteConfig(configPath: string) {
    return invokeWithAppContext<void>(
      'delete_subscription_config',
      { configPath },
      undefined,
    )
  },

  rollbackConfig(configPath: string) {
    return invokeWithAppContext<string>(
      'rollback_subscription_config',
      { configPath },
      undefined,
    )
  },

  getCurrentConfig() {
    return invokeWithAppContext<unknown>('get_current_config', undefined, { withApiPort: true })
  },
}
