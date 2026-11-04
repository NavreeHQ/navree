/**
 * The public runtime settings, typed (NUXT_PUBLIC_* environment variables override them at runtime).
 * The socket's settings are the `@navree/nuxt` module's: `public.navree.realtime` is `false` when it is off.
 */
export function useCrmConfig() {
  const config = useRuntimeConfig().public;
  const navree = config.navree as { realtime?: { wsHost: string; wsPort: number } | false } | undefined;
  const realtime = navree?.realtime;
  return {
    mocks: Boolean(config.apiMocks),
    realtime: realtime ? { wsHost: String(realtime.wsHost), wsPort: Number(realtime.wsPort) } : null,
    demoAccounts: Boolean(config.demoAccounts) || Boolean(config.apiMocks),
  };
}
