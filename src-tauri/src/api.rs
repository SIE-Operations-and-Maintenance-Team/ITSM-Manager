// ITSM API 封装（纯 HTTP 层，不依赖 Tauri State）
use serde_json::{json, Value};

const API_BASE: &str = "https://api-itsm.chinasie.com";

pub async fn do_get(
    client: &reqwest::Client,
    token: &str,
    path: &str,
) -> Result<Value, String> {
    let url = format!("{}{}", API_BASE, path);
    client
        .get(&url)
        .header("authorization", token)
        .header("language", "zh#cn")
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?
        .json::<Value>()
        .await
        .map_err(|e| format!("解析失败: {}", e))
}

pub async fn do_post(
    client: &reqwest::Client,
    token: &str,
    path: &str,
    body: Value,
) -> Result<Value, String> {
    let url = format!("{}{}", API_BASE, path);
    client
        .post(&url)
        .header("authorization", token)
        .header("language", "zh#cn")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {}", e))?
        .json::<Value>()
        .await
        .map_err(|e| format!("解析失败: {}", e))
}

pub async fn list_views(client: &reqwest::Client, token: &str) -> Result<Value, String> {
    do_get(
        client,
        token,
        "/api/itsm/incidentViewBase/findMyViewList?containCount=true&type=1",
    )
    .await
}

pub async fn list_tickets(
    client: &reqwest::Client,
    token: &str,
    seach_type: i64,
) -> Result<Value, String> {
    let body = json!({
        "pageIndex": 1, "pageRows": 200,
        "params": { "seachType": seach_type },
        "orderByBean": { "attributeName": "", "sortType": "" }
    });
    do_post(client, token, "/api/itsm/incidentService/find-pagination", body).await
}

pub async fn get_detail(
    client: &reqwest::Client,
    token: &str,
    id: &str,
) -> Result<Value, String> {
    do_get(
        client,
        token,
        &format!("/api/itsm/incidentService/get-with-fields?id={}", id),
    )
    .await
}

pub async fn list_replies(
    client: &reqwest::Client,
    token: &str,
    incident_id: &str,
) -> Result<Value, String> {
    do_get(
        client,
        token,
        &format!("/api/itsm/incidentService/find-replyList?incidentId={}", incident_id),
    )
    .await
}

pub async fn claim(client: &reqwest::Client, token: &str, id: &str) -> Result<Value, String> {
    let url = format!("{}/api/itsm/incidentService/snatch-order", API_BASE);
    let form = reqwest::multipart::Form::new().text("id", id.to_string());
    client
        .post(&url)
        .header("authorization", token)
        .header("language", "zh#cn")
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("接单失败: {}", e))?
        .json::<Value>()
        .await
        .map_err(|e| format!("解析失败: {}", e))
}

pub async fn reply(
    client: &reqwest::Client,
    token: &str,
    order_id: &str,
    detail: &str,
    is_private: bool,
    order_type: &str,
) -> Result<Value, String> {
    let body = json!({
        "params": {
            "orderId": order_id, "profile": "", "fileIds": [],
            "orderType": order_type,
            "detail": format!("<p>{}</p>", detail),
            "isPrivate": if is_private { 1 } else { 0 },
            "operationSource": "ITSM_DEVOPS", "replyType": 1
        }
    });
    do_post(client, token, "/api/itsm/incidentService/order-reply", body).await
}

pub async fn change_status(
    client: &reqwest::Client,
    token: &str,
    id: &str,
    status: &str,
    content: &str,
    is_resolve: bool,
) -> Result<Value, String> {
    let mut data = do_get(
        client,
        token,
        &format!("/api/itsm/incidentService/get-with-fields?id={}", id),
    )
    .await?
    .get("data")
    .cloned()
    .ok_or_else(|| "未取到工单数据".to_string())?;
    if let Some(obj) = data.as_object_mut() {
        obj.insert("status".into(), Value::String(status.to_string()));
        obj.insert("solution".into(), Value::String(format!("<p>{}</p>", content)));
        if is_resolve {
            obj.insert("statusName".into(), Value::String("已解决".into()));
            obj.insert("endCode".into(), Value::String("1".into()));
            obj.insert("accountability".into(), Value::String("3".into()));
            obj.insert("statusIndex".into(), json!(3));
        }
    } else {
        return Err("工单数据格式异常".into());
    }
    do_post(client, token, "/api/itsm/incidentService/update", json!({ "params": data })).await
}

