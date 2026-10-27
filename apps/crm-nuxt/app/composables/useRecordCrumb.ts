import { type MaybeRefOrGetter, toValue } from "vue";
import { useUiStore } from "@/stores/ui";

/**
 * Puts a detail screen's record name in the top bar's breadcrumb (and the
 * tab title), e.g. Deals › Pilot extension. Cleared when the screen goes.
 */
export function useRecordCrumb(title: MaybeRefOrGetter<string | undefined | null>) {
  const ui = useUiStore();
  const route = useRoute();
  const path = route.path;
  watchEffect(() => {
    const value = toValue(title);
    ui.record = value ? { path, title: value } : null;
  });
  onBeforeUnmount(() => {
    if (ui.record?.path === path) ui.record = null;
  });
}
