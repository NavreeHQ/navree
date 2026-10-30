/**
 * The "Build AI into your product" examples. The code follows the AI docs
 * (content/docs/ai/*.mdx), shortened: the reference CRM's deals summary,
 * follow-up drafts, streamed summary, deals assistant, grounded answers and
 * MCP server.
 */

export type ProductSample = {
  id: string;
  /** The tab label. */
  label: string;
  /** One line under the label in the tab list. */
  hint: string;
  title: string;
  text: string;
  points: string[];
  href: string;
  linkText: string;
  file: string;
  code: string;
  /** A second, smaller block under the code (a response, a call site). */
  extra?: { file: string; lang: string; code: string };
};

export const productSamples: ProductSample[] = [
  {
    id: "text",
    label: "Text",
    hint: "ai.text(prompt)",
    title: "Text from any model, behind one service",
    text: "Inject `Arc<Ai>` and ask. The provider's API, keys, retries, failover and usage metrics are the SDK's job, so your code never imports a vendor client.",
    points: [
      "Anthropic, OpenAI, Google Gemini, Mistral, Ollama and AWS Bedrock",
      "Models named by their job, such as `fast`, in `config/app.toml`",
      "Failover to the next model when a provider is down",
    ],
    href: "/docs/ai/ai-sdk/#generating-text",
    linkText: "Generating text",
    file: "crates/crm-deals/src/controller.rs",
    code: `/// Two sentences on where the deal stands.
#[get("/{id}/summary")]
#[can(Read, Deal, from = "id")]
async fn summary(
    &self,
    uow: Uow,
    CurrentAccess(access): CurrentAccess,
    deal: Authorized<Deal, Read>,
) -> Result<Json<serde_json::Value>> {
    let deal = self.deals.show(&uow, deal.into_inner()).await?;
    let timeline = self.insights.timeline(&uow, &access, deal.id).await?;
    let summary = self
        .ai
        .text(format!("Deal \\"{}\\". Activity:\\n{timeline}", deal.title))
        .system("You summarize CRM deals for sales managers in two sentences.")
        .model("fast")
        .await?;
    Ok(Json(json!({ "summary": summary.text })))
}`,
    extra: {
      file: "config/app.toml",
      lang: "toml",
      code: `[ai.models.fast]
provider = "anthropic"
model = "claude-sonnet-5-5"
fallbacks = ["backup"]

[ai.models.backup]
provider = "openai"
model = "gpt-5"`,
    },
  },
  {
    id: "structured",
    label: "Structured output",
    hint: "ai.structured::<T>()",
    title: "Ask for a type, get a checked value",
    text: "`ai.structured::<T>` sends the JSON Schema of `T` from the same `ToSchema` derive your OpenAPI document uses, deserializes the answer and, with `.validated()`, checks it against its validation rules.",
    points: [
      "Doc comments become field descriptions the model reads",
      "Invalid answers go back to the model to repair, up to a limit",
      "Enums make classification a type",
    ],
    href: "/docs/ai/ai-sdk/#structured-output",
    linkText: "Structured output",
    file: "crates/crm-deals/src/insights.rs",
    code: `/// A follow-up email the rep can edit before sending.
#[derive(Debug, Deserialize, Serialize, ToSchema, Validate)]
pub struct FollowUpEmail {
    /// The subject line, without a prefix like "Re:".
    #[validate(length(min = 1, max = 80))]
    pub subject: String,
    /// The plain-text body, signed with the rep's first name.
    #[validate(length(min = 1, max = 2000))]
    pub body: String,
    /// The day to send it, when the activity suggests one.
    #[validate(skip)]
    pub send_on: Option<NaiveDate>,
}

let email = self
    .ai
    .structured::<FollowUpEmail>(format!("Write a follow-up from {rep} about \\"{}\\".\\n\\n{timeline}", deal.title))
    .system("You draft short, specific sales follow-ups. No filler.")
    .model("fast")
    .validated()
    .await?;`,
  },
  {
    id: "streaming",
    label: "Streaming",
    hint: "ai.stream(prompt)",
    title: "Words on screen from the first half second",
    text: "`ai.stream` returns a `TextStream`, and a `TextStream` is a response: return it from a handler and the browser receives server-sent events. For work that outlives the request, stream from a queued job over real time.",
    points: [
      "Errors after the first event arrive as Navree's JSON error",
      "A client that disconnects stops the generation and its cost",
      "`on_finish` stores what the user saw",
    ],
    href: "/docs/ai/ai-sdk/#streaming",
    linkText: "Streaming",
    file: "crates/crm-deals/src/controller.rs",
    code: `use navree::ai::TextStream;

/// Streams a summary of the deal as server-sent events.
#[get("/{id}/summary/stream")]
#[can(Read, Deal, from = "id")]
async fn summary_stream(
    &self,
    uow: Uow,
    CurrentAccess(access): CurrentAccess,
    deal: Authorized<Deal, Read>,
) -> Result<TextStream> {
    let deal = self.deals.show(&uow, deal.into_inner()).await?;
    let timeline = self.insights.timeline(&uow, &access, deal.id).await?;
    Ok(self
        .ai
        .stream(format!("Summarize the deal \\"{}\\":\\n{timeline}", deal.title))
        .system("You summarize CRM deals in at most two sentences.")
        .model("fast")
        .await?)
}`,
    extra: {
      file: "text/event-stream",
      lang: "text",
      code: `event: text
data: {"text":"Acme moved to negotiation"}

event: text
data: {"text":" after Tuesday's pricing call."}

event: done
data: {"model":"fast","usage":{"input_tokens":912,"output_tokens":41},"finish":"stop"}`,
    },
  },
  {
    id: "agents",
    label: "Agents and tools",
    hint: "#[agent], #[tool]",
    title: "Agents that act as the user, through your services",
    text: "An agent is an injectable struct with `#[tool]` methods. Each tool calls your own services with the signed-in user's access, so the model can never see or change more than the user could.",
    points: [
      "Doc comments are tool descriptions; arguments are validated DTOs",
      "Access rules on tools, approvals for risky actions",
      "Conversation memory, streaming runs and runs in queued jobs",
    ],
    href: "/docs/ai/agents-and-tools/",
    linkText: "Agents and tools",
    file: "crates/crm-deals/src/assistant.rs",
    code: `/// The deals assistant in the CRM's chat panel.
#[injectable]
pub struct DealsAssistant {
    deals: Arc<DealService>,
}

#[agent]
impl Agent for DealsAssistant {
    const NAME: &'static str = "deals-assistant";

    fn instructions(&self, run: &AgentRun) -> String {
        format!("You are the sales assistant of a small CRM team, talking to {}.", run.user().name)
    }

    /// Finds the deals the user can see, newest first. Returns at most 20.
    #[tool(read_only)]
    async fn search_deals(&self, run: &AgentRun, args: SearchDeals) -> Result<Vec<DealResource>> {
        let filter = DealFilter { search: args.query, stage: args.stage, ..DealFilter::default() };
        let page = PageQuery { per_page: 20, ..PageQuery::default() };
        let deals = self.deals.list(run.uow(), run.accessible::<Deal, Read>(), filter, page).await?;
        Ok(deals.data)
    }
}`,
    extra: {
      file: "crates/crm-deals/src/assistant_controller.rs",
      lang: "rust",
      code: `let reply = self
    .ai
    .agent::<DealsAssistant>()
    .acting_as(access)
    .conversation(input.conversation_id)
    .prompt(input.message)
    .await?;`,
    },
  },
  {
    id: "rag",
    label: "Embeddings and RAG",
    hint: "VectorStore, rerank",
    title: "Answers from your own data, with sources",
    text: "Index records through the queue into pgvector or Qdrant, search them with metadata filters, rerank, and ask for a structured answer that cites its sources by number.",
    points: [
      "`Embeddings` with a cache, `Chunker` for long text",
      "Filters run inside the vector search, so permissions hold",
      "Indexes kept in sync by a queued job",
    ],
    href: "/docs/ai/embeddings-and-rag/",
    linkText: "Embeddings and RAG",
    file: "crates/crm-deals/src/ask.rs",
    code: `pub async fn ask(&self, deal: &DealResource, question: &str) -> Result<GroundedAnswer> {
    let candidates = self
        .vectors
        .collection("activity_notes")
        .search(question)
        .filter(Filter::eq("subject_id", deal.id))
        .limit(25)
        .await?;
    let passages = self.ai.rerank(question, candidates).model("fast").top(6).await?;

    let numbered = passages
        .iter()
        .enumerate()
        .map(|(i, p)| format!("[{}] {}", i + 1, p.text))
        .collect::<Vec<_>>()
        .join("\\n\\n");
    Ok(self
        .ai
        .structured::<GroundedAnswer>(format!("Sources:\\n{numbered}\\n\\nQuestion: {question}"))
        .system("Answer using only the numbered sources, and cite the ones you use.")
        .model("smart")
        .validated()
        .await?)
}`,
  },
  {
    id: "mcp",
    label: "MCP servers",
    hint: "#[mcp_server]",
    title: "Your app's tools in any AI client",
    text: "Mark an injectable's methods `#[mcp_tool]`, `#[mcp_resource]` or `#[mcp_prompt]` and your app serves them over MCP, so people use your product from Claude, ChatGPT or Cursor as themselves.",
    points: [
      "Streamable HTTP and stdio",
      "Sign-in through Navree auth or OAuth 2.1",
      "Per-tool permissions with the same access rules as your routes",
    ],
    href: "/docs/ai/mcp-servers/",
    linkText: "MCP servers",
    file: "crates/crm-deals/src/mcp.rs",
    code: `/// The CRM's MCP server: deals and their activity.
#[injectable]
pub struct CrmMcp {
    deals: Arc<DealService>,
}

#[mcp_server]
impl CrmMcp {
    /// Lists the deals the user can see, newest first. Returns at most 25.
    #[mcp_tool(read_only)]
    #[can(Read, Deal)]
    async fn list_deals(&self, cx: &McpContext, args: ListDeals) -> Result<Vec<DealResource>> {
        let filter = DealFilter { stage: args.stage, search: args.query, ..DealFilter::default() };
        let page = PageQuery { per_page: 25, ..PageQuery::default() };
        Ok(self.deals.list(cx.uow(), cx.accessible::<Deal, Read>(), filter, page).await?.data)
    }

    /// Moves a deal to another stage. Moving to \`lost\` needs a reason.
    #[mcp_tool(destructive)]
    #[can(Update, Deal)]
    async fn move_deal(&self, cx: &McpContext, args: MoveDealArgs) -> Result<DealResource> {
        let deal = cx.authorize::<Deal, Move>(args.deal_id).await?;
        let input = MoveDeal { stage: Some(args.stage), position: None, lost_reason: args.lost_reason };
        self.deals.move_to(cx.uow(), cx.user_id(), deal.into_inner(), input).await
    }
}`,
  },
];

/** The providers `navree-ai` speaks to (spec §5). */
export const providers = ["Anthropic", "OpenAI", "Google Gemini", "Mistral", "Ollama", "AWS Bedrock"];
