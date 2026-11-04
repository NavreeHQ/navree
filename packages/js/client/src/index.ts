/**
 * `@navree/client`: a typed client for Navree APIs.
 *
 * - `createNavreeClient<paths>()`: the typed fetch core on the schema generated
 *   from the app's OpenAPI document, in cookie mode (behind a
 *   backend-for-frontend) or bearer mode, with `X-Workspace` and request ids;
 * - `NavreeError`: Navree's error shape with its `code`, `unwrap`;
 * - `listQuery()`, `checkListQuery()`: filters, sort, search and paging in the
 *   one list-query format a Navree list reads and writes;
 * - pages, query keys, operations (SSE) and locale helpers.
 *
 * Abilities (`@casl/ability`) are in `@navree/client/abilities`, real time
 * (`pusher-js`) in `@navree/client/realtime`.
 */
export { createNavreeClient, newRequestId, unwrap, type NavreeClient, type NavreeClientOptions, type WithQuery } from "./client.js";
export {
  defaultCode,
  errorMessage,
  isErrorBody,
  isNavreeError,
  NavreeError,
  reasonPhrase,
  type NavreeErrorBody,
} from "./errors.js";
export { direction, errorKey, isRtl, RTL_LANGUAGES, translateError, type Translate, type TranslateErrorOptions } from "./i18n.js";
export {
  checkListQuery,
  firstPage,
  fromSorting,
  isListParam,
  listQuery,
  ListQueryBuilder,
  listQuerySerializer,
  parseListQuery,
  serializeListQuery,
  setFilter,
  sortDirection,
  toggleSort,
  toSorting,
  withDefaults,
  withPage,
  type FilterCondition,
  type FilterOperator,
  type FilterScalar,
  type FilterValue,
  type ListQuery,
  type ListQueryCheck,
  type ListQueryProblem,
} from "./list-query.js";
export {
  cancelOperation,
  followOperation,
  getOperation,
  isFinished,
  readEventStream,
  type FollowOperationOptions,
  type Operation,
  type OperationFetch,
  type OperationProgress,
  type OperationStatus,
  type ServerSentEvent,
} from "./operations.js";
export {
  collectAll,
  facetCounts,
  hasNextPage,
  isCursorPage,
  isPage,
  pageRange,
  pages,
  type AnyPage,
  type CursorPage,
  type FacetCount,
  type Facets,
  type Page,
  type PageFetcher,
  type PageMeta,
} from "./pagination.js";
export { queryKeys, type ResourceKeys } from "./query-keys.js";
