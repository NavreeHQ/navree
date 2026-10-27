/** The public runtime settings, typed (NUXT_PUBLIC_* environment variables override them at runtime). */
export function useCrmConfig() {
  const config = useRuntimeConfig().public;
  return {
    mocks: Boolean(config.apiMocks),
    realtime: Boolean(config.realtime),
    pusherKey: String(config.pusherKey),
    wsHost: String(config.wsHost),
    wsPort: Number(config.wsPort),
    demoAccounts: Boolean(config.demoAccounts) || Boolean(config.apiMocks),
  };
}