// ============ 补单 / 转派 / 取消 / 关闭 ============

/// 服务目录树（3 级 cascader 数据源）
pub async fn list_service_tree(
    client: &reqwest::Client,
    token: &str,
) -> Result<Value, String> {
    do_get(
        client,
        token,
        "/api/bussconfig/service-config/findServiceTypeTree2?filterPermission=false&status=0&type=1",
    )
    .await
}

/// 补单模板（叶子服务目录 id → 模板字段 + createTemplateId）
pub async fn get_replenish_template(
    client: &reqwest::Client,
    token: &str,
    leaf_id: &str,
) -> Result<Value, String> {
    do_get(
        client,
        token,
        &format!("/api/bussconfig/service-config/template/replenish?id={}", leaf_id),
    )
    .await
}

/// 字典：dicType（PRIORITY / Request_Source / REQUEST_TYPE / Influence_Degree / Urgent_Degree / itsm_incident_allot_type）
pub async fn get_dict(
    client: &reqwest::Client,
    token: &str,
    tenant_id: &str,
    dic_type: &str,
) -> Result<Value, String> {
    do_get(
        client,
        token,
        &format!(
            "/dictionary/dic-items/get-dictype-notoken?dicType={}&tenantId={}",
            dic_type, tenant_id
        ),
    )
    .await
}

/// 全支持组列表
pub async fn list_support_groups(
    client: &reqwest::Client,
    token: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/bussconfig/support-group/find-list",
        json!({ "params": { "groupState": 0 } }),
    )
    .await
}

/// 全支持组成员（前端按 sgId 过滤出某组成员）
pub async fn list_support_members(
    client: &reqwest::Client,
    token: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/bussconfig/support-group-member/find-alllist",
        json!({ "params": {} }),
    )
    .await
}

/// 客户组搜索（关键字为空时返回首页列表）
pub async fn search_customer_groups(
    client: &reqwest::Client,
    token: &str,
    keyword: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/bussconfig/customer-group/find-pagination",
        json!({
            "pageIndex": 1,
            "pageRows": 20,
            "params": { "groupState": "0", "customerGroupName": keyword }
        }),
    )
    .await
}

/// 人员搜索（关键字为空时返回首页列表）
pub async fn search_base_persons(
    client: &reqwest::Client,
    token: &str,
    keyword: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/base/base-person/find-pagination-all",
        json!({
            "pageIndex": 1,
            "pageRows": 20,
            "params": { "useState": "0", "psnName": keyword }
        }),
    )
    .await
}

/// 补单提交：params 为前端组装的完整表单
pub async fn save_replenish(
    client: &reqwest::Client,
    token: &str,
    params: Value,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/itsm/incidentService/save",
        json!({ "params": params }),
    )
    .await
}

/// 转派提交：params 为前端组装的 {incidentId, assign, supportBy, allotType, ...}
pub async fn reassign(
    client: &reqwest::Client,
    token: &str,
    params: Value,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/itsm/incident-bpm/order-reply",
        json!({ "params": params }),
    )
    .await
}

/// 取消工单：无 params 包裹，body 直接是 {incidentId, operationReason}
pub async fn cancel_incident(
    client: &reqwest::Client,
    token: &str,
    incident_id: &str,
    reason: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/itsm/incidentAction/delete-incident",
        json!({ "incidentId": incident_id, "operationReason": reason }),
    )
    .await
}

/// 关闭工单：仅 incidentId，无 reason
pub async fn close_incident(
    client: &reqwest::Client,
    token: &str,
    incident_id: &str,
) -> Result<Value, String> {
    do_post(
        client,
        token,
        "/api/itsm/incidentAction/close-incident",
        json!({ "params": { "incidentId": incident_id } }),
    )
    .await
}

// ============ 带错误分类的列表拉取（scheduler 专用） ============

#[derive(Debug, Clone, PartialEq)]
pub enum RefreshError {
    Network,
    Auth,
    Server,
}

#[derive(Debug)]
pub enum FetchError {
    Network(String),
    Auth,
    Server(String),
}

