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
    default_support_group: Option<(String, String)>,
}

impl ItsmHandler {
    pub fn new(
        token: TokenStore,
        client: reqwest::Client,
        default_seach_type: i64,
        default_support_group: Option<(String, String)>,
    ) -> Self {
        Self { token, client, default_seach_type, default_support_group }
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateTicketParams {
    #[schemars(description = "二级服务目录 stId（list_service_tree 返回的二级 serviceType[].stId）")]
    service_type: String,
    #[schemars(description = "三级服务目录叶子 stId（list_service_tree 返回的三级 children[].stId）；同时作为 get_replenish_template 的 leaf_id")]
    service_sub_type: String,
    #[schemars(description = "工单主题")]
    order_subject: String,
    #[schemars(description = "详细描述，HTML 格式：换行 <br>、段落 <p>、重点 <strong>、列表 <ul>/<li>、表格 <table>")]
    detail: String,
    #[schemars(description = "客户组 cgId（search_customer_groups 返回的 cgId）")]
    contact_customer_group: String,
    #[schemars(description = "客户组名称（与 contact_customer_group 配套传入）")]
    contact_customer_group_name: String,
    #[schemars(description = "提单人 userId（search_base_persons 返回的 userId）")]
    requestor: String,
    #[schemars(description = "提单人名称（与 requestor 配套传入）")]
    requestor_name: String,
    #[schemars(description = "支持组 sgId（list_support_groups 返回的 sgId）。省略时用应用设置中的默认支持组")]
    assign: Option<String>,
    #[schemars(description = "支持组名称（与 assign 配套传入）")]
    assign_name: Option<String>,
    #[schemars(description = "支持人 userId（search_base_persons 返回的 userId）。可选")]
    support_by: Option<String>,
    #[schemars(description = "支持人名称（与 support_by 配套传入）。可选")]
    support_name: Option<String>,
    #[schemars(description = "补单模板 id（get_replenish_template 返回的 data.id）。建议先调 get_replenish_template 取值")]
    create_template_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetReplenishTemplateParams {
    #[schemars(description = "三级服务目录叶子 stId（即 create_ticket 的 service_sub_type）")]
    leaf_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchCustomerGroupsParams {
    #[schemars(description = "客户组名称关键字")]
    keyword: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchBasePersonsParams {
    #[schemars(description = "人员姓名/工号关键字（提单人、支持人均用本工具查询）")]
    keyword: String,
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

/// 由入参与应用默认支持组组装 `save_replenish` 的 params body（纯函数，便于单测）。
/// 返回值即 `api::save_replenish` 的 `params`。
/// 必填缺空 → invalid_params；assign 未传且无默认 → invalid_params；
/// 可选 ID/name 仅传其一（配套校验）→ invalid_params。
fn build_replenish_params(
    params: &CreateTicketParams,
    default_support_group: Option<&(String, String)>,
) -> Result<Value, McpError> {
    let service_type = required_text("service_type", params.service_type.clone())?;
    let service_sub_type = required_text("service_sub_type", params.service_sub_type.clone())?;
    let order_subject = required_text("order_subject", params.order_subject.clone())?;
    let detail = required_text("detail", params.detail.clone())?;
    let contact_customer_group = required_text("contact_customer_group", params.contact_customer_group.clone())?;
    let contact_customer_group_name = required_text("contact_customer_group_name", params.contact_customer_group_name.clone())?;
    let requestor = required_text("requestor", params.requestor.clone())?;
    let requestor_name = required_text("requestor_name", params.requestor_name.clone())?;

    fn pair(
        id: Option<&str>,
        name: Option<&str>,
        id_field: &str,
        name_field: &str,
    ) -> Result<(String, String), McpError> {
        let id = id.map(str::trim).filter(|s| !s.is_empty());
        let name = name.map(str::trim).filter(|s| !s.is_empty());
        match (id, name) {
            (Some(i), Some(n)) => Ok((i.to_string(), n.to_string())),
            (None, None) => Ok((String::new(), String::new())),
            _ => Err(McpError::invalid_params(
                format!("{id_field} 与 {name_field} 必须同时传入或同时省略"),
                None,
            )),
        }
    }

    let (assign_in, assign_name_in) = pair(params.assign.as_deref(), params.assign_name.as_deref(), "assign", "assign_name")?;
    let (assign, assign_name) = if !assign_in.is_empty() {
        (assign_in, assign_name_in)
    } else if let Some((id, name)) = default_support_group {
        (id.clone(), name.clone())
    } else {
        return Err(McpError::invalid_params(
            "未配置默认支持组，请在应用设置中配置，或显式传入 assign 与 assign_name".to_string(),
            None,
        ));
    };

    let (support_by, support_name) = pair(params.support_by.as_deref(), params.support_name.as_deref(), "support_by", "support_name")?;
    let create_template_id = params
        .create_template_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_default();

    Ok(json!({
        "serviceType": service_type,
        "serviceSubType": service_sub_type,
        "orderSubject": order_subject,
        "detail": detail,
        "fileIds": [],
        "priority": "3",
        "contactCustomerGroup": contact_customer_group,
        "requestor": requestor,
        "assign": assign,
        "supportBy": support_by,
        "effect": "4",
        "urgency": "1",
        "cc": [],
        "orderSign": 1,
        "contactCustomerGroupName": contact_customer_group_name,
        "requestorName": requestor_name,
        "assignName": assign_name,
        "assignLevel": 1,
        "supportName": support_name,
        "relatedorderList": [],
        "createTemplateId": create_template_id,
    }))
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
        description = "列出三级服务目录树：一级大类 → 二级 serviceType(stId) → 三级 children[](stId)。补单时取二级 stId 作 service_type、三级叶子 stId 作 service_sub_type。",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true)
    )]
    async fn list_service_tree(&self) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let value = api::list_service_tree(&self.client, &token)
            .await
            .map_err(api_error)?;
        Ok(json_result(value))
    }

    #[tool(
        description = "按三级服务目录叶子 stId 取补单模板；返回的 data.id 作为 create_ticket 的 create_template_id。",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true)
    )]
    async fn get_replenish_template(
        &self,
        Parameters(params): Parameters<GetReplenishTemplateParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let leaf_id = required_text("leaf_id", params.leaf_id)?;
        let value = api::get_replenish_template(&self.client, &token, &leaf_id)
            .await
            .map_err(api_error)?;
        Ok(json_result(value))
    }

    #[tool(
        description = "按关键字模糊搜索客户组；返回 cgId 与 customerGroupName，作为 create_ticket 的 contact_customer_group / contact_customer_group_name。",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true)
    )]
    async fn search_customer_groups(
        &self,
        Parameters(params): Parameters<SearchCustomerGroupsParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let keyword = required_text("keyword", params.keyword)?;
        let value = api::search_customer_groups(&self.client, &token, &keyword)
            .await
            .map_err(api_error)?;
        Ok(json_result(value))
    }

    #[tool(
        description = "按关键字模糊搜索人员；返回 userId 与 psnName。提单人(create_ticket 的 requestor)与支持人(support_by)均用本工具查询。",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true)
    )]
    async fn search_base_persons(
        &self,
        Parameters(params): Parameters<SearchBasePersonsParams>,
    ) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let keyword = required_text("keyword", params.keyword)?;
        let value = api::search_base_persons(&self.client, &token, &keyword)
            .await
            .map_err(api_error)?;
        Ok(json_result(value))
    }

    #[tool(
        description = "列出全部支持组；返回 sgId 与 supportGroupName，作为 create_ticket 的 assign / assign_name（也可不传 assign 走应用默认支持组）。",
        annotations(read_only_hint = true, destructive_hint = false, idempotent_hint = true, open_world_hint = true)
    )]
    async fn list_support_groups(&self) -> Result<CallToolResult, McpError> {
        let token = self.token()?;
        let value = api::list_support_groups(&self.client, &token)
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

fn build_router(token: TokenStore, client: reqwest::Client, default_seach_type: i64, default_support_group: Option<(String, String)>) -> axum::Router {
    let config = StreamableHttpServerConfig::default()
        .with_stateful_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None);
    let service: StreamableHttpService<ItsmHandler, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(ItsmHandler::new(token.clone(), client.clone(), default_seach_type, default_support_group.clone())),
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
    default_support_group: Option<(String, String)>,
) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|error| format!("绑定 127.0.0.1:{port} 失败: {error}"))?;
    println!("[mcp] listening on http://127.0.0.1:{port}/mcp");
    axum::serve(listener, build_router(token, client, default_seach_type, default_support_group))
        .await
        .map_err(|error| format!("MCP server 退出: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> ItsmHandler {
        ItsmHandler::new(TokenStore::default(), reqwest::Client::new(), 7, None)
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
                "get_replenish_template",
                "get_ticket_by_code",
                "list_replies",
                "list_service_tree",
                "list_support_groups",
                "list_views",
                "reply",
                "resolve",
                "search_base_persons",
                "search_customer_groups",
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
        let router = build_router(TokenStore::default(), reqwest::Client::new(), 7, None);
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
        assert_eq!(tools.len(), 15);
        assert!(tools.iter().any(|tool| tool["name"] == "list_views"));
        assert!(tools.iter().any(|tool| tool["name"] == "resolve"));

        task.abort();
        let _ = task.await;
    }

    fn cgp(overrides: impl FnOnce(&mut CreateTicketParams)) -> CreateTicketParams {
        let mut p = CreateTicketParams {
            service_type: "ST1".into(),
            service_sub_type: "SST1".into(),
            order_subject: "主题".into(),
            detail: "<p>描述</p>".into(),
            contact_customer_group: "CG1".into(),
            contact_customer_group_name: "客户组A".into(),
            requestor: "RQ1".into(),
            requestor_name: "张三".into(),
            assign: None,
            assign_name: None,
            support_by: None,
            support_name: None,
            create_template_id: None,
        };
        overrides(&mut p);
        p
    }

    #[test]
    fn build_replenish_params_missing_required_returns_error() {
        let p = cgp(|p| p.order_subject = "  ".into());
        let err = build_replenish_params(&p, None).unwrap_err();
        assert!(err.message.contains("order_subject"), "实际: {}", err.message);
    }

    #[test]
    fn build_replenish_params_fills_assign_from_config() {
        let p = cgp(|_| {});
        let v = build_replenish_params(&p, Some(&("SG1".into(), "支持组X".into()))).unwrap();
        assert_eq!(v["assign"], "SG1");
        assert_eq!(v["assignName"], "支持组X");
    }

    #[test]
    fn build_replenish_params_missing_assign_no_config_returns_error() {
        let p = cgp(|_| {});
        let err = build_replenish_params(&p, None).unwrap_err();
        assert!(err.message.contains("默认支持组"), "实际: {}", err.message);
    }

    #[test]
    fn build_replenish_params_overrides_assign() {
        let p = cgp(|p| {
            p.assign = Some("SG9".into());
            p.assign_name = Some("自定义组".into());
        });
        let v = build_replenish_params(&p, Some(&("SG1".into(), "默认组".into()))).unwrap();
        assert_eq!(v["assign"], "SG9");
        assert_eq!(v["assignName"], "自定义组");
    }

    #[test]
    fn build_replenish_params_pair_check() {
        let p = cgp(|p| p.assign = Some("SG9".into()));
        let err = build_replenish_params(&p, Some(&("SG1".into(), "默认组".into()))).unwrap_err();
        assert!(err.message.contains("assign") && err.message.contains("assign_name"), "实际: {}", err.message);
    }

    #[test]
    fn build_replenish_params_hardcoded_fields() {
        let p = cgp(|_| {});
        let v = build_replenish_params(&p, Some(&("SG1".into(), "G".into()))).unwrap();
        assert_eq!(v["fileIds"], json!([]));
        assert_eq!(v["priority"], "3");
        assert_eq!(v["effect"], "4");
        assert_eq!(v["urgency"], "1");
        assert_eq!(v["cc"], json!([]));
        assert_eq!(v["orderSign"], 1);
        assert_eq!(v["assignLevel"], 1);
        assert_eq!(v["relatedorderList"], json!([]));
    }

    #[test]
    fn build_replenish_params_matches_frontend_shape() {
        let p = cgp(|p| {
            p.support_by = Some("U2".into());
            p.support_name = Some("李四".into());
            p.create_template_id = Some("TPL1".into());
        });
        let v = build_replenish_params(&p, Some(&("SG1".into(), "G".into()))).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        let expected = [
            "serviceType","serviceSubType","orderSubject","detail","fileIds","priority",
            "contactCustomerGroup","requestor","assign","supportBy","effect","urgency","cc",
            "orderSign","contactCustomerGroupName","requestorName","assignName","assignLevel",
            "supportName","relatedorderList","createTemplateId",
        ];
        for k in expected {
            assert!(keys.contains(&k), "缺字段 {k}");
        }
        assert_eq!(keys.len(), expected.len(), "字段数不匹配：{:?}", keys);
        assert_eq!(v["supportBy"], "U2");
        assert_eq!(v["supportName"], "李四");
        assert_eq!(v["createTemplateId"], "TPL1");
    }
}
