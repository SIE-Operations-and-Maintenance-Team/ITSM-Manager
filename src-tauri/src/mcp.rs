// MCP 边界层：对外暴露 10 个工单 tools（6 只读 + 4 写），复用 api.rs HTTP 实现，不含新 ITSM endpoint
use crate::api::{self, FetchError, SearchParams, AUTH_EXPIRED_ERR};
use crate::state::TokenStore;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_router, ErrorData as McpError,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// MCP handler：持有共享 token + reqwest::Client，可被 axum 共享层 clone
#[derive(Clone)]
pub struct ItsmHandler {
    token: TokenStore,
    client: reqwest::Client,
    default_seach_type: i64,
}

impl ItsmHandler {
    pub fn new(token: TokenStore, client: reqwest::Client, default_seach_type: i64) -> Self {
        Self { token, client, default_seach_type }
    }

    fn token(&self) -> Result<String, McpError> {
        self.token
            .get()
            .map_err(|message| McpError::internal_error(message, None))
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchByCodeParams {
    #[schemars(description = "视图 seachType；省略时用设置中的 MCP 缺省视图（默认 7=所有工单）。可先调用 list_views 获取")]
    seach_type: Option<i64>,
    #[schemars(description = "工单号或主题关键字；按 codeAndSubject 模糊匹配")]
    keyword: String,
    #[schemars(description = "页码，从 1 开始；省略时为 1")]
    page_index: Option<i64>,
    #[schemars(description = "每页条数，范围 1..=200；省略时为 50")]
    page_size: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchByCustomerGroupParams {
    #[schemars(description = "视图 seachType；省略时用设置中的 MCP 缺省视图（默认 7=所有工单）")]
    seach_type: Option<i64>,
    #[schemars(description = "客户组名称关键字；按 contactCustomerGroupName 模糊匹配")]
    keyword: String,
    #[schemars(description = "页码，从 1 开始；省略时为 1")]
    page_index: Option<i64>,
    #[schemars(description = "每页条数，范围 1..=200；省略时为 50")]
    page_size: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetDetailParams {
    #[schemars(description = "工单 incidentId，不是展示用 incidentCode")]
    id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListRepliesParams {
    #[schemars(description = "工单 incidentId")]
    incident_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetTicketByCodeParams {
    #[schemars(description = "工单展示单号 incidentCode，如 IM26070065")]
    code: String,
    #[schemars(description = "视图 seachType；省略时用设置中的 MCP 缺省视图")]
    seach_type: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReplyParams {
    #[schemars(description = "工单 incidentId")]
    order_id: String,
    #[schemars(description = "回复内容。建议用 HTML 格式以保留排版：换行用 <br>，段落用 <p>，重点用 <strong>，列表用 <ul>/<li>，表格用 <table>；避免依赖纯文本换行")]
    detail: String,
    #[schemars(description = "true 表示内部备注，false 表示公开回复")]
    is_private: bool,
    #[schemars(description = "ITSM 工单类型；默认 \"1\"（与工单 orderType 字段一致）。其他取值需向业务确认")]
    order_type: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SuspendParams {
    #[schemars(description = "工单 incidentId")]
    id: String,
    #[schemars(description = "暂挂原因")]
    reason: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UnhangParams {
    #[schemars(description = "工单 incidentId")]
    id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ResolveParams {
    #[schemars(description = "工单 incidentId")]
    id: String,
    #[schemars(description = "解决方案；不能为空")]
    solution: String,
}

fn api_error(message: impl Into<String>) -> McpError {
    McpError::internal_error(message.into(), None)
}

fn fetch_error(error: FetchError) -> McpError {
    match error {
        FetchError::Network(message) | FetchError::Server(message) => api_error(message),
        FetchError::Auth => api_error(AUTH_EXPIRED_ERR),
    }
}

fn json_result(value: Value) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(value.to_string())])
}

/// get_detail 返回精简：从 get-with-fields 的 data 中只保留 agent 常用核心字段，
/// 丢弃四套 *Fields 表单模板与 extField1-35。
/// 注意：写操作（resolve/change_status）依赖完整 data 拼 update body，故裁剪只发生在
/// 此 MCP 输出层，api.rs 不得改动。
fn pick_detail_fields(v: &Value) -> Value {
    const KEEP: &[&str] = &[
        "incidentId", "incidentCode", "orderSubject", "detail", "status", "statusName",
        "priority", "priorityName", "effect", "effectName", "urgency",
        "supportBy", "supportName", "requestor", "requestorName", "assign", "assignName",
        "serviceFullName", "serviceTypeName", "serviceSubTypeName",
        "incidentType", "incidentTypeName", "incidentSource", "incidentSourceName",
        "contactCustomerGroup", "contactCustomerGroupName",
        "creationDate", "lastUpdateDate", "firstResponseTime", "hopeResolvedTime",
        "resolvedTime", "closeTime", "solution", "orderType", "tenantId", "phone", "email",
    ];
    let obj = match v.get("data").and_then(|d| d.as_object()) {
        Some(o) => o,
        None => return json!({}),
    };
    let mut m = serde_json::Map::new();
    for &k in KEEP {
        if let Some(val) = obj.get(k) {
            m.insert(k.to_string(), val.clone());
        }
    }
    Value::Object(m)
}

fn required_text(field: &str, value: String) -> Result<String, McpError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(McpError::invalid_params(format!("{field} 不能为空"), None));
    }
    Ok(value)
}

fn pagination(page_index: Option<i64>, page_size: Option<i64>) -> Result<(i64, i64), McpError> {
    let page_index = page_index.unwrap_or(1);
    let page_size = page_size.unwrap_or(50);
    if page_index < 1 {
        return Err(McpError::invalid_params("page_index 必须 >= 1", None));
    }
    if !(1..=200).contains(&page_size) {
        return Err(McpError::invalid_params("page_size 必须在 1..=200", None));
    }
    Ok((page_index, page_size))
}

#[tool_router(server_handler)]
impl ItsmHandler {
    #[tool(
        description = "列出当前登录账号可用的工单视图。先调用本工具取得每个视图的 seachType，再调用搜索工具。",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn list_views(&self) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let value = api::list_views(&self.client, &token)
            .await
            .map_err(api_error)?;
        Ok(json_result(value))
    }

    #[tool(
        description = "按工单号或主题关键字，在指定 seachType 视图中模糊搜索工单。返回 data、count、page_index、page_size。",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn search_tickets_by_code(
        &self,
        Parameters(params): Parameters<SearchByCodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let keyword = required_text("keyword", params.keyword)?;
        let (page_index, page_size) = pagination(params.page_index, params.page_size)?;
        let seach_type = params.seach_type.unwrap_or(self.default_seach_type);
        let search = SearchParams {
            code_and_subject: Some(keyword),
            ..Default::default()
        };
        let (data, count) = api::fetch_tickets_raw(
            &self.client,
            &token,
            seach_type,
            page_index,
            page_size,
            Some(&search),
        )
        .await
        .map_err(fetch_error)?;
        Ok(json_result(json!({
            "data": data,
            "count": count,
            "seach_type": seach_type,
            "page_index": page_index,
            "page_size": page_size,
        })))
    }

    #[tool(
        description = "按客户组名称关键字，在指定 seachType 视图中模糊搜索工单。返回 data、count、page_index、page_size。",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn search_tickets_by_customer_group(
        &self,
        Parameters(params): Parameters<SearchByCustomerGroupParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let keyword = required_text("keyword", params.keyword)?;
        let (page_index, page_size) = pagination(params.page_index, params.page_size)?;
        let seach_type = params.seach_type.unwrap_or(self.default_seach_type);
        let search = SearchParams {
            contact_customer_group_name: Some(keyword),
            ..Default::default()
        };
        let (data, count) = api::fetch_tickets_raw(
            &self.client,
            &token,
            seach_type,
            page_index,
            page_size,
            Some(&search),
        )
        .await
        .map_err(fetch_error)?;
        Ok(json_result(json!({
            "data": data,
            "count": count,
            "seach_type": seach_type,
            "page_index": page_index,
            "page_size": page_size,
        })))
    }

    #[tool(
        description = "按 incidentId 读取工单核心详情（精简字段，不含表单模板）。参数 id 必须是 incidentId，不是展示单号；展示单号请用 get_ticket_by_code。",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn get_detail(
        &self,
        Parameters(params): Parameters<GetDetailParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let id = required_text("id", params.id)?;
        let value = api::get_detail(&self.client, &token, &id)
            .await
            .map_err(api_error)?;
        Ok(json_result(pick_detail_fields(&value)))
    }

    #[tool(
        description = "按 incidentId 列出工单的历史回复轨迹（含回复人、时间、内容、是否内部备注）。",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn list_replies(
        &self,
        Parameters(params): Parameters<ListRepliesParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let incident_id = required_text("incident_id", params.incident_id)?;
        let value = api::list_replies(&self.client, &token, &incident_id)
            .await
            .map_err(api_error)?;
        let replies = value.get("data").cloned().unwrap_or(Value::Array(vec![]));
        let count = replies.as_array().map(|a| a.len()).unwrap_or(0);
        Ok(json_result(json!({ "replies": replies, "count": count })))
    }

    #[tool(
        description = "按展示单号(incidentCode，如 IM26070065)一步返回工单核心详情与历史回复。内部：缺省视图搜 code → incidentId → 并发取详情与回复。命中多条时取首条并附 hint。",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn get_ticket_by_code(
        &self,
        Parameters(params): Parameters<GetTicketByCodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let code = required_text("code", params.code)?;
        let seach_type = params.seach_type.unwrap_or(self.default_seach_type);
        let search = SearchParams {
            code_and_subject: Some(code.clone()),
            ..Default::default()
        };
        let (data, count) = api::fetch_tickets_raw(
            &self.client,
            &token,
            seach_type,
            1,
            1,
            Some(&search),
        )
        .await
        .map_err(fetch_error)?;
        let arr = data.as_array().ok_or_else(|| api_error("搜索响应非数组"))?;
        if arr.is_empty() {
            return Err(McpError::invalid_params(
                format!("未找到单号为 {code} 的工单"),
                None,
            ));
        }
        let first = &arr[0];
        let incident_id = first
            .get("incidentId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| api_error("搜索结果缺 incidentId"))?
            .to_string();
        let incident_code = first.get("incidentCode").cloned().unwrap_or(Value::Null);
        // 并发取详情与回复（reqwest::Client 支持同一 client 并发请求）
        let (detail_res, replies_res) = tokio::join!(
            api::get_detail(&self.client, &token, &incident_id),
            api::list_replies(&self.client, &token, &incident_id),
        );
        let detail = pick_detail_fields(&detail_res.map_err(api_error)?);
        let replies = replies_res
            .map_err(api_error)?
            .get("data")
            .cloned()
            .unwrap_or(Value::Array(vec![]));
        let mut result = serde_json::Map::new();
        result.insert("count".into(), json!(count));
        result.insert("incident_id".into(), json!(incident_id));
        result.insert("incident_code".into(), incident_code);
        result.insert("detail".into(), detail);
        result.insert("replies".into(), replies);
        if count > 1 {
            result.insert(
                "hint".into(),
                Value::String(format!("命中 {count} 条，已取首条；如非目标请用更完整的单号")),
            );
        }
        Ok(json_result(Value::Object(result)))
    }

    #[tool(
        description = "回复工单。本工具只追加回复，不改工单状态；如需变更状态请用 resolve / unhang 等。首版不上传附件，fileIds 固定为空；本操作会修改真实 ITSM 工单。",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn reply(
        &self,
        Parameters(params): Parameters<ReplyParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let order_id = required_text("order_id", params.order_id)?;
        let detail = required_text("detail", params.detail)?;
        let order_type = match params.order_type {
            Some(s) => required_text("order_type", s)?,
            None => "1".to_string(),
        };
        let value = api::reply(
            &self.client,
            &token,
            &order_id,
            &detail,
            &[],
            params.is_private,
            &order_type,
        )
        .await
        .map_err(api_error)?;
        Ok(json_result(value))
    }

    #[tool(
        description = "暂挂工单并记录暂挂原因；本操作会修改真实 ITSM 工单。",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn suspend(
        &self,
        Parameters(params): Parameters<SuspendParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let id = required_text("id", params.id)?;
        let reason = required_text("reason", params.reason)?;
        let value = api::suspend_or_unhang(&self.client, &token, &id, "suspend", &reason)
            .await
            .map_err(api_error)?;
        Ok(json_result(value))
    }

    #[tool(
        description = "解除工单暂挂；本操作会修改真实 ITSM 工单。",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn unhang(
        &self,
        Parameters(params): Parameters<UnhangParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let id = required_text("id", params.id)?;
        let value = api::suspend_or_unhang(&self.client, &token, &id, "unhang", "")
            .await
            .map_err(api_error)?;
        Ok(json_result(value))
    }

    #[tool(
        description = "将工单状态改为 Resolved 并写入解决方案；本操作会修改真实 ITSM 工单。",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    async fn resolve(
        &self,
        Parameters(params): Parameters<ResolveParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let id = required_text("id", params.id)?;
        let solution = required_text("solution", params.solution)?;
        let value = api::change_status(&self.client, &token, &id, "Resolved", &solution, true)
            .await
            .map_err(api_error)?;
        Ok(json_result(value))
    }
}

use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};

fn build_router(token: TokenStore, client: reqwest::Client, default_seach_type: i64) -> axum::Router {
    let config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None);
    let service: StreamableHttpService<ItsmHandler, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(ItsmHandler::new(token.clone(), client.clone(), default_seach_type)),
            Default::default(),
            config,
        );
    axum::Router::new().nest_service("/mcp", service)
}

pub async fn serve(
    token: TokenStore,
    client: reqwest::Client,
    port: u16,
    default_seach_type: i64,
) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|error| format!("绑定 127.0.0.1:{port} 失败: {error}"))?;
    println!("[mcp] listening on http://127.0.0.1:{port}/mcp");
    axum::serve(listener, build_router(token, client, default_seach_type))
        .await
        .map_err(|error| format!("MCP server 退出: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> ItsmHandler {
        ItsmHandler::new(TokenStore::default(), reqwest::Client::new(), 7)
    }

    #[test]
    fn exposes_exactly_ten_tools() {
        let tools = ItsmHandler::tool_router().list_all();
        let mut names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "get_detail",
                "get_ticket_by_code",
                "list_replies",
                "list_views",
                "reply",
                "resolve",
                "search_tickets_by_code",
                "search_tickets_by_customer_group",
                "suspend",
                "unhang",
            ]
        );