/// 纯分类：依据 status / 是否连接超时 / 是否解析失败
pub fn classify(status_code: Option<u16>, is_connect_or_timeout: bool, parse_failed: bool) -> RefreshError {
    if matches!(status_code, Some(401)) {
        return RefreshError::Auth;
    }
    if is_connect_or_timeout {
        return RefreshError::Network;
    }
    if parse_failed {
        return RefreshError::Server;
    }
    match status_code {
        Some(c) if (500..600).contains(&c) => RefreshError::Server,
        None => RefreshError::Network,
        Some(_) => RefreshError::Server,
    }
}

pub fn map_fetch_error(e: &FetchError) -> RefreshError {
    match e {
        FetchError::Network(_) => RefreshError::Network,
        FetchError::Auth => RefreshError::Auth,
        FetchError::Server(_) => RefreshError::Server,
    }
}

/// 纯解析：list_tickets 响应 → (data 数组, count)
pub fn parse_tickets_response(v: &Value) -> Result<(Value, i64), FetchError> {
    let data = v.get("data").ok_or_else(|| FetchError::Server("响应缺 data".into()))?;
    let arr = data.get("data").cloned().unwrap_or(Value::Array(vec![]));
    let count = data.get("count").and_then(|c| c.as_i64()).unwrap_or(0);
    Ok((arr, count))
}

pub async fn fetch_tickets_raw(
    client: &reqwest::Client,
    token: &str,
    seach_type: i64,
    page_index: i64,
    page_rows: i64,
) -> Result<(Value, i64), FetchError> {
    let body = json!({
        "pageIndex": page_index, "pageRows": page_rows,
        "params": { "seachType": seach_type },
        "orderByBean": { "attributeName": "", "sortType": "" }
    });
    let url = format!("{}/api/itsm/incidentService/find-pagination", API_BASE);
    let resp = client
        .post(&url)
        .header("authorization", token)
        .header("language", "zh#cn")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                FetchError::Network(e.to_string())
            } else {
                FetchError::Server(e.to_string())
            }
        })?;
    let status = resp.status().as_u16();
    if classify(Some(status), false, false) == RefreshError::Auth {
        return Err(FetchError::Auth);
    }
    if !(200..300).contains(&status) {
        return Err(FetchError::Server(format!("HTTP {}", status)));
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| FetchError::Server(format!("解析失败: {}", e)))?;
    parse_tickets_response(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classify_401_is_auth() {
        assert_eq!(classify(Some(401), false, false), RefreshError::Auth);
    }

    #[test]
    fn classify_connect_is_network() {
        assert_eq!(classify(None, true, false), RefreshError::Network);
    }

    #[test]
    fn classify_503_is_server() {
        assert_eq!(classify(Some(503), false, false), RefreshError::Server);
    }

    #[test]
    fn classify_parse_fail_is_server() {
        assert_eq!(classify(Some(200), false, true), RefreshError::Server);
    }

    #[test]
    fn classify_400_is_server() {
        assert_eq!(classify(Some(400), false, false), RefreshError::Server);
    }

    #[test]
    fn map_network() {
        assert_eq!(map_fetch_error(&FetchError::Network("x".into())), RefreshError::Network);
    }

    #[test]
    fn map_auth() {
        assert_eq!(map_fetch_error(&FetchError::Auth), RefreshError::Auth);
    }

    #[test]
    fn map_server() {
        assert_eq!(map_fetch_error(&FetchError::Server("y".into())), RefreshError::Server);
    }

    #[test]
    fn parse_normal() {
        let v = json!({"code": 800, "data": {"data": [{"id": "A"}], "count": 5}});
        let (arr, count) = parse_tickets_response(&v).unwrap();
        assert_eq!(count, 5);
        assert_eq!(arr, json!([{"id": "A"}]));
    }

    #[test]
    fn parse_missing_data_is_error() {
        let v = json!({"code": 800});
        assert!(parse_tickets_response(&v).is_err());
    }

    #[test]
    fn parse_missing_count_defaults_zero() {
        let v = json!({"data": {"data": []}});
        let (_, count) = parse_tickets_response(&v).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn parse_missing_inner_data_defaults_empty() {
        let v = json!({"data": {"count": 3}});
        let (arr, count) = parse_tickets_response(&v).unwrap();
        assert_eq!(count, 3);
        assert_eq!(arr, json!([]));
    }
}
