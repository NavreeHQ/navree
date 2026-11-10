/**
 * `@navree/nuxt/vue`: the Vue side of the Nuxt module, usable in any Vue 3
 * app: sign-in state, the workspace, operations, the route's list query,
 * the data table and form errors. Abilities are in `@navree/nuxt/abilities`
 * (`@casl/vue`), real time in `@navree/nuxt/realtime` (`pusher-js`).
 */
export { createNavreeAuth, type AuthStatus, type NavreeAuth, type NavreeAuthOptions, type SignInResult } from "./auth.js";
export {
  defaultLabels,
  NavreeDataTable,
  NavreePagination,
  useDataTable,
  type DataTableBulk,
  type DataTableBulkAction,
  type DataTableLabels,
  type NavreeColumnDef,
  type NavreeColumnMeta,
  type NavreeTable,
  type NavreeTableFeatures,
  type UseDataTableOptions,
} from "./data-table.js";
export { applyFieldErrors, type ApplyFieldErrorsOptions } from "./forms.js";
export { isListParam, mergeListQuery, useListQuery, type ListQueryState } from "./list-query.js";
export { useOperation, type UseOperationOptions, type UseOperationResult } from "./operation.js";
export { createWorkspace, WORKSPACE_COOKIE, type Workspace } from "./workspace.js";
export { direction, errorKey, errorMessage, isRtl, localeAttributes, translateError, type Translate } from "@navree/client";