        for tool in &tools {
            let annotations = tool.annotations.as_ref().unwrap();
            let is_write = matches!(
                tool.name.as_ref(),
                "reply" | "resolve" | "suspend" | "unhang"
            );
            assert_eq!(annotations.read_only_hint, Some(!is_write), "{}", tool.name);
            assert_eq!(
                annotations.destructive_hint,
                Some(is_write),
                "{}",
                tool.name
            );
        }
    }

    #[test]
    fn pagination_defaults_and_validates() {
        assert_eq!(pagination(None, None).unwrap(), (1, 50));
        assert_eq!(pagination(Some(2), Some(100)).unwrap(), (2, 100));
        assert!(pagination(Some(0), Some(50)).is_err());
        assert!(pagination(Some(1), Some(0)).is_err());
        assert!(pagination(Some(1), Some(201)).is_err());
    }

    #[test]
    fn required_text_trims_and_rejects_blank() {
        assert_eq!(required_text("id", " A-1 ".into()).unwrap(), "A-1");
        assert!(required_text("id", "   ".into()).is_err());
    }

    #[tokio::test]
    async fn list_views_without_login_returns_mcp_error() {
        let err = handler().list_views().await.unwrap_err();
        assert_eq!(err.message, "未登录");
    }

    #[tokio::test]
    async fn reply_without_login_stops_before_network() {
        let err = handler()
            .reply(Parameters(ReplyParams {
                order_id: "OID".into(),
                detail: "test".into(),
                is_private: false,
                order_type: Some("1".into()),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.message, "未登录");
    }

    #[tokio::test]
    async fn list_replies_without_login_returns_mcp_error() {
        let err = handler()
            .list_replies(Parameters(ListRepliesParams {
                incident_id: "X".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.message, "未登录");
    }

    #[tokio::test]
    async fn get_ticket_by_code_without_login_stops_before_network() {
        let err = handler()
            .get_ticket_by_code(Parameters(GetTicketByCodeParams {
                code: "IM1".into(),
                seach_type: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.message, "未登录");
    }

    async fn spawn_test_server() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = build_router(TokenStore::default(), reqwest::Client::new(), 7);
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{addr}/mcp"), task)
    }

    async fn post_rpc(
        client: &reqwest::Client,
        url: &str,
        body: Value,
        protocol_version: Option<&str>,
    ) -> Value {
        let mut request = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(version) = protocol_version {
            request = request.header("MCP-Protocol-Version", version);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), 200);
        let content_type = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(content_type.contains("application/json"));
        response.json().await.unwrap()
    }

    #[tokio::test]
    async fn streamable_http_initializes_and_lists_ten_tools() {
        let (url, task) = spawn_test_server().await;
        let client = reqwest::Client::new();

        let initialized = post_rpc(
            &client,
            &url,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "itsm-manager-test", "version": "1.0.0"}
                }
            }),
            None,
        )
        .await;
        assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");

        let listed = post_rpc(
            &client,
            &url,
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list",
                "params": {}
            }),
            Some("2025-11-25"),
        )
        .await;
        let tools = listed["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 10);
        assert!(tools.iter().any(|tool| tool["name"] == "list_views"));
        assert!(tools.iter().any(|tool| tool["name"] == "resolve"));

        task.abort();
        let _ = task.await;
    }
}
