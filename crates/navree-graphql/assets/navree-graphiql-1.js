// Starts GraphiQL for the page it is loaded by: the endpoint and the
// subscriptions path come from data attributes on #graphiql, so the page
// needs no inline script. Bump the number in this file's name when it changes.
(function () {
  var root = document.getElementById('graphiql');
  var endpoint = new URL(root.getAttribute('data-endpoint'), window.location.href).toString();
  var ws = root.getAttribute('data-subscriptions');
  var wsUrl = ws === null || ws === '' ? undefined : (function () {
    var url = new URL(ws, window.location.href);
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
    return url.toString();
  })();
  // The Authorization header from GraphiQL's headers tab goes with every
  // query and, for subscriptions, in the connection_init payload.
  var fetcher = function (params, options) {
    var headers = (options && options.headers) || {};
    var authorization = headers.Authorization || headers.authorization;
    return GraphiQL.createFetcher({
      url: endpoint,
      subscriptionUrl: wsUrl,
      wsConnectionParams: authorization ? { authorization: authorization } : {},
    })(params, options);
  };
  ReactDOM.createRoot(root).render(
    React.createElement(GraphiQL, { fetcher: fetcher, defaultEditorToolsVisibility: true })
  );
})();
