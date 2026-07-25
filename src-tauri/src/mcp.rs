// MCP 边界层：对外暴露 4 个只读 ITSM 工具，复用 api.rs HTTP 实现，零业务改动
use crate::api::{self, FetchError, SearchParams, AUTH_EXPIRED_ERR};
use crate::state::TokenStore;
use rmcp::{
    ErrorData as McpError,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars, tool, tool_router,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// MCP handler：持有共享 token + reqwest::Client，可被 axum 共享层 clone
#[derive(Clone)]
pub struct ItsmHandler {
    token: TokenStore,
    client: reqwest::Client,
}

impl ItsmHandler {
    pub fn new(token: TokenStore, client: reqwest::Client) -> Self {
        Self { token, client }
    }

    fn token(&self) -> Result<String, McpError> {
        self.token
            .get()
            .map_err(|message| McpError::internal_error(message, None))
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchByCodeParams {
    #[schemars(description = "视图 seachType；先调用 list_views 获取")]
    seach_type: i64,
    #[schemars(description = "工单号或主题关键字；按 codeAndSubject 模糊匹配")]
    keyword: String,
    #[schemars(description = "页码，从 1 开始；省略时为 1")]
    page_index: Option<i64>,
    #[schemars(description = "每页条数，范围 1..=200；省略时为 50")]
    page_size: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchByCustomerGroupParams {
    #[schemars(description = "视图 seachType；先调用 list_views 获取")]
    seach_type: i64,
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

fn required_text(field: &str, value: String) -> Result<String, McpError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(McpError::invalid_params(
            format!("{field} 不能为空"),
            None,
        ));
    }
    Ok(value)
}

fn pagination(
    page_index: Option<i64>,
    page_size: Option<i64>,
) -> Result<(i64, i64), McpError> {
    let page_index = page_index.unwrap_or(1);
    let page_size = page_size.unwrap_or(50);
    if page_index < 1 {
        return Err(McpError::invalid_params("page_index 必须 >= 1", None));
    }
    if !(1..=200).contains(&page_size) {
        return Err(McpError::invalid_params(
            "page_size 必须在 1..=200",
            None,
        ));
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
        let search = SearchParams {
            code_and_subject: Some(keyword),
            ..Default::default()
        };
        let (data, count) = api::fetch_tickets_raw(
            &self.client,
            &token,
            params.seach_type,
            page_index,
            page_size,
            Some(&search),
        )
        .await
        .map_err(fetch_error)?;
        Ok(json_result(json!({
            "data": data,
            "count": count,
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
        let search = SearchParams {
            contact_customer_group_name: Some(keyword),
            ..Default::default()
        };
        let (data, count) = api::fetch_tickets_raw(
            &self.client,
            &token,
            params.seach_type,
            page_index,
            page_size,
            Some(&search),
        )
        .await
        .map_err(fetch_error)?;
        Ok(json_result(json!({
            "data": data,
            "count": count,
            "page_index": page_index,
            "page_size": page_size,
        })))
    }

    #[tool(
        description = "按 incidentId 读取工单详情和动态字段；参数 id 必须是 incidentId，不是展示单号。",
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
        Ok(json_result(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> ItsmHandler {
        ItsmHandler::new(TokenStore::default(), reqwest::Client::new())
    }

    #[test]
    fn exposes_four_read_tools() {
        let names: Vec<String> = ItsmHandler::tool_router()
            .list_all()
            .into_iter()
            .map(|t| t.name.into_owned())
            .collect();
        assert_eq!(names, vec![
            "get_detail",
            "list_views",
            "search_tickets_by_code",
            "search_tickets_by_customer_group",
        ]);
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
}
