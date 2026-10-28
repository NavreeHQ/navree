async fn from_header(headers: HeaderMap) -> Response {
    // ruleid: navree-unwrap-on-request-data
    let agent = headers.get("user-agent").unwrap();
    // ruleid: navree-unwrap-on-request-data
    let id: u64 = headers.get("x-id").and_then(|v| v.to_str().ok()).expect("an id").parse().unwrap();
    Response::new(agent, id)
}

async fn from_path(Path(id): Path<String>) -> Response {
    // ruleid: navree-unwrap-on-request-data
    let n: u64 = id.parse().unwrap();
    Response::new(n)
}

async fn from_body(Valid(Json(input)): Valid<Json<Create>>) -> Response {
    // ruleid: navree-unwrap-on-request-data
    let name = input.name.expect("a name");
    Response::new(name)
}

async fn from_query(query: Query<Search>) -> Response {
    // ruleid: navree-unwrap-on-request-data
    let page = query.page.unwrap();
    Response::new(page)
}

impl Controller {
    async fn show(&self, uow: Uow, Path(id): Path<String>) -> Response {
        // ruleid: navree-unwrap-on-request-data
        let n: u64 = id.parse().unwrap();
        Response::new(n)
    }
}

async fn fine(headers: HeaderMap, Path(id): Path<String>) -> Result<Response, AppError> {
    // ok: navree-unwrap-on-request-data
    let agent = headers.get("user-agent").ok_or_else(|| AppError::bad_request("no agent"))?;
    // ok: navree-unwrap-on-request-data
    let n: u64 = id.parse().unwrap_or(0);
    // Not request data: a constant that cannot fail.
    // ok: navree-unwrap-on-request-data
    let re = Regex::new("^[a-z]+$").unwrap();
    // ok: navree-unwrap-on-request-data
    let guard = SHARED.lock().unwrap();
    Ok(Response::new(agent, n, re, guard))
}
